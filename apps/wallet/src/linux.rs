//! On Linux, an AppImage that is simply double-clicked has no app entry, so
//! the desktop shows a generic icon for its window and lists it nowhere.
//! Each time it starts, the app writes its entry and icon into the user's
//! own folders, pointing at wherever the AppImage is now, so they follow it
//! when it moves or updates itself.

use std::fs;
use std::path::PathBuf;

const ICON_128: &[u8] = include_bytes!("../icons/128x128.png");
const ICON_256: &[u8] = include_bytes!("../icons/128x128@2x.png");

pub fn register_appimage() {
    let (Some(appimage), Some(home)) = (std::env::var_os("APPIMAGE"), std::env::var_os("HOME")) else {
        return;
    };
    let share = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&home).join(".local/share"));
    let icons = share.join("icons/hicolor");
    for (size, bytes) in [("128x128", ICON_128), ("256x256", ICON_256)] {
        let dir = icons.join(size).join("apps");
        if fs::create_dir_all(&dir).is_ok() {
            let _ = fs::write(dir.join("orien-wallet.png"), bytes);
        }
    }
    let exec = appimage.to_string_lossy().replace('"', "\\\"");
    let entry = |hidden: bool| {
        format!(
            "[Desktop Entry]\nType=Application\nName=Orien Wallet\nComment=Send and receive ORN\nExec=\"{exec}\"\n\
             Icon=orien-wallet\nStartupWMClass=orien-wallet\nCategories=Office;Finance;\nTerminal=false\n{}",
            if hidden { "NoDisplay=true\n" } else { "" }
        )
    };
    let applications = share.join("applications");
    if fs::create_dir_all(&applications).is_ok() {
        // The window is matched by its program name on X11, and on Wayland
        // by its app id, which may be the app's identifier instead. The
        // second entry is hidden, so the menu lists Orien once.
        let _ = fs::write(applications.join("orien-wallet.desktop"), entry(false));
        let _ = fs::write(applications.join("network.orien.wallet.desktop"), entry(true));
    }
}
