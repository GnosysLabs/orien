//! Files a person holds: the genesis file, validator keys and wallets.

use crate::phrase;
use crate::store::write_replacing;
use anyhow::{Context, Result, anyhow, bail};
use orien_crypto::keytree::KeyTree;
use orien_crypto::{Hash32, Kdf, ots, prf};
use orien_ledger::{
    AccountId, Fees, Genesis, Kind, Order, SPARE_HEIGHT, Signature, SignedBy, SignedOrder, Signer, address,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub fn random_seed() -> [u8; 32] {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).expect("the operating system could not provide randomness");
    seed
}

fn parse_seed(text: &str) -> Result<[u8; 32]> {
    let mut seed = [0u8; 32];
    hex::decode_to_slice(text.trim(), &mut seed).map_err(|_| anyhow!("seed is not 64 hex characters"))?;
    Ok(seed)
}

pub fn load_genesis(path: &Path) -> Result<Genesis> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("cannot read genesis file {}", path.display()))?;
    let genesis: Genesis = serde_json::from_str(&text)
        .with_context(|| format!("{} is not a genesis file", path.display()))?;
    genesis.validate().map_err(|e| anyhow!("invalid genesis file: {e}"))?;
    Ok(genesis)
}

pub fn save_genesis(path: &Path, genesis: &Genesis) -> Result<()> {
    fs::write(path, serde_json::to_string_pretty(genesis)? + "\n")?;
    Ok(())
}

/// A validator's secret: the seed of its key tree.
#[derive(Clone)]
pub struct ValidatorKey {
    pub seed: [u8; 32],
    pub height: u8,
}

#[derive(Serialize, Deserialize)]
struct ValidatorKeyFile {
    seed: String,
    height: u8,
}

impl ValidatorKey {
    pub fn generate(height: u8) -> ValidatorKey {
        ValidatorKey { seed: random_seed(), height }
    }

    pub fn load(path: &Path) -> Result<ValidatorKey> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("cannot read validator key {}", path.display()))?;
        let file: ValidatorKeyFile = serde_json::from_str(&text)
            .with_context(|| format!("{} is not a validator key", path.display()))?;
        Ok(ValidatorKey { seed: parse_seed(&file.seed)?, height: file.height })
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let file = ValidatorKeyFile { seed: hex::encode(self.seed), height: self.height };
        write_replacing(path, (serde_json::to_string_pretty(&file)? + "\n").as_bytes(), true)?;
        Ok(())
    }
}

/// A wallet: one seed, one account.
///
/// Everything comes from the seed: the one-time keys that sign payments,
/// drawn one after another, and a separate small set of spare keys. The
/// address commits to the first key and to the spares, so both are fixed
/// before anything is signed.
///
/// The wallet's job beyond holding the seed is to make sure each one-time
/// key signs at most one order. It writes every order it signs to disk
/// before the order leaves the process, and afterwards will only ever
/// re-send that same order under that payment number.
pub struct Wallet {
    path: PathBuf,
    seed: [u8; 32],
    /// The secret the recovery phrase holds, and which of its accounts this
    /// wallet is. Wallets made before recovery phrases have neither.
    master: Option<[u8; 32]>,
    account: u32,
    /// The secret, sealed with the owner's password, when the wallet has
    /// one. Kept so that saving rewrites the same sealed secret.
    sealed: Option<Sealed>,
    /// The spare keys. Used only after a checkpoint cancels what a key that
    /// signed twice had signed.
    spares: KeyTree,
    address: AccountId,
    next_seq: u64,
    pending: Option<SignedOrder>,
    /// What each one-time key still in use has signed, by payment number,
    /// and what each spare key has signed, by its number: the digest of the
    /// order. Kept even after a pending order is let go, so that no key
    /// ever signs two different orders, whatever the wallet is asked.
    used: BTreeMap<u64, Hash32>,
    used_spares: BTreeMap<u32, Hash32>,
    /// The network's fee rule, which sets the largest fee each order
    /// accepts. Set from the genesis file with `accept_fee`; no fee until
    /// then.
    fees: Fees,
}

