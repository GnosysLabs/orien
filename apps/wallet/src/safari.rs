//! The Safari extension: it holds no wallet. The app leaves the wallet's
//! address and balance where the extension can read them, in the folder
//! the two share (an app group), and does all signing itself.

/// The folder the app and its Safari extension share.
const GROUP: &str = "4PDUNTF69S.network.orien.wallet";

/// Leaves the address and balance for the extension to show.
pub fn share(address: &str, balance: Option<&str>) {
    let Some(home) = std::env::var_os("HOME") else { return };
    let dir = std::path::Path::new(&home).join("Library/Group Containers").join(GROUP);
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let seen = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let text = serde_json::json!({ "address": address, "balance": balance, "seen": seen }).to_string();
    let file = dir.join("wallet.json");
    let draft = dir.join("wallet.json.new");
    if std::fs::write(&draft, text).is_ok() {
        let _ = std::fs::rename(draft, file);
    }
}
