//! Firefox's extension reaches Orien Wallet the way Safari's does: through
//! the browser's native messaging, which only the extension named in our
//! manifest may use. Firefox starts this program with that extension's ID;
//! started so, it opens no window and only passes the extension's
//! questions to the running app, on this computer's own connection, and
//! the answers back. It holds no keys: the app approves and signs.
//!
//! Each time the app starts it tells Firefox where it is, so the link
//! follows the app when it moves or updates.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;

/// The Firefox extension's ID, fixed in its manifest.
pub const EXTENSION: &str = "wallet@orien.network";
/// The name the extension asks for (the same as Safari's).
const HOST: &str = "network.orien.wallet";

/// Started by Firefox for its extension, not by a person.
pub fn asked_by_firefox() -> bool {
    std::env::args().skip(1).any(|arg| arg == EXTENSION)
}

/// Answers Firefox until it closes the connection: each message is four
/// bytes of length, then that much JSON, both ways.
pub fn answer_firefox() {
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    loop {
        let mut size = [0u8; 4];
        if input.read_exact(&mut size).is_err() {
            return;
        }
        let mut message = vec![0u8; u32::from_ne_bytes(size).min(1 << 20) as usize];
        if input.read_exact(&mut message).is_err() {
            return;
        }
        let message: serde_json::Value = serde_json::from_slice(&message).unwrap_or_default();
        let answer = serde_json::to_vec(&ask(&message)).unwrap_or_else(|_| b"{}".to_vec());
        let sent = output
            .write_all(&(answer.len() as u32).to_ne_bytes())
            .and_then(|()| output.write_all(&answer))
            .and_then(|()| output.flush());
        if sent.is_err() {
            return;
        }
    }
}

/// One question to the running app, as Safari's extension asks it.
fn ask(message: &serde_json::Value) -> serde_json::Value {
    let ask = message.get("ask").and_then(|a| a.as_str()).unwrap_or("wallet");
    let (path, post, wait) = match ask {
        "people" | "wallets" | "activity" => (ask, false, 3),
        "send" | "signin" | "signin-site" => (ask, true, 180),
        "quote" | "search" | "profiles" | "image" | "use-wallet" | "open-app" => (ask, true, 8),
        _ => ("wallet", false, 3),
    };
    let body = if post { serde_json::to_vec(message).unwrap_or_default() } else { Vec::new() };
    let answer = || -> Option<serde_json::Value> {
        let address = std::net::SocketAddr::from(([127, 0, 0, 1], crate::extension::PORT));
        let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(3)).ok()?;
        stream.set_read_timeout(Some(Duration::from_secs(wait))).ok()?;
        let head = format!(
            "{} /{path} HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Orien-Extension: 1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            if post { "POST" } else { "GET" },
            body.len()
        );
        stream.write_all(head.as_bytes()).ok()?;
        stream.write_all(&body).ok()?;
        let mut reply = Vec::new();
        stream.read_to_end(&mut reply).ok()?;
        let start = reply.windows(4).position(|w| w == b"\r\n\r\n")? + 4;
        serde_json::from_slice(&reply[start..]).ok()
    };
    // No answer: Orien Wallet isn't running.
    answer().unwrap_or_else(|| serde_json::json!({ "closed": true }))
}

/// Tells Firefox, if it's on this computer, where this program is.
pub fn register() {
    let Some(program) = program() else { return };
    let manifest = serde_json::json!({
        "name": HOST,
        "description": "Orien Wallet, for its Firefox extension",
        "path": program,
        "type": "stdio",
        "allowed_extensions": [EXTENSION],
    });
    let Ok(text) = serde_json::to_string_pretty(&manifest) else { return };
    let Some(file) = manifest_file() else { return };
    if std::fs::read_to_string(&file).ok().as_deref() != Some(text.as_str()) {
        let _ = file.parent().map(std::fs::create_dir_all);
        let _ = std::fs::write(&file, text);
    }
    #[cfg(windows)]
    point_registry_at(&file);
}

/// The program Firefox should start: the AppImage itself on Linux (the
/// program inside it moves on each start), this program elsewhere.
fn program() -> Option<String> {
    #[cfg(target_os = "linux")]
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        return Some(PathBuf::from(appimage).to_string_lossy().into_owned());
    }
    std::env::current_exe().ok().map(|exe| exe.to_string_lossy().into_owned())
}

/// Where Firefox looks for it: a set folder on Macs and Linux (only when
/// Firefox has been used here), anywhere on Windows, named in the registry.
fn manifest_file() -> Option<PathBuf> {
    let name = format!("{HOST}.json");
    #[cfg(target_os = "macos")]
    {
        let support = PathBuf::from(std::env::var_os("HOME")?).join("Library/Application Support");
        support.join("Firefox").exists().then(|| support.join("Mozilla/NativeMessagingHosts").join(name))
    }
    #[cfg(target_os = "linux")]
    {
        let mozilla = PathBuf::from(std::env::var_os("HOME")?).join(".mozilla");
        mozilla.exists().then(|| mozilla.join("native-messaging-hosts").join(name))
    }
    #[cfg(windows)]
    {
        Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Orien Wallet").join(name))
    }
}

/// HKEY_CURRENT_USER\Software\Mozilla\NativeMessagingHosts\network.orien.wallet
/// = the manifest's path.
#[cfg(windows)]
fn point_registry_at(file: &std::path::Path) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::Registry::{HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyExW, RegSetValueExW};
    let wide = |text: &std::ffi::OsStr| text.encode_wide().chain(Some(0)).collect::<Vec<u16>>();
    let key_name = wide(std::ffi::OsStr::new(&format!(r"Software\Mozilla\NativeMessagingHosts\{HOST}")));
    let value = wide(file.as_os_str());
    let mut key: HKEY = std::ptr::null_mut();
    // SAFETY: plain registry calls with buffers that outlive them.
    unsafe {
        let made = RegCreateKeyExW(HKEY_CURRENT_USER, key_name.as_ptr(), 0, std::ptr::null(), REG_OPTION_NON_VOLATILE, KEY_WRITE, std::ptr::null(), &mut key, std::ptr::null_mut());
        if made != 0 {
            return;
        }
        RegSetValueExW(key, std::ptr::null(), 0, REG_SZ, value.as_ptr().cast(), (value.len() * 2) as u32);
        RegCloseKey(key);
    }
}