#[derive(Serialize, Deserialize)]
struct WalletFile {
    /// The recovery phrase's secret, as hex, and the account number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    master: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    account: u32,
    /// The account's own secret, as hex. Empty when the wallet has a
    /// password: `sealed` holds it then.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    seed: String,
    /// The address, in the clear when the secret is sealed, so that a
    /// balance can be read without the password.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sealed: Option<Sealed>,
    /// Payments known to be final. The next payment carries this number.
    next_seq: u64,
    /// A signed order not yet known to be final, as hex.
    #[serde(default)]
    pending: Option<String>,
    /// The digest each one-time key signed, by payment number, as hex.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    used: BTreeMap<u64, String>,
    /// The digest each spare key signed, by its number, as hex.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    used_spares: BTreeMap<u32, String>,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// A wallet's secret sealed with a password: the password is stretched
/// with Argon2id, slow and memory-hungry on purpose so that guessing is
/// impractical, and the secret encrypted with XChaCha20-Poly1305, which also
/// detects any change to it. Both are well-reviewed, widely used designs.
/// This protects the file on the owner's computer only; nothing on the
/// network depends on it.
#[derive(Clone, Serialize, Deserialize)]
struct Sealed {
    /// Argon2id's memory, in KiB, and passes.
    memory: u32,
    passes: u32,
    salt: String,
    nonce: String,
    secret: String,
}

/// What a sealed wallet keeps secret.
#[derive(Serialize, Deserialize)]
struct Secret {
    seed: String,
    #[serde(default)]
    master: Option<String>,
}

/// Why a wallet would not open.
#[derive(Debug)]
pub enum OpenError {
    /// It has a password, and none was given.
    NeedsPassword,
    /// The password given is not this wallet's.
    WrongPassword,
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::NeedsPassword => f.write_str("this wallet has a password"),
            OpenError::WrongPassword => f.write_str("that is not this wallet's password"),
        }
    }
}

impl std::error::Error for OpenError {}

const ARGON_MEMORY: u32 = 64 * 1024;
const ARGON_PASSES: u32 = 3;

fn stretch(password: &str, salt: &[u8], memory: u32, passes: u32) -> Result<[u8; 32]> {
    use argon2::{Algorithm, Argon2, Params, Version};
    let params = Params::new(memory, passes, 1, Some(32)).map_err(|e| anyhow!("{e}"))?;
    let mut key = [0u8; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|e| anyhow!("{e}"))?;
    Ok(key)
}

fn seal(secret: &Secret, password: &str, address: &AccountId) -> Result<Sealed> {
    use chacha20poly1305::aead::{Aead, KeyInit, Payload};
    use chacha20poly1305::{XChaCha20Poly1305, XNonce};
    let salt = random_seed();
    let nonce: [u8; 24] = random_seed()[..24].try_into().expect("24 of 32 bytes");
    let key = stretch(password, &salt, ARGON_MEMORY, ARGON_PASSES)?;
    let plain = serde_json::to_vec(secret)?;
    // The address is bound in, so a sealed secret cannot be moved to
    // another wallet file unnoticed.
    let sealed = XChaCha20Poly1305::new(&key.into())
        .encrypt(XNonce::from_slice(&nonce), Payload { msg: &plain, aad: address.as_bytes() })
        .map_err(|_| anyhow!("could not seal the wallet"))?;
    Ok(Sealed {
        memory: ARGON_MEMORY,
        passes: ARGON_PASSES,
        salt: hex::encode(salt),
        nonce: hex::encode(nonce),
        secret: hex::encode(sealed),
    })
}

fn unseal(sealed: &Sealed, password: &str, address: &AccountId) -> Result<Secret> {
    use chacha20poly1305::aead::{Aead, KeyInit, Payload};
    use chacha20poly1305::{XChaCha20Poly1305, XNonce};
    let salt = hex::decode(&sealed.salt).context("damaged wallet")?;
    let nonce = hex::decode(&sealed.nonce).context("damaged wallet")?;
    let secret = hex::decode(&sealed.secret).context("damaged wallet")?;
    if nonce.len() != 24 {
        bail!("damaged wallet");
    }
    let key = stretch(password, &salt, sealed.memory, sealed.passes)?;
    let plain = XChaCha20Poly1305::new(&key.into())
        .decrypt(XNonce::from_slice(&nonce), Payload { msg: &secret, aad: address.as_bytes() })
        .map_err(|_| OpenError::WrongPassword)?;
    Ok(serde_json::from_slice(&plain)?)
}

