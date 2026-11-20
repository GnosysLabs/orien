//! The Safari extension: it holds no wallet. It asks this app, over a
//! connection only this Mac can make, for the wallet's address and
//! balance; any signing stays here, in the app.

use std::io::{Read, Write};
use std::sync::Mutex;

/// Where the extension finds the app: this Mac only.
pub const PORT: u16 = 47_211;

/// What the extension is told, kept current by the app as it refreshes.
static WALLET: Mutex<Option<String>> = Mutex::new(None);

/// Keeps the address and balance for the extension to show.
pub fn share(address: &str, balance: Option<&str>) {
    let text = serde_json::json!({ "address": address, "balance": balance }).to_string();
    *WALLET.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(text);
}

/// Answers the extension, for as long as the app runs.
pub fn serve() {
    std::thread::spawn(|| {
        let Ok(listener) = std::net::TcpListener::bind(("127.0.0.1", PORT)) else { return };
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
            let mut request = [0u8; 1024];
            let read = stream.read(&mut request).unwrap_or(0);
            // Only the one question; a web page can't ask it (no page may
            // send this header to another site without the app agreeing).
            let request = String::from_utf8_lossy(&request[..read]);
            let asked = request.starts_with("GET /wallet ") && request.contains("\r\nX-Orien-Extension: 1\r\n");
            let body = if asked { WALLET.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone().unwrap_or_else(|| "{}".into()) } else { "{}".into() };
            let status = if asked { "200 OK" } else { "403 Forbidden" };
            let _ = write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        }
    });
}
