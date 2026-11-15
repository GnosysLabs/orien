//! The Bitcoin lock behind a swap: coins that the payee can take by
//! revealing a secret, or that go back to the payer after a block height.
//!
//! The lock is a pay-to-witness-script-hash (P2WSH) output with this script:
//!
//! ```text
//! OP_IF
//!     OP_SIZE 32 OP_EQUALVERIFY OP_SHA256 <hash> OP_EQUALVERIFY <payee key> OP_CHECKSIG
//! OP_ELSE
//!     <height> OP_CHECKLOCKTIMEVERIFY OP_DROP <payer key> OP_CHECKSIG
//! OP_ENDIF
//! ```
//!
//! It checks SHA-256, as Orien's locked payments do, so one secret opens
//! both. The secret must be exactly 32 bytes, as on Orien. The keys are
//! one-time keys made for this swap alone.

use anyhow::{Result, anyhow, bail};
use bitcoin::absolute::LockTime;
use bitcoin::blockdata::opcodes::all::{
    OP_CHECKSIG, OP_CLTV, OP_DROP, OP_ELSE, OP_ENDIF, OP_EQUALVERIFY, OP_IF, OP_SHA256, OP_SIZE,
};
use bitcoin::hashes::Hash;
use bitcoin::script::Builder;
use bitcoin::secp256k1::{Message, Secp256k1, SecretKey};
use bitcoin::sighash::{EcdsaSighashType, SighashCache};
use bitcoin::transaction::Version;
use bitcoin::{Address, Amount, Network, OutPoint, PublicKey, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Witness};

/// The terms of one swap's Bitcoin lock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lock {
    /// SHA-256 of the secret.
    pub hash: [u8; 32],
    /// Who can take the coins with the secret.
    pub payee: PublicKey,
    /// Who gets them back after `height`.
    pub payer: PublicKey,
    /// The block height from which the payer can take them back.
    pub height: u32,
}

impl Lock {
    pub fn script(&self) -> ScriptBuf {
        Builder::new()
            .push_opcode(OP_IF)
            .push_opcode(OP_SIZE)
            .push_int(32)
            .push_opcode(OP_EQUALVERIFY)
            .push_opcode(OP_SHA256)
            .push_slice(self.hash)
            .push_opcode(OP_EQUALVERIFY)
            .push_key(&self.payee)
            .push_opcode(OP_CHECKSIG)
            .push_opcode(OP_ELSE)
            .push_int(self.height as i64)
            .push_opcode(OP_CLTV)
            .push_opcode(OP_DROP)
            .push_key(&self.payer)
            .push_opcode(OP_CHECKSIG)
            .push_opcode(OP_ENDIF)
            .into_script()
    }

    /// The address to pay the swap's coins into.
    pub fn address(&self, network: Network) -> Address {
        Address::p2wsh(&self.script(), network)
    }

    /// The payee takes the coins, revealing the secret, and pays them to
    /// `to` less `fee`.
    pub fn claim(&self, coin: &Coin, secret: &[u8; 32], key: &SecretKey, to: &Address, fee: Amount) -> Result<Transaction> {
        if bitcoin::hashes::sha256::Hash::hash(secret).to_byte_array() != self.hash {
            bail!("that secret does not open this lock");
        }
        let mut tx = self.spend(coin, to, fee, LockTime::ZERO, Sequence::MAX)?;
        let signature = self.sign(&tx, coin, key)?;
        tx.input[0].witness = Witness::from_slice(&[signature, secret.to_vec(), vec![1], self.script().to_bytes()]);
        Ok(tx)
    }

    /// The payer takes the coins back, from `height` on, and pays them to
    /// `to` less `fee`.
    pub fn refund(&self, coin: &Coin, key: &SecretKey, to: &Address, fee: Amount) -> Result<Transaction> {
        let lock_time = LockTime::from_height(self.height).map_err(|_| anyhow!("not a block height"))?;
        let mut tx = self.spend(coin, to, fee, lock_time, Sequence::ENABLE_LOCKTIME_NO_RBF)?;
        let signature = self.sign(&tx, coin, key)?;
        tx.input[0].witness = Witness::from_slice(&[signature, Vec::new(), self.script().to_bytes()]);
        Ok(tx)
    }

    fn spend(&self, coin: &Coin, to: &Address, fee: Amount, lock_time: LockTime, sequence: Sequence) -> Result<Transaction> {
        let value = coin.value.checked_sub(fee).filter(|v| *v > Amount::from_sat(330)).ok_or_else(|| {
            anyhow!("the fee of {} would leave too little of {}", fee, coin.value)
        })?;
        Ok(Transaction {
            version: Version::TWO,
            lock_time,
            input: vec![TxIn { previous_output: coin.outpoint, script_sig: ScriptBuf::new(), sequence, witness: Witness::new() }],
            output: vec![TxOut { value, script_pubkey: to.script_pubkey() }],
        })
    }

