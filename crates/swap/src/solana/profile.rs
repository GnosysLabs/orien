//! Off-chain proof for linking the built-in devnet wallet to an Orien profile.
use super::{Keypair, Pubkey, text};
use ed25519_dalek::{Signature, VerifyingKey};

fn message(origin: &str, account: i64, orn: &str, sol: &Pubkey, nonce: &[u8; 32]) -> Vec<u8> {
    // The 0xff prefix distinguishes off-chain signatures from legacy transactions.
    let body = format!(
        "Orien Network wallet link v1\nSite: {}\nProfile: {account}\nORN: {orn}\nSolana Devnet: {}\nChallenge: {}",
        origin.trim_end_matches('/'),
        text(sol),
        hex::encode(nonce)
    );
    [b"\xff".as_slice(), body.as_bytes()].concat()
}

pub fn sign(key: &Keypair, origin: &str, account: i64, orn: &str, nonce: &[u8; 32]) -> [u8; 64] {
    key.sign(&message(origin, account, orn, &key.pubkey(), nonce))
}

pub fn valid(
    sol: &Pubkey,
    origin: &str,
    account: i64,
    orn: &str,
    nonce: &[u8; 32],
    signature: &[u8; 64],
) -> bool {
    VerifyingKey::from_bytes(sol).is_ok_and(|key| {
        key.verify_strict(
            &message(origin, account, orn, sol, nonce),
            &Signature::from_bytes(signature),
        )
        .is_ok()
    })
}
