//! The Safari extension: it holds no wallet. It asks this app, over a
//! connection only this Mac can make, for the wallet's address and
//! balance, and asks it to send; the app approves each payment with
//! Touch ID or the password, and does all signing itself.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Mutex;
use tauri::AppHandle;

/// Where the extension finds the app: this Mac only.
pub const PORT: u16 = 47_211;

/// The address, balance and most that can be sent, as the app last saw
/// them.
static WALLET: Mutex<Option<serde_json::Value>> = Mutex::new(None);

/// Keeps what the extension shows current, as the app refreshes.
pub fn share(address: &str, balance: Option<&str>, most: Option<&str>) {
    let seen = serde_json::json!({ "address": address, "balance": balance, "most": most });
    *WALLET.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(seen);
}

/// Answers the extension, for as long as the app runs.
pub fn serve(app: AppHandle) {
    std::thread::spawn(move || {
        let Ok(listener) = std::net::TcpListener::bind(("127.0.0.1", PORT)) else { return };
        for stream in listener.incoming().flatten() {
            let app = app.clone();
            std::thread::spawn(move || answer(&app, stream));
        }
    });
}

/// One question: its path and body, or None if it isn't the extension's.
fn read(stream: &mut TcpStream) -> Option<(String, Vec<u8>)> {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(3)));
    let mut data = Vec::new();
    let mut buffer = [0u8; 4096];
    let head_end = loop {
        let read = stream.read(&mut buffer).ok().filter(|&n| n > 0)?;
        data.extend_from_slice(&buffer[..read]);
        if let Some(at) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break at + 4;
        }
        if data.len() > 16 * 1024 {
            return None;
        }
    };
    let head = String::from_utf8_lossy(&data[..head_end]).to_string();
    let lines: Vec<&str> = head.split("\r\n").collect();
    let header = |name: &str| {
        lines.iter().find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case(name)).map(|(_, v)| v.trim().to_string()))
    };
    // Only the extension: it says so, and a web page can neither add that
    // header to another site nor hide where it comes from.
    if header("X-Orien-Extension").as_deref() != Some("1") || header("Origin").is_some() {
        return None;
    }
    let mut parts = lines.first()?.split(' ');
    let (method, path) = (parts.next()?, parts.next()?.to_string());
    let length: usize = header("Content-Length").and_then(|v| v.parse().ok()).unwrap_or(0).min(64 * 1024);
    let mut body = data[head_end..].to_vec();
    while body.len() < length {
        let read = stream.read(&mut buffer).ok().filter(|&n| n > 0)?;
        body.extend_from_slice(&buffer[..read]);
    }
    Some((format!("{method} {path}"), body))
}

fn answer(app: &AppHandle, mut stream: TcpStream) {
    let (status, body) = match read(&mut stream) {
        None => ("403 Forbidden", serde_json::json!({})),
        Some((asked, body)) => {
            let body: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
            let text = |key: &str| body.get(key).and_then(|v| v.as_str()).unwrap_or("").to_string();
            match asked.as_str() {
                "GET /wallet" => ("200 OK", wallet(app)),
                "GET /people" => ("200 OK", people(app)),
                "POST /search" => ("200 OK", crate::board::search(&text("text")).unwrap_or_default()),
                "POST /quote" => ("200 OK", serde_json::to_value(crate::quote(text("amount"))).unwrap_or_default()),
                "POST /send" => {
                    let password = Some(text("password")).filter(|p| !p.is_empty());
                    match send(app, text("to"), text("amount"), password) {
                        Ok(sent) => ("200 OK", sent),
                        Err(problem) => ("200 OK", serde_json::json!({ "problem": problem })),
                    }
                }
                _ => ("404 Not Found", serde_json::json!({})),
            }
        }
    };
    let body = body.to_string();
    let _ = write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
}

/// The wallet as the extension shows it, and how a payment is approved:
/// "touch" (Touch ID), "password", or "none" (no password).
fn wallet(app: &AppHandle) -> serde_json::Value {
    let mut seen = WALLET.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone().unwrap_or_else(|| serde_json::json!({}));
    let dir = crate::folder(app).ok();
    if let Some(dir) = &dir
        && let Some(file) = crate::wallet_file(dir)
    {
        // Which wallet: its name and icon, as the app shows them.
        if let Some(named) = crate::known_wallets(dir).into_iter().find(|w| w.path == file) {
            seen["name"] = named.name.into();
        }
        if let Some(address) = seen.get("address").and_then(|a| a.as_str()).map(str::to_string)
            && let Some(icon) = crate::contacts::icons(dir).get(&address)
        {
            seen["icon"] = icon.emoji.clone().into();
        }
        let approve = if !crate::has_password(&file) {
            "none"
        } else if crate::biometric::enabled(&file) {
            "touch"
        } else {
            "password"
        };
        seen["approve"] = approve.into();
    }
    seen
}

/// Sends, as the app's own Send does. With Touch ID on, it asks for it
/// every time, so nothing leaves the wallet unseen.
fn send(app: &AppHandle, to: String, amount: String, password: Option<String>) -> Result<serde_json::Value, String> {
    let file = crate::wallet_file(&crate::folder(app)?).ok_or("no wallet yet")?;
    if password.is_none() && crate::has_password(&file) {
        if !crate::biometric::enabled(&file) {
            return Err("password: needed".into());
        }
        crate::biometric::unlock(&file)?;
    }
    let to = to.split_whitespace().collect::<String>();
    let sent = tauri::async_runtime::block_on(crate::send_payment(app.clone(), to, amount, password))?;
    serde_json::to_value(sent).map_err(|error| error.to_string())
}

/// Who can be sent to by name, as the app's own Send suggests: contacts,
/// and this person's other wallets with their icons.
fn people(app: &AppHandle) -> serde_json::Value {
    let Ok(dir) = crate::folder(app) else { return serde_json::json!({}) };
    let current = crate::wallet_file(&dir);
    let icons = crate::contacts::icons(&dir);
    let contacts: Vec<_> =
        crate::contacts::list(&dir).into_iter().map(|c| serde_json::json!({ "name": c.name, "address": c.address })).collect();
    let wallets: Vec<_> = crate::known_wallets(&dir)
        .into_iter()
        .filter(|w| Some(&w.path) != current.as_ref())
        .filter_map(|w| {
            let address = orien_net::keys::Wallet::peek(&w.path).ok()?.0.to_string();
            let icon = icons.get(&address).map(|i| i.emoji.clone());
            Some(serde_json::json!({ "name": w.name, "address": address, "icon": icon }))
        })
        .collect();
    serde_json::json!({ "contacts": contacts, "wallets": wallets })
}
