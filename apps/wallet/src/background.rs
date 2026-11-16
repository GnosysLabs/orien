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
        if !marker.exists() && app.autolaunch().enable().is_ok() {
            let _ = std::fs::write(marker, b"");
        }
    }

    // Started at login: stay out of sight. Otherwise, show the window.
    if !(can_come_back && std::env::args().any(|arg| arg == QUIET)) {
        show(app);
    }
    Ok(())
}
