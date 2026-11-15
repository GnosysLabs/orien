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

    /// The payee takes every coin in `coins`, revealing the secret, and pays
    /// them to `to`, less a fee of `fee_rate` satoshis per virtual byte. It
    /// signals replace-by-fee, so that it can be sent again with a higher fee.
    /// A fee above `cap` percent of the coins is refused.
    pub fn claim(&self, coins: &[Coin], secret: &[u8; 32], key: &SecretKey, to: &Address, fee_rate: u64, cap: u64) -> Result<Transaction> {
        if bitcoin::hashes::sha256::Hash::hash(secret).to_byte_array() != self.hash {
            bail!("that secret does not open this lock");
        }
        let script = self.script().to_bytes();
        let witness = |signature: Vec<u8>| Witness::from_slice(&[signature, secret.to_vec(), vec![1], script.clone()]);
        self.spend(coins, to, (fee_rate, cap), LockTime::ZERO, key, witness)
    }

    /// The payer takes every coin in `coins` back, from `height` on, and pays
    /// them to `to`, less a fee of `fee_rate` satoshis per virtual byte, at
    /// most `cap` percent of the coins.
    pub fn refund(&self, coins: &[Coin], key: &SecretKey, to: &Address, fee_rate: u64, cap: u64) -> Result<Transaction> {
        let lock_time = LockTime::from_height(self.height).map_err(|_| anyhow!("not a block height"))?;
        let script = self.script().to_bytes();
        let witness = |signature: Vec<u8>| Witness::from_slice(&[signature, Vec::new(), script.clone()]);
        self.spend(coins, to, (fee_rate, cap), lock_time, key, witness)
    }

    fn spend(
        &self,
        coins: &[Coin],
        to: &Address,
        (fee_rate, cap): (u64, u64),
        lock_time: LockTime,
        key: &SecretKey,
        witness: impl Fn(Vec<u8>) -> Witness,
    ) -> Result<Transaction> {
        if coins.is_empty() {
            bail!("there is nothing to spend");
        }
        let total: Amount = coins.iter().map(|coin| coin.value).sum();
        // Replaceable, so a stuck transaction can be sent again with a higher
        // fee; any sequence below the maximum also lets the lock time count.
        let sequence = Sequence::ENABLE_RBF_NO_LOCKTIME;
        let mut tx = Transaction {
            version: Version::TWO,
            lock_time,
            input: coins
                .iter()
                .map(|coin| TxIn { previous_output: coin.outpoint, script_sig: ScriptBuf::new(), sequence, witness: Witness::new() })
                .collect(),
            output: vec![TxOut { value: total, script_pubkey: to.script_pubkey() }],
        };
        // The size with signatures of the largest length, to price the fee.
        for input in &mut tx.input {
            input.witness = witness(vec![0; 73]);
        }
        let fee = Amount::from_sat(fee_rate.saturating_mul(tx.vsize() as u64));
        // A fee above the cap is refused: either the network is unusually
        // busy, or the fee rate is wrong. Better to wait.
        if fee.to_sat() > total.to_sat() / 100 * cap.min(90) {
            bail!("Bitcoin's fees are too high right now for {total}: {fee}. The swap waits for them to fall.");
        }
        let value = total - fee;
        if value < to.script_pubkey().minimal_non_dust() {
            bail!("{total} is too little to send after Bitcoin's fee");
        }
        tx.output[0].value = value;
        for (index, coin) in coins.iter().enumerate() {
            let sighash = SighashCache::new(&tx).p2wsh_signature_hash(index, &self.script(), coin.value, EcdsaSighashType::All)?;
            let signature = Secp256k1::new().sign_ecdsa(&Message::from_digest(sighash.to_byte_array()), key);
            tx.input[index].witness = witness(bitcoin::ecdsa::Signature { signature, sighash_type: EcdsaSighashType::All }.to_vec());
        }
        Ok(tx)
    }
}

