//! A short-lived mailbox for moving a wallet between someone's own devices
//! with a short code instead of 24 words.
//!
//! The devices seal everything end to end (SPAKE2, then XChaCha20-Poly1305);
//! this only passes opaque bytes between them. A mailbox has four slots,
//! each written once: a, the sending device's SPAKE2 message; b, the
//! receiving device's; c, the receiver's key confirmation; d, the sealed
//! wallet. It lives ten minutes, in memory only: a restart just means trying
//! again. Nothing written is ever logged.

use crate::api::App;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a mailbox lives, from when it is opened.
pub const LIFETIME: Duration = Duration::from_secs(600);
/// The most one slot may hold, decoded.
pub const MAX_DATA: usize = 16 * 1024;
/// Mailboxes open at once, at most: 64 KB each at worst, so 64 MB in all.
const MAX_OPEN: usize = 1000;
/// Per internet address, an hour: mailboxes opened, and slots written.
const OPENS_PER_HOUR: usize = 20;
const WRITES_PER_HOUR: usize = 60;
const SLOTS: [&str; 4] = ["a", "b", "c", "d"];

struct Mailbox {
    opened: Instant,
    slots: [Option<Vec<u8>>; 4],
}

/// The open mailboxes, by code, and recent opens and writes by address.
#[derive(Default)]
pub struct Mailboxes {
    open: Mutex<HashMap<u16, Mailbox>>,
    recent: Mutex<Vec<(Instant, String, bool)>>,
}

impl Mailboxes {
    /// Forgets lapsed mailboxes; called on every use, so none outlive
    /// their ten minutes for long.
    fn sweep(open: &mut HashMap<u16, Mailbox>) {
        open.retain(|_, mailbox| mailbox.opened.elapsed() < LIFETIME);
    }

    /// Whether `who` may open (`opening`) or write one more time this hour,
    /// counting it if so.
    fn allowed(&self, who: &str, opening: bool) -> bool {
        let mut recent = self.recent.lock().unwrap();
        recent.retain(|(at, _, _)| at.elapsed() < Duration::from_secs(3600));
        let limit = if opening { OPENS_PER_HOUR } else { WRITES_PER_HOUR };
        if recent.iter().filter(|(_, w, o)| w == who && *o == opening).count() >= limit {
            return false;
        }
        recent.push((Instant::now(), who.to_string(), opening));
        true
    }
}

fn answer(status: StatusCode, body: serde_json::Value) -> Response {
    (status, Json(body)).into_response()
}

fn slot_index(slot: &str) -> Option<usize> {
    SLOTS.iter().position(|s| *s == slot)
}

/// Opens a mailbox under a free code from 1 to 9999.
pub async fn open(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let pairs = app.pairs();
    if !pairs.allowed(&crate::push::sender(&headers), true) {
        return answer(StatusCode::TOO_MANY_REQUESTS, json!({ "error": "too many codes for now; try again later" }));
    }
    let mut open = pairs.open.lock().unwrap();
    Mailboxes::sweep(&mut open);
    if open.len() >= MAX_OPEN {
        return answer(StatusCode::SERVICE_UNAVAILABLE, json!({ "error": "too many devices are pairing right now; try again in a minute" }));
    }
    for _ in 0..100 {
        let mut bytes = [0u8; 2];
        if getrandom::fill(&mut bytes).is_err() {
            break;
        }
        let code = (u16::from_le_bytes(bytes) % 9999) + 1;
        if let std::collections::hash_map::Entry::Vacant(free) = open.entry(code) {
            free.insert(Mailbox { opened: Instant::now(), slots: Default::default() });
            return answer(StatusCode::OK, json!({ "code": code }));
        }
    }
    answer(StatusCode::SERVICE_UNAVAILABLE, json!({ "error": "no free code right now; try again in a minute" }))
}

#[derive(Deserialize)]
pub struct Data {
    data: String,
}

/// Writes one slot, once.
pub async fn write(State(app): State<Arc<App>>, headers: HeaderMap, Path((code, slot)): Path<(u16, String)>, Json(body): Json<Data>) -> Response {
    let Some(index) = slot_index(&slot) else { return answer(StatusCode::NOT_FOUND, json!({ "error": "no such slot" })) };
    let Ok(data) = base64::engine::general_purpose::STANDARD.decode(body.data.trim()) else {
        return answer(StatusCode::BAD_REQUEST, json!({ "error": "the data is not base64" }));
    };
    if data.len() > MAX_DATA {
        return answer(StatusCode::PAYLOAD_TOO_LARGE, json!({ "error": "at most 16 KB" }));
    }
    let pairs = app.pairs();
    if !pairs.allowed(&crate::push::sender(&headers), false) {
        return answer(StatusCode::TOO_MANY_REQUESTS, json!({ "error": "too many tries for now; try again later" }));
    }
    let mut open = pairs.open.lock().unwrap();
    Mailboxes::sweep(&mut open);
    let Some(mailbox) = open.get_mut(&code) else { return answer(StatusCode::NOT_FOUND, json!({ "error": "that code has run out" })) };
    if mailbox.slots[index].is_some() {
        return answer(StatusCode::CONFLICT, json!({ "error": "already written" }));
    }
    mailbox.slots[index] = Some(data);
    answer(StatusCode::OK, json!({}))
}

/// Reads one slot: 202 while it is still empty.
pub async fn read(State(app): State<Arc<App>>, Path((code, slot)): Path<(u16, String)>) -> Response {
    let Some(index) = slot_index(&slot) else { return answer(StatusCode::NOT_FOUND, json!({ "error": "no such slot" })) };
    let pairs = app.pairs();
    let mut open = pairs.open.lock().unwrap();
    Mailboxes::sweep(&mut open);
    match open.get(&code) {
        None => answer(StatusCode::NOT_FOUND, json!({ "error": "that code has run out" })),
        Some(mailbox) => match &mailbox.slots[index] {
            None => answer(StatusCode::ACCEPTED, json!({})),
            Some(data) => answer(StatusCode::OK, json!({ "data": base64::engine::general_purpose::STANDARD.encode(data) })),
        },
    }
}

/// Closes a mailbox, when the devices are done or a match failed.
pub async fn close(State(app): State<Arc<App>>, Path(code): Path<u16>) -> Response {
    let pairs = app.pairs();
    let mut open = pairs.open.lock().unwrap();
    Mailboxes::sweep(&mut open);
    match open.remove(&code) {
        Some(_) => answer(StatusCode::OK, json!({})),
        None => answer(StatusCode::NOT_FOUND, json!({ "error": "that code has run out" })),
    }
}

impl Mailboxes {
    /// For tests: makes every open mailbox look `age` older.
    #[doc(hidden)]
    pub fn age_for_tests(&self, age: Duration) {
        for mailbox in self.open.lock().unwrap().values_mut() {
            mailbox.opened -= age;
        }
    }
}