    fn sign(&self, tx: &Transaction, coin: &Coin, key: &SecretKey) -> Result<Vec<u8>> {
        let sighash = SighashCache::new(tx).p2wsh_signature_hash(0, &self.script(), coin.value, EcdsaSighashType::All)?;
        let signature = Secp256k1::new().sign_ecdsa(&Message::from_digest(sighash.to_byte_array()), key);
        Ok(bitcoin::ecdsa::Signature { signature, sighash_type: EcdsaSighashType::All }.to_vec())
    }
}

/// The coin locked in a swap's address: where it is and how much it holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Coin {
    pub outpoint: OutPoint,
    pub value: Amount,
}

/// A fresh one-time key for one side of a swap.
pub fn new_key() -> (SecretKey, PublicKey) {
    let secp = Secp256k1::new();
    let (secret, public) = secp.generate_keypair(&mut bitcoin::secp256k1::rand::thread_rng());
    (secret, PublicKey::new(public))
}

/// Reads the secret out of a transaction that claimed a swap's lock: the
/// second item of its witness, once it hashes to the lock's hash.
pub fn secret_in(tx: &Transaction, hash: &[u8; 32]) -> Option<[u8; 32]> {
    tx.input.iter().find_map(|input| {
        let item: [u8; 32] = input.witness.nth(1)?.try_into().ok()?;
        (bitcoin::hashes::sha256::Hash::hash(&item).to_byte_array() == *hash).then_some(item)
    })
}

/// About how large, in virtual bytes, a claim or refund transaction is: for
/// working out its fee.
pub const SPEND_VBYTES: u64 = 180;

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::Txid;
    use bitcoin::hashes::sha256;

    fn lock(height: u32) -> (Lock, SecretKey, SecretKey, [u8; 32]) {
        let secret = [5u8; 32];
        let (payee_key, payee) = new_key();
        let (payer_key, payer) = new_key();
        (Lock { hash: sha256::Hash::hash(&secret).to_byte_array(), payee, payer, height }, payee_key, payer_key, secret)
    }

    fn coin() -> Coin {
        Coin { outpoint: OutPoint { txid: Txid::all_zeros(), vout: 0 }, value: Amount::from_sat(1_000_000) }
    }

    /// Runs Bitcoin Core's own script check on input 0 of `tx`.
    fn valid(lock: &Lock, tx: &Transaction) -> bool {
        let spent = TxOut { value: coin().value, script_pubkey: lock.address(Network::Bitcoin).script_pubkey() };
        tx.verify(|_| Some(spent.clone())).is_ok()
    }

    fn to() -> Address {
        let (_, key) = new_key();
        Address::p2wpkh(&bitcoin::CompressedPublicKey(key.inner), Network::Bitcoin)
    }

    #[test]
    fn the_payee_claims_with_the_secret_and_reveals_it() {
        let (lock, payee_key, payer_key, secret) = lock(900_000);
        let tx = lock.claim(&coin(), &secret, &payee_key, &to(), Amount::from_sat(2_000)).unwrap();
        assert!(valid(&lock, &tx), "Bitcoin's rules accept the claim");
        assert_eq!(tx.output[0].value, Amount::from_sat(998_000));
        assert_eq!(secret_in(&tx, &lock.hash), Some(secret), "the other side reads the secret from it");
        // The wrong key, or the wrong secret, cannot claim.
        let stolen = lock.claim(&coin(), &secret, &payer_key, &to(), Amount::from_sat(2_000)).unwrap();
        assert!(!valid(&lock, &stolen));
        assert!(lock.claim(&coin(), &[6; 32], &payee_key, &to(), Amount::from_sat(2_000)).is_err());
        println!("claim: {} vbytes", tx.vsize());
        assert!(tx.vsize() as u64 <= SPEND_VBYTES);
    }

    #[test]
    fn the_payer_takes_the_coins_back_only_after_the_height() {
        let (lock, payee_key, payer_key, _) = lock(900_000);
        let tx = lock.refund(&coin(), &payer_key, &to(), Amount::from_sat(2_000)).unwrap();
        assert!(valid(&lock, &tx), "Bitcoin's rules accept the refund");
        assert_eq!(tx.lock_time, LockTime::from_height(900_000).unwrap());
        println!("refund: {} vbytes", tx.vsize());
        assert!(tx.vsize() as u64 <= SPEND_VBYTES);
        // A refund that claims an earlier height is refused by the script.
        let mut early = tx.clone();
        early.lock_time = LockTime::from_height(899_999).unwrap();
        assert!(!valid(&lock, &early));
        // Only the payer's key refunds.
        assert!(!valid(&lock, &lock.refund(&coin(), &payee_key, &to(), Amount::from_sat(2_000)).unwrap()));
    }

    #[test]
    fn the_fee_must_leave_something() {
        let (lock, payee_key, _, secret) = lock(900_000);
        assert!(lock.claim(&coin(), &secret, &payee_key, &to(), Amount::from_sat(999_800)).is_err());
    }
}