impl Lock {
    /// Whether `tx` spends at least one of `coins`, and every input of it
    /// that spends one carries this lock's script and a valid signature by
    /// `key`. What an explorer reports as someone's spend is believed only
    /// when this holds.
    pub fn signed_by(&self, tx: &Transaction, coins: &[Coin], key: &PublicKey) -> bool {
        let script = self.script();
        let secp = Secp256k1::verification_only();
        let mut any = false;
        for (index, input) in tx.input.iter().enumerate() {
            let Some(coin) = coins.iter().find(|coin| coin.outpoint == input.previous_output) else { continue };
            any = true;
            let (Some(signature), Some(last)) = (input.witness.nth(0), input.witness.last()) else { return false };
            if last != script.as_bytes() {
                return false;
            }
            let Ok(signature) = bitcoin::ecdsa::Signature::from_slice(signature) else { return false };
            let Ok(sighash) = SighashCache::new(tx).p2wsh_signature_hash(index, &script, coin.value, signature.sighash_type) else {
                return false;
            };
            if secp.verify_ecdsa(&Message::from_digest(sighash.to_byte_array()), &signature.signature, &key.inner).is_err() {
                return false;
            }
        }
        any
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

    fn coin(n: u8, sats: u64) -> Coin {
        Coin { outpoint: OutPoint { txid: Txid::from_byte_array([n; 32]), vout: 0 }, value: Amount::from_sat(sats) }
    }

    /// Runs Bitcoin Core's own script check on every input of `tx`.
    fn valid(lock: &Lock, tx: &Transaction, coins: &[Coin]) -> bool {
        let script = lock.address(Network::Bitcoin).script_pubkey();
        tx.verify(|outpoint| {
            coins.iter().find(|c| c.outpoint == *outpoint).map(|c| TxOut { value: c.value, script_pubkey: script.clone() })
        })
        .is_ok()
    }

    fn to() -> Address {
        let (_, key) = new_key();
        Address::p2wpkh(&bitcoin::CompressedPublicKey(key.inner), Network::Bitcoin)
    }

    #[test]
    fn the_payee_claims_with_the_secret_and_reveals_it() {
        let (lock, payee_key, payer_key, secret) = lock(900_000);
        let coins = [coin(1, 1_000_000)];
        let tx = lock.claim(&coins, &secret, &payee_key, &to(), 10, 10).unwrap();
        assert!(valid(&lock, &tx, &coins), "Bitcoin's rules accept the claim");
        assert_eq!(tx.output[0].value, Amount::from_sat(1_000_000 - 10 * tx.vsize() as u64));
        assert!(tx.input[0].sequence.is_rbf(), "it can be replaced with a higher fee");
        assert_eq!(secret_in(&tx, &lock.hash), Some(secret), "the other side reads the secret from it");
        assert!(lock.signed_by(&tx, &coins, &lock.payee), "signed by the payee");
        assert!(!lock.signed_by(&tx, &coins, &lock.payer));
        let mut forged = tx.clone();
        forged.output[0].value = Amount::from_sat(1);
        assert!(!lock.signed_by(&forged, &coins, &lock.payee), "a changed transaction is not signed");
        // The wrong key, or the wrong secret, cannot claim.
        assert!(!valid(&lock, &lock.claim(&coins, &secret, &payer_key, &to(), 10, 10).unwrap(), &coins));
        assert!(lock.claim(&coins, &[6; 32], &payee_key, &to(), 10, 10).is_err());
        println!("claim: {} vbytes", tx.vsize());
    }

    #[test]
    fn several_payments_are_claimed_or_refunded_together() {
        let (lock, payee_key, payer_key, secret) = lock(900_000);
        let coins = [coin(1, 400_000), coin(2, 700_000)];
        let claim = lock.claim(&coins, &secret, &payee_key, &to(), 3, 10).unwrap();
        assert!(valid(&lock, &claim, &coins));
        assert_eq!(claim.input.len(), 2);
        let refund = lock.refund(&coins, &payer_key, &to(), 3, 10).unwrap();
        assert!(valid(&lock, &refund, &coins));
    }

    #[test]
    fn the_payer_takes_the_coins_back_only_after_the_height() {
        let (lock, payee_key, payer_key, _) = lock(900_000);
        let coins = [coin(1, 1_000_000)];
        let tx = lock.refund(&coins, &payer_key, &to(), 10, 10).unwrap();
        assert!(valid(&lock, &tx, &coins), "Bitcoin's rules accept the refund");
        assert_eq!(tx.lock_time, LockTime::from_height(900_000).unwrap());
        println!("refund: {} vbytes", tx.vsize());
        // A refund that claims an earlier height is refused by the script.
        let mut early = tx.clone();
        early.lock_time = LockTime::from_height(899_999).unwrap();
        assert!(!valid(&lock, &early, &coins));
        // Only the payer's key refunds.
        assert!(!valid(&lock, &lock.refund(&coins, &payee_key, &to(), 10, 10).unwrap(), &coins));
    }

    #[test]
    fn fees_are_kept_sane() {
        let (lock, payee_key, _, secret) = lock(900_000);
        // A fee over a tenth of the coins is refused.
        assert!(lock.claim(&[coin(1, 100_000)], &secret, &payee_key, &to(), 100, 10).is_err());
        assert!(lock.claim(&[coin(1, 100_000)], &secret, &payee_key, &to(), 50, 10).is_ok());
        assert!(lock.claim(&[], &secret, &payee_key, &to(), 1, 10).is_err());
        // Near a deadline, a higher cap lets a fee through.
        assert!(lock.claim(&[coin(1, 100_000)], &secret, &payee_key, &to(), 100, 50).is_ok());
    }
}
