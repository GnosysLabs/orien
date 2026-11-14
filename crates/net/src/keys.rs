//! Files a person holds: the genesis file, validator keys and wallets.

use crate::store::write_replacing;
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use token_crypto::{Hash32, Kdf, ots, prf};
use token_ledger::{AccountId, Genesis, Order, SignedOrder};

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
/// The wallet's job beyond holding the seed is to make sure each one-time
/// key signs at most one order. It writes every order it signs to disk
/// before the order leaves the process, and afterwards will only ever
/// re-send that same order under that payment number.
pub struct Wallet {
    path: PathBuf,
    seed: [u8; 32],
    address: AccountId,
    next_seq: u64,
    pending: Option<SignedOrder>,
}

#[derive(Serialize, Deserialize)]
struct WalletFile {
    seed: String,
    /// Payments known to be final. The next payment carries this number.
    next_seq: u64,
    /// A signed order not yet known to be final, as hex.
    #[serde(default)]
    pending: Option<String>,
}

fn wallet_key(seed: &[u8; 32], index: u64) -> ots::SecretKey {
    ots::SecretKey::from_seed(prf(seed, Kdf::WalletKey, index))
}

impl Wallet {
    pub fn create(path: &Path) -> Result<Wallet> {
        if path.exists() {
            bail!("{} already exists; refusing to overwrite a wallet", path.display());
        }
        let seed = random_seed();
        let wallet = Wallet {
            path: path.to_owned(),
            seed,
            address: wallet_key(&seed, 0).commitment(),
            next_seq: 0,
            pending: None,
        };
        wallet.save()?;
        Ok(wallet)
    }

    pub fn load(path: &Path) -> Result<Wallet> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("cannot read wallet {}", path.display()))?;
        let file: WalletFile = serde_json::from_str(&text)
            .with_context(|| format!("{} is not a wallet file", path.display()))?;
        let seed = parse_seed(&file.seed)?;
        let pending = match file.pending {
            Some(text) => {
                let bytes = hex::decode(text).context("wallet holds a corrupt pending payment")?;
                Some(borsh::from_slice(&bytes).context("wallet holds a corrupt pending payment")?)
            }
            None => None,
        };
        Ok(Wallet {
            path: path.to_owned(),
            seed,
            address: wallet_key(&seed, 0).commitment(),
            next_seq: file.next_seq,
            pending,
        })
    }

    fn save(&self) -> Result<()> {
        let pending = match &self.pending {
            Some(order) => Some(hex::encode(borsh::to_vec(order)?)),
            None => None,
        };
        let file = WalletFile { seed: hex::encode(self.seed), next_seq: self.next_seq, pending };
        write_replacing(&self.path, (serde_json::to_string_pretty(&file)? + "\n").as_bytes(), true)
            .with_context(|| format!("cannot write wallet {}", self.path.display()))
    }

    /// One-time key number `index`. Key `n` signs payment `n` and nothing else.
    pub fn key(&self, index: u64) -> ots::SecretKey {
        wallet_key(&self.seed, index)
    }

    pub fn address(&self) -> AccountId {
        self.address
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
    pub fn sign_order(
        &mut self,
        seq: u64,
        to: AccountId,
        amount: u64,
        network: &Hash32,
    ) -> Result<SignedOrder> {
        if let Some(pending) = &self.pending {
            if pending.order.seq == seq {
                if pending.order.to == to && pending.order.amount == amount {
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
        let order = Order {
            from: self.address,
            seq,
            to,
            amount,
            next_key: self.key(seq + 1).commitment(),
        };
        let digest = order.digest(network);
        let signed = SignedOrder { signature: self.key(seq).sign(digest.as_bytes()), order };
        self.pending = Some(signed.clone());
        self.save()?;
        Ok(signed)
    }

    /// Whether `signed` carries this wallet's own signature: made with the
    /// one-time key this wallet derives for that payment number.
    pub fn signed_by_this_wallet(&self, signed: &SignedOrder, network: &Hash32) -> bool {
        let digest = signed.order.digest(network);
        signed.order.from == self.address
            && ots::recover(&signed.signature, digest.as_bytes()) == self.key(signed.order.seq).commitment()
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
        self.pending = Some(signed.clone());
        self.save()
    }

    /// Records that payment `seq` is final.
    pub fn confirm(&mut self, seq: u64) -> Result<()> {
        if self.pending.as_ref().is_some_and(|p| p.order.seq <= seq) {
            self.pending = None;
        }
        self.next_seq = self.next_seq.max(seq + 1);
        self.save()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wallet_adopts_only_what_its_own_key_signed() {
        let dir = std::env::temp_dir().join(format!("token-keys-adopt-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let (network, elsewhere, payee) = (Hash32([7; 32]), Hash32([8; 32]), Hash32([9; 32]));

        // Two copies of one wallet, and a stranger's.
        let first = Wallet::create(&dir.join("first.wallet")).unwrap();
        fs::copy(dir.join("first.wallet"), dir.join("copy.wallet")).unwrap();
        let mut copy = Wallet::load(&dir.join("copy.wallet")).unwrap();
        let stranger = Wallet::create(&dir.join("stranger.wallet")).unwrap();

        let order = |from: &Wallet, next_key: &Wallet| Order {
            from: from.address(),
            seq: 0,
            to: payee,
            amount: 5,
            next_key: next_key.key(1).commitment(),
        };
        let sign = |with: &Wallet, order: Order| SignedOrder {
            signature: with.key(0).sign(order.digest(&network).as_bytes()),
            order,
        };

        let good = sign(&first, order(&first, &first));
        let refused = [
            ("a stranger's payment", sign(&stranger, order(&stranger, &stranger)), network),
            ("signed with someone else's key", sign(&stranger, order(&first, &first)), network),
            ("hands the account to a foreign key", sign(&first, order(&first, &stranger)), network),
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
        assert!(reloaded.sign_order(0, payee, 6, &network).is_err());
        assert_eq!(reloaded.sign_order(0, payee, 5, &network).unwrap(), good);
        let _ = fs::remove_dir_all(dir);
    }
}
