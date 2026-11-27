//! A devnet address belongs to a profile only while its parent ORN proof exists.
use crate::api::{App, Failure, refused};
use crate::store::{Store, now, orien_address, random};
use anyhow::{Result, ensure};
use axum::{Json, extract::State, http::HeaderMap};
use orien_swap::solana;
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Deserialize)]
pub(crate) struct Link {
    account: i64,
    orn: String,
    sol: String,
}

impl Link {
    fn check(&self, store: &Store, account: i64) -> Result<solana::Pubkey> {
        ensure!(account == self.account, "the signed-in profile changed");
        ensure!(
            self.orn == orien_address(&self.orn)?
                && store.is_verified_wallet(account, &self.orn)?,
            "verify this ORN wallet first"
        );
        ensure!(self.sol.len() <= 44, "invalid Solana address");
        let key = solana::address(&self.sol)?;
        ensure!(
            solana::is_wallet_key(&key) && solana::text(&key) == self.sol,
            "invalid Solana wallet"
        );
        Ok(key)
    }
}

impl Store {
    pub(crate) fn create_sol_links(&self) -> Result<()> {
        self.db.execute_batch("CREATE TABLE IF NOT EXISTS sol_devnet_links (
            account INTEGER NOT NULL, orn TEXT NOT NULL, sol TEXT NOT NULL UNIQUE,
            PRIMARY KEY(account, orn), FOREIGN KEY(account, orn) REFERENCES verified_wallets(account,address) ON DELETE CASCADE
        );
        CREATE TABLE IF NOT EXISTS sol_devnet_challenges (
            account INTEGER NOT NULL, orn TEXT NOT NULL, sol TEXT NOT NULL, nonce TEXT NOT NULL, expires INTEGER NOT NULL,
            PRIMARY KEY(account, orn), FOREIGN KEY(account, orn) REFERENCES verified_wallets(account,address) ON DELETE CASCADE
        );")?;
        Ok(())
    }

    fn start_sol_link(&self, account: i64, link: &Link) -> Result<String> {
        link.check(self, account)?;
        let nonce = hex::encode(random::<32>()?);
        self.db.execute("INSERT INTO sol_devnet_challenges(account,orn,sol,nonce,expires) VALUES (?,?,?,?,?)
            ON CONFLICT(account,orn) DO UPDATE SET sol=excluded.sol, nonce=excluded.nonce, expires=excluded.expires",
            params![account,link.orn,link.sol,nonce,now()+300])?;
        Ok(nonce)
    }

    fn finish_sol_link(&mut self, origin: &str, account: i64, proof: &Signed) -> Result<()> {
        let link = &proof.link;
        let key = link.check(self, account)?;
        let tx = self.db.transaction()?;
        let nonce: Option<String> = tx.query_row("SELECT nonce FROM sol_devnet_challenges WHERE account=? AND orn=? AND sol=? AND expires>?",
            params![account,link.orn,link.sol,now()], |r| r.get(0)).optional()?;
        let nonce =
            nonce.ok_or_else(|| anyhow::anyhow!("the challenge expired; link the wallet again"))?;
        let mut bytes = [0; 32];
        hex::decode_to_slice(nonce, &mut bytes)?;
        let mut signature = [0; 64];
        hex::decode_to_slice(&proof.signature, &mut signature)?;
        ensure!(
            solana::profile::valid(&key, origin, account, &link.orn, &bytes, &signature),
            "the wallet signature does not match this profile"
        );
        tx.execute(
            "INSERT INTO sol_devnet_links(account,orn,sol) VALUES(?,?,?) ON CONFLICT DO NOTHING",
            params![account, link.orn, link.sol],
        )?;
        let matches: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sol_devnet_links WHERE account=? AND orn=? AND sol=?)",
            params![account, link.orn, link.sol],
            |r| r.get(0),
        )?;
        ensure!(matches, "this wallet already has a different profile link");
        tx.execute(
            "DELETE FROM sol_devnet_challenges WHERE account=? AND orn=?",
            params![account, link.orn],
        )?;
        tx.commit()?;
        Ok(())
    }
}