fn wallet_key(seed: &[u8; 32], index: u64) -> ots::SecretKey {
    ots::SecretKey::from_seed(prf(seed, Kdf::WalletKey, index))
}

/// The spare keys come from their own stream of the seed, so no ordinary
/// key is ever also a spare.
fn wallet_spares(seed: &[u8; 32]) -> KeyTree {
    KeyTree::generate(prf(seed, Kdf::WalletSpare, 0), SPARE_HEIGHT)
}

impl Wallet {
    /// A new wallet: a fresh recovery phrase, and its account 1.
    pub fn create(path: &Path) -> Result<Wallet> {
        Wallet::from_master(path, random_seed(), 1)
    }

    /// The wallet for account `account` of the recovery phrase holding
    /// `master`. Each computer should use its own account.
    pub fn from_master(path: &Path, master: [u8; 32], account: u32) -> Result<Wallet> {
        if path.exists() {
            bail!("{} already exists; refusing to overwrite a wallet", path.display());
        }
        if account == 0 {
            bail!("accounts are numbered from 1");
        }
        let seed = phrase::account_seed(&master, account);
        let spares = wallet_spares(&seed);
        let wallet = Wallet {
            path: path.to_owned(),
            seed,
            master: Some(master),
            account,
            sealed: None,
            address: address(&wallet_key(&seed, 0).commitment(), &spares.root()),
            spares,
            next_seq: 0,
            pending: None,
            used: BTreeMap::new(),
            used_spares: BTreeMap::new(),
            fees: Fees::NONE,
        };
        wallet.save()?;
        Ok(wallet)
    }

    /// Opens a wallet without a password. A wallet that has one is refused
    /// with `OpenError::NeedsPassword`; use `open`.
    pub fn load(path: &Path) -> Result<Wallet> {
        Wallet::open(path, None)
    }

