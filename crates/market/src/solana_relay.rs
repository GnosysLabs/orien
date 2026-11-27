//! Solana for Orien Wallet. The wallet's SOL and USDC reads and sends come
//! here, and go on to a Solana service whose key stays on this server: free
//! public services refuse an app that checks balances often.
use crate::api::App;
use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use serde_json::Value;
use std::{sync::Arc, time::Duration};

/// Only what the wallet asks Solana, so the key can't be used for anything else.
const ALLOWED: &[&str] = &[
    "getAccountInfo",
    "getBalance",
    "getBlockHeight",
    "getBlockTime",
    "getFeeForMessage",
    "getLatestBlockhash",
    "getMinimumBalanceForRentExemption",
    "getMultipleAccounts",
    "getSignatureStatuses",
    "getSignaturesForAddress",
    "getSlot",
    "getTokenAccountsByOwner",
    "getTransaction",
    "isBlockhashValid",
    "sendTransaction",
    "simulateTransaction",
];

pub fn routes() -> Router<Arc<App>> {
    Router::new().route(
        "/api/market/solana",
        post(relay).layer(axum::extract::DefaultBodyLimit::max(16 * 1024)),
    )
}

fn refuse(status: StatusCode, message: &str) -> Response {
    (status, axum::Json(serde_json::json!({ "error": message }))).into_response()
}

async fn relay(State(app): State<Arc<App>>, body: Bytes) -> Response {
    let Some(upstream) = app.solana_relay().map(str::to_owned) else {
        return refuse(StatusCode::NOT_FOUND, "no Solana relay here");
    };
    // One request at a time, never a batch, and only a method the wallet uses.
    let Ok(request) = serde_json::from_slice::<Value>(&body) else {
        return refuse(StatusCode::BAD_REQUEST, "not JSON");
    };
    let method = request["method"].as_str().unwrap_or_default();
    if !request.is_object() || !ALLOWED.contains(&method) {
        return refuse(StatusCode::FORBIDDEN, "that Solana method isn't relayed");
    }
    let answer = tokio::task::spawn_blocking(move || -> Result<(u16, String), String> {
        let response = ureq::post(&upstream)
            .config()
            .timeout_global(Some(Duration::from_secs(20)))
            .http_status_as_error(false)
            .build()
            .header("Content-Type", "application/json")
            .send(&body[..])
            .map_err(|error| error.to_string())?;
        let status = response.status().as_u16();
        let text = response.into_body().read_to_string().map_err(|error| error.to_string())?;
        Ok((status, text))
    })
    .await;
    match answer {
        Ok(Ok((status, text))) => (
            StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY),
            [(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "no-store")],
            text,
        )
            .into_response(),
        // Never the upstream's address: it holds the key.
        _ => refuse(StatusCode::BAD_GATEWAY, "Solana didn't answer"),
    }
}