#[derive(Deserialize)]
pub(crate) struct Signed {
    #[serde(flatten)]
    link: Link,
    signature: String,
}

pub(crate) async fn challenge(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(link): Json<Link>,
) -> Result<Json<Value>, Failure> {
    let account = app.signed_in(&headers)?;
    let nonce = app
        .store()
        .start_sol_link(account, &link)
        .map_err(refused)?;
    Ok(Json(json!({ "nonce": nonce })))
}

pub(crate) async fn confirm(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(proof): Json<Signed>,
) -> Result<Json<Value>, Failure> {
    let account = app.signed_in(&headers)?;
    app.store()
        .finish_sol_link(app.origin(), account, &proof)
        .map_err(refused)?;
    Ok(Json(json!({ "linked": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_only_the_verified_profile_and_consumes_its_challenge() {
        let mut store = Store::in_memory().unwrap();
        let owner = store.create_account("Maya", "maya").unwrap().id;
        let other = store.create_account("Kai", "kai").unwrap().id;
        let orn = "ab".repeat(32);
        let signer = solana::Keypair::random();
        let link = Link {
            account: owner,
            orn: orn.clone(),
            sol: solana::text(&signer.pubkey()),
        };
        assert!(store.start_sol_link(owner, &link).is_err());
        store.verified_wallet_for_tests(owner, &orn).unwrap();
        assert!(store.start_sol_link(other, &link).is_err());
        let nonce = store.start_sol_link(owner, &link).unwrap();
        let sign = |nonce: &str| {
            let mut bytes = [0; 32];
            hex::decode_to_slice(nonce, &mut bytes).unwrap();
            hex::encode(solana::profile::sign(
                &signer,
                "https://orien.network",
                owner,
                &orn,
                &bytes,
            ))
        };
        let mut proof = Signed {
            link,
            signature: sign(&nonce),
        };
        let nonce = store.start_sol_link(owner, &proof.link).unwrap();
        assert!(
            store
                .finish_sol_link("https://orien.network", owner, &proof)
                .is_err()
        );
        proof.signature = sign(&nonce);
        assert!(
            store
                .finish_sol_link("another-site", owner, &proof)
                .is_err()
        );
        assert!(
            store
                .finish_sol_link("https://orien.network", other, &proof)
                .is_err()
        );
        store
            .finish_sol_link("https://orien.network", owner, &proof)
            .unwrap();
        assert!(
            store
                .finish_sol_link("https://orien.network", owner, &proof)
                .is_err()
        );
        assert_eq!(
            store.verified_wallets(owner).unwrap()[0]
                .solana_devnet
                .as_deref(),
            Some(proof.link.sol.as_str())
        );
        store.remove_proof(owner, &orn).unwrap();
        store.verified_wallet_for_tests(owner, &orn).unwrap();
        assert!(
            store.proofs(owner, "to").unwrap()[0]
                .solana_devnet
                .is_none()
        );
    }
}

pub(crate) async fn unlink(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(link): Json<Link>,
) -> Result<Json<Value>, Failure> {
    let account = app.signed_in(&headers)?;
    let mut store = app.store();
    link.check(&store, account).map_err(refused)?;
    let tx = store.db.transaction().map_err(anyhow::Error::from)?;
    tx.execute(
        "DELETE FROM sol_devnet_challenges WHERE account=? AND orn=?",
        params![account, link.orn],
    )
    .map_err(anyhow::Error::from)?;
    tx.execute(
        "DELETE FROM sol_devnet_links WHERE account=? AND orn=? AND sol=?",
        params![account, link.orn, link.sol],
    )
    .map_err(anyhow::Error::from)?;
    tx.commit().map_err(anyhow::Error::from)?;
    Ok(Json(json!({ "linked": false })))
}