    /// Opens a wallet, with its password if it has one.
    pub fn open(path: &Path, password: Option<&str>) -> Result<Wallet> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("cannot read wallet {}", path.display()))?;
        let mut file: WalletFile = serde_json::from_str(&text)
            .with_context(|| format!("{} is not a wallet file", path.display()))?;
        if let Some(sealed) = &file.sealed {
            let Some(password) = password else { return Err(OpenError::NeedsPassword.into()) };
            let address: AccountId =
                file.address.as_deref().unwrap_or_default().parse().map_err(|_| anyhow!("damaged wallet"))?;
            let secret = unseal(sealed, password, &address)?;
            file.seed = secret.seed;
            file.master = secret.master;
        }
        let seed = parse_seed(&file.seed)?;
        let master = file.master.as_deref().map(parse_seed).transpose()?;
        if let Some(master) = &master
            && phrase::account_seed(master, file.account) != seed
        {
            bail!("{} is damaged: its account does not come from its recovery phrase", path.display());
        }
        let pending = match file.pending {
            Some(text) => {
                let bytes = hex::decode(text).context("wallet holds a corrupt pending payment")?;
                Some(borsh::from_slice(&bytes).context("wallet holds a corrupt pending payment")?)
            }
            None => None,
        };
        let digest = |text: &String| -> Result<Hash32> {
            text.parse().map_err(|_| anyhow!("{} records a damaged digest", path.display()))
        };
        let used = file.used.iter().map(|(seq, text)| Ok((*seq, digest(text)?))).collect::<Result<_>>()?;
        let used_spares =
            file.used_spares.iter().map(|(leaf, text)| Ok((*leaf, digest(text)?))).collect::<Result<_>>()?;
        let spares = wallet_spares(&seed);
        Ok(Wallet {
            path: path.to_owned(),
            seed,
            master,
            account: file.account,
            sealed: file.sealed,
            address: address(&wallet_key(&seed, 0).commitment(), &spares.root()),
            spares,
            next_seq: file.next_seq,
            pending,
            used,
            used_spares,
            fees: Fees::NONE,
        })
    }

    fn save(&self) -> Result<()> {
        let pending = match &self.pending {
            Some(order) => Some(hex::encode(borsh::to_vec(order)?)),
            None => None,
        };
        let used: BTreeMap<u64, String> = self.used.iter().map(|(seq, digest)| (*seq, digest.to_string())).collect();
        let used_spares: BTreeMap<u32, String> =
            self.used_spares.iter().map(|(leaf, digest)| (*leaf, digest.to_string())).collect();
        // With a password only the sealed secret is written, and the
        // address beside it in the clear.
        let file = match &self.sealed {
            Some(sealed) => WalletFile {
                master: None,
                account: self.account,
                seed: String::new(),
                address: Some(self.address.to_string()),
                sealed: Some(sealed.clone()),
                next_seq: self.next_seq,
                pending,
                used,
                used_spares,
            },
            None => WalletFile {
                master: self.master.map(hex::encode),
                account: self.account,
                seed: hex::encode(self.seed),
                address: None,
                sealed: None,
                next_seq: self.next_seq,
                pending,
                used,
                used_spares,
            },
        };
        write_replacing(&self.path, (serde_json::to_string_pretty(&file)? + "\n").as_bytes(), true)
            .with_context(|| format!("cannot write wallet {}", self.path.display()))
    }

    /// One-time key number `index`. Key `n` signs payment `n` and nothing else.
    pub fn key(&self, index: u64) -> ots::SecretKey {
        wallet_key(&self.seed, index)
    }

    /// Puts a password on the wallet, replaces it, or with `None` removes
    /// it, and saves the wallet.
    pub fn set_password(&mut self, password: Option<&str>) -> Result<()> {
        self.sealed = match password {
            Some(password) => {
                let secret = Secret { seed: hex::encode(self.seed), master: self.master.map(hex::encode) };
                Some(seal(&secret, password, &self.address)?)
            }
            None => None,
        };
        self.save()
    }

    /// The wallet file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn has_password(&self) -> bool {
        self.sealed.is_some()
    }

    /// A wallet's address and how many of its payments it knows are final,
    /// read without its password.
    pub fn peek(path: &Path) -> Result<(AccountId, u64)> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("cannot read wallet {}", path.display()))?;
        let file: WalletFile = serde_json::from_str(&text)
            .with_context(|| format!("{} is not a wallet file", path.display()))?;
        if let Some(address) = &file.address {
            let address = address.parse().map_err(|_| anyhow!("damaged wallet"))?;
            return Ok((address, file.next_seq));
        }
        let wallet = Wallet::load(path)?;
        Ok((wallet.address(), wallet.next_seq()))
    }

    /// The recovery phrase, for a wallet made with one. Whoever reads it
    /// can spend from every account it makes.
    pub fn phrase(&self) -> Option<Vec<&'static str>> {
        self.master.as_ref().map(phrase::to_words)
    }

    /// Which account of its recovery phrase this wallet is, if it has one.
    pub fn account_number(&self) -> Option<u32> {
        self.master.map(|_| self.account)
    }

    pub fn address(&self) -> AccountId {
        self.address
    }

    /// The root of this wallet's spare keys.
    pub fn spare(&self) -> Hash32 {
        self.spares.root()
    }

    /// The order this wallet would sign for payment number `seq`. It is not
    /// signed and nothing is recorded.
    pub fn order(&self, seq: u64, to: AccountId, amount: u64) -> Order {
        self.order_of(Kind::Payment, seq, to, amount)
    }

    /// The order of this kind this wallet would sign for number `seq`.
    pub fn order_of(&self, kind: Kind, seq: u64, to: AccountId, amount: u64) -> Order {
        let max_fee = self.fees.on(&kind, amount);
        Order {
            kind,
            from: self.address,
            seq,
            to,
            amount,
            max_fee,
            next_key: self.key(seq + 1).commitment(),
            spare: self.spares.root(),
        }
    }

    /// Lets this wallet's orders pay the network's fee, from its genesis
    /// file. Each order accepts exactly the fee its amount calls for.
    pub fn accept_fee(&mut self, fees: Fees) {
        self.fees = fees;
    }

    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }

    pub fn pending(&self) -> Option<&SignedOrder> {
        self.pending.as_ref()
    }

    /// Signs payment number `seq`. If this wallet already signed an order
    /// under that number, the same order comes back when the details match,
    /// and an error when they do not.
    ///
    /// `signer` is the key a checkpoint proves the account must use, where
    /// one does. After a cancellation that is a numbered spare key.
    pub fn sign_order(
        &mut self,
        seq: u64,
        to: AccountId,
        amount: u64,
        signer: Option<&Signer>,
        network: &Hash32,
    ) -> Result<SignedOrder> {
        self.sign(Kind::Payment, seq, to, amount, signer, network)
    }

    /// `sign_order` for an order of any kind: a stake or unstake order names
    /// the wallet's own account as `to`.
    pub fn sign(
        &mut self,
        kind: Kind,
        seq: u64,
        to: AccountId,
        amount: u64,
        signer: Option<&Signer>,
        network: &Hash32,
    ) -> Result<SignedOrder> {
        // An order signed with a key that a checkpoint has since retired is
        // dead. It no longer blocks its payment number.
        if let (Some(Signer::Spare(leaf)), Some(pending)) = (signer, &self.pending)
            && pending.order.seq == seq
            && !matches!(&pending.signature, Signature::Spare(signature) if signature.leaf == *leaf)
        {
            self.retire_pending();
        }
        if let Some(pending) = &self.pending {
            if pending.order.seq == seq {
                if pending.order.kind == kind && pending.order.to == to && pending.order.amount == amount {
                    return Ok(pending.clone());
                }
                bail!(
                    "payment #{seq} is already signed ({} to {}) and not yet final. Its one-time \
                     key cannot sign a different payment. Finish that one first.",
                    pending.order.amount,
                    pending.order.to.short(),
                );
            }
            if pending.order.seq > seq {
                bail!("wallet has signed payment #{} but the network is at #{seq}", pending.order.seq);
            }
        }
        if seq < self.next_seq {
            bail!("payment #{seq} was already made from this wallet");
        }
        let order = self.order_of(kind, seq, to, amount);
        let digest = order.digest(network);
        // A one-time key that signs two different orders gives away enough to
        // forge a third. Whatever this wallet is asked, and whatever it has let
        // go of, no key of it signs a second, different order.
        let spare = matches!(signer, Some(Signer::Spare(_)));
        let earlier = match signer {
            Some(Signer::Spare(leaf)) => self.used_spares.get(leaf),
            _ => self.used.get(&seq),
        };
        if earlier.is_some_and(|earlier| *earlier != digest) {
            bail!(
                "this wallet's {} for payment #{seq} has already signed a different payment. Signing again could \
                 let someone forge a payment, so it will not.",
                if spare { "spare key" } else { "key" }
            );
        }
        let signature = match signer {
            Some(Signer::Spare(leaf)) if *leaf >= self.spares.capacity() => {
                bail!("this account has used up all {} of its spare keys", self.spares.capacity())
            }
            Some(Signer::Spare(leaf)) => {
                self.used_spares.insert(*leaf, digest);
                Signature::Spare(self.spares.sign_with_leaf(*leaf, digest.as_bytes()))
            }
            Some(Signer::Key(expected)) if *expected != self.key(seq).commitment() => bail!(
                "the account's next payment must be signed with a key this wallet file does not \
                 hold. Another wallet may have taken the account over."
            ),
            _ => {
                self.used.insert(seq, digest);
                Signature::Key(self.key(seq).sign(digest.as_bytes()))
            }
        };
        let signed = SignedOrder { signature, order };
        self.pending = Some(signed.clone());
        self.save()?;
        Ok(signed)
    }

    /// Drops the order signed under `seq`, once a checkpoint has cancelled
    /// that number. The key that signed it is retired with it.
    pub fn forget(&mut self, seq: u64) -> Result<()> {
        if self.pending.as_ref().is_some_and(|pending| pending.order.seq == seq) {
            self.retire_pending();
            self.save()?;
        }
        Ok(())
    }

    /// Lets go of the pending order, keeping its key marked as spent. An
    /// order signed before wallets kept these records has no digest on file;
    /// its key is then marked as spent on anything at all.
    fn retire_pending(&mut self) {
        const SPENT: Hash32 = Hash32([0xff; 32]);
        if let Some(pending) = self.pending.take() {
            match &pending.signature {
                Signature::Key(_) => {
                    self.used.entry(pending.order.seq).or_insert(SPENT);
                }
                Signature::Spare(signature) => {
                    self.used_spares.entry(signature.leaf).or_insert(SPENT);
                }
            }
        }
    }

    /// Whether `signed` carries this wallet's own signature: made with the
    /// one-time key this wallet derives for that payment number, or with
    /// one of its spare keys.
    pub fn signed_by_this_wallet(&self, signed: &SignedOrder, network: &Hash32) -> bool {
        let own_key = match signed.signed_by(network) {
            Some(SignedBy::Key(key)) => key == self.key(signed.order.seq).commitment(),
            Some(SignedBy::Spare { root, .. }) => root == self.spares.root(),
            None => false,
        };
        own_key && signed.order.from == self.address && signed.order.spare == self.spares.root()
    }

    /// Takes over an order that another copy of this wallet signed, so that
    /// this copy can finish the payment. Nothing is signed here, and from
    /// now on this copy will not sign anything else under that number.
    pub fn adopt(&mut self, signed: &SignedOrder, network: &Hash32) -> Result<()> {
        let order = &signed.order;
        let seq = order.seq;
        if order.from != self.address {
            bail!("that payment is from a different account");
        }
        if seq < self.next_seq {
            bail!("payment #{seq} was already made from this wallet");
        }
        if self.pending.as_ref().is_some_and(|pending| pending.order.seq > seq) {
            bail!("this wallet has already signed a later payment than #{seq}");
        }
        if !self.signed_by_this_wallet(signed, network) {
            bail!("payment #{seq} was not signed with this wallet's key");
        }
        if order.next_key != self.key(seq + 1).commitment() {
            bail!(
                "payment #{seq} hands the account to a key this wallet does not hold. \
                 Finishing it would lock this wallet out of the account."
            );
        }
        self.record(signed, network);
        self.pending = Some(signed.clone());
        self.save()
    }

    /// Notes which key signed `signed`, so that it never signs anything else.
    fn record(&mut self, signed: &SignedOrder, network: &Hash32) {
        let digest = signed.order.digest(network);
        match &signed.signature {
            Signature::Key(_) => {
                self.used.insert(signed.order.seq, digest);
            }
            Signature::Spare(signature) => {
                self.used_spares.insert(signature.leaf, digest);
            }
        }
    }

    /// Records that payment `seq` is final.
    pub fn confirm(&mut self, seq: u64) -> Result<()> {
        if self.pending.as_ref().is_some_and(|p| p.order.seq <= seq) {
            self.pending = None;
        }
        self.next_seq = self.next_seq.max(seq + 1);
        // Keys for numbers already made never sign again: the wallet refuses
        // any number below `next_seq`.
        self.used = self.used.split_off(&self.next_seq);
        self.save()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_key_ever_signs_two_different_orders() {
        let dir = std::env::temp_dir().join(format!("orien-keys-twice-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let (network, payee) = (Hash32([7; 32]), Hash32([9; 32]));
        let path = dir.join("w.wallet");
        let mut wallet = Wallet::create(&path).unwrap();

        // Signed, then let go of: the same order may be signed again, a
        // different one under that number may not.
        let first = wallet.sign_order(0, payee, 5, None, &network).unwrap();
        wallet.forget(0).unwrap();
        assert!(wallet.pending().is_none());
        assert!(wallet.sign_order(0, payee, 6, None, &network).is_err());
        assert_eq!(wallet.sign_order(0, payee, 5, None, &network).unwrap(), first);

        // Spare keys likewise, each by its own number.
        let spare = Some(Signer::Spare(0));
        wallet.sign_order(0, payee, 7, spare.as_ref(), &network).unwrap();
        wallet.forget(0).unwrap();
        assert!(wallet.sign_order(0, payee, 8, spare.as_ref(), &network).is_err());
        wallet.sign_order(0, payee, 8, Some(&Signer::Spare(1)), &network).unwrap();
        wallet.forget(0).unwrap();

        // The records survive a reload.
        let mut reloaded = Wallet::load(&path).unwrap();
        assert!(reloaded.sign_order(0, payee, 6, None, &network).is_err());
        assert!(reloaded.sign_order(0, payee, 9, Some(&Signer::Spare(0)), &network).is_err());

        // A wallet file from before these records: a pending order that is
        // let go of leaves its key spent on anything else.
        let mut old = Wallet::create(&dir.join("old.wallet")).unwrap();
        old.sign_order(0, payee, 5, None, &network).unwrap();
        let mut file: serde_json::Value = serde_json::from_str(&fs::read_to_string(dir.join("old.wallet")).unwrap()).unwrap();
        file.as_object_mut().unwrap().remove("used");
        fs::write(dir.join("old.wallet"), file.to_string()).unwrap();
        let mut old = Wallet::load(&dir.join("old.wallet")).unwrap();
        old.forget(0).unwrap();
        assert!(old.sign_order(0, payee, 6, None, &network).is_err());

        // Once a payment is final its number is never signed again, and its
        // record is no longer needed.
        let mut fresh = Wallet::create(&dir.join("fresh.wallet")).unwrap();
        fresh.sign_order(0, payee, 5, None, &network).unwrap();
        fresh.confirm(0).unwrap();
        assert!(fresh.used.is_empty());
        assert!(fresh.sign_order(0, payee, 5, None, &network).is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_wallet_adopts_only_what_its_own_key_signed() {
        let dir = std::env::temp_dir().join(format!("orien-keys-adopt-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let (network, elsewhere, payee) = (Hash32([7; 32]), Hash32([8; 32]), Hash32([9; 32]));

        // Two copies of one wallet, and a stranger's.
        let first = Wallet::create(&dir.join("first.wallet")).unwrap();
        fs::copy(dir.join("first.wallet"), dir.join("copy.wallet")).unwrap();
        let mut copy = Wallet::load(&dir.join("copy.wallet")).unwrap();
        let stranger = Wallet::create(&dir.join("stranger.wallet")).unwrap();

        let order = |from: &Wallet, next_key: &Wallet| Order {
            next_key: next_key.key(1).commitment(),
            ..from.order(0, payee, 5)
        };
        let sign = |with: &Wallet, order: Order| SignedOrder {
            signature: Signature::Key(with.key(0).sign(order.digest(&network).as_bytes())),
            order,
        };

        let good = sign(&first, order(&first, &first));
        let refused = [
            ("a stranger's payment", sign(&stranger, order(&stranger, &stranger)), network),
            ("signed with someone else's key", sign(&stranger, order(&first, &first)), network),
            ("hands the account to a foreign key", sign(&first, order(&first, &stranger)), network),
            (
                "names a stranger's spare keys",
                sign(&first, Order { spare: stranger.spare(), ..order(&first, &first) }),
                network,
            ),
            ("signed for another network", good.clone(), elsewhere),
        ];
        for (what, signed, network) in &refused {
            assert!(copy.adopt(signed, network).is_err(), "adopted an order {what}");
        }
        assert!(copy.pending().is_none());

        // The order the first copy signed is accepted, survives a reload,
        // and from then on blocks any other payment under that number.
        copy.adopt(&good, &network).unwrap();
        let mut reloaded = Wallet::load(&dir.join("copy.wallet")).unwrap();
        assert_eq!(reloaded.pending(), Some(&good));
        assert!(reloaded.sign_order(0, payee, 6, None, &network).is_err());
        assert_eq!(reloaded.sign_order(0, payee, 5, None, &network).unwrap(), good);
        let _ = fs::remove_dir_all(dir);
    }
}
