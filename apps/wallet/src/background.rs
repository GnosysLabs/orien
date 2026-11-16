//! On computers the wallet keeps running with its window closed, with an
//! icon in the menu bar (Mac) or the system tray (Windows, Linux), and
//! starts by itself at login. So a taken swap offer goes ahead, and money
//! arriving is announced, without the window open.

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WindowEvent};
use tauri_plugin_autostart::ManagerExt;

/// The argument the login start passes, to start with no window.
pub const QUIET: &str = "--background";

/// Shows the window, in front.
pub fn show(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Sets up the icon, the window's close button, and starting at login.
pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Orien Wallet", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Orien Wallet", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    // The Mac's menu bar wants a one-colour picture it tints itself.
    #[cfg(target_os = "macos")]
    let icon = Image::from_bytes(include_bytes!("../icons/tray-mac.png"))?;
    #[cfg(not(target_os = "macos"))]
    let icon = Image::from_bytes(include_bytes!("../icons/32x32.png"))?;
    let tray = TrayIconBuilder::with_id("orien")
        .icon(icon)
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("Orien Wallet")
        .menu(&menu)
        // The Mac opens the menu on a click; elsewhere a click opens the
        // window and the menu is on the right button.
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show(tray.app_handle());
            }
        })
        .build(app);
    // Some Linux desktops have no tray. Then closing the window really
    // closes the app (a Mac always has its Dock icon to come back by).
    let can_come_back = tray.is_ok() || cfg!(target_os = "macos");
    if let Err(error) = tray {
        eprintln!("no tray icon: {error}");
    }

    // Closing the window only hides it: the wallet keeps going.
    if let Some(window) = app.get_webview_window("main")
        && can_come_back
    {
        let hidden = window.clone();
        window.on_window_event(move |event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = hidden.hide();
            }
        });
    }

    // Starting at login is set up once, the first time the app runs; after
    // that it's left as the person has it (System Settings can turn it off).
    if let Ok(dir) = crate::folder(app) {
        let marker = dir.join("starts-at-login");
        #[cfg(target_os = "macos")]
        {
            // Newer Macs open the app at login their own way, which names it
            // "Orien Wallet" (the older way named the developer). Moved over
            // once, from 1.7.0's way too.
            let moved = dir.join("opens-at-login");
            if !moved.exists() && mac::open_at_login() {
                if marker.exists() {
                    let _ = app.autolaunch().disable();
                }
                let _ = std::fs::write(&marker, b"");
                let _ = std::fs::write(moved, b"");
            }
        }
        if !marker.exists() && app.autolaunch().enable().is_ok() {
            let _ = std::fs::write(marker, b"");
        }
    }

    // Started at login: stay out of sight. Otherwise, show the window.
    let at_login = std::env::args().any(|arg| arg == QUIET);
    #[cfg(target_os = "macos")]
    let at_login = at_login || mac::opened_at_login();
    if !(can_come_back && at_login) {
        show(app);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::runtime::AnyClass;
    use objc2_foundation::NSAppleEventManager;
    use objc2_service_management::SMAppService;

    /// Asks macOS to open the app at login, as System Settings' Login Items
    /// list does. Only on macOS 13 and later; false where it can't.
    pub fn open_at_login() -> bool {
        if AnyClass::get(c"SMAppService").is_none() {
            return false;
        }
        // SAFETY: the class exists on this macOS, checked just above.
        let service = unsafe { SMAppService::mainAppService() };
        // Already on the list counts as done.
        match unsafe { service.registerAndReturnError() } {
            Ok(()) => true,
            Err(error) => error.code() == objc2_service_management::kSMErrorAlreadyRegistered as isize,
        }
    }

    /// Whether macOS opened the app as a login item: the "open" it sent
    /// says so.
    pub fn opened_at_login() -> bool {
        const PROPERTY_DATA: u32 = u32::from_be_bytes(*b"prdt");
        const LOGIN_ITEM: u32 = u32::from_be_bytes(*b"lgit");
        NSAppleEventManager::sharedAppleEventManager()
            .currentAppleEvent()
            .and_then(|event| event.paramDescriptorForKeyword(PROPERTY_DATA))
            .is_some_and(|data| data.enumCodeValue() == LOGIN_ITEM)
    }
}
