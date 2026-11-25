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

/// Whether closing the window keeps the wallet running in the menu bar or
/// tray (the default), or quits it. The person's choice, kept in a file.
static KEEP_RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

fn quits_file(app: &AppHandle) -> Option<std::path::PathBuf> {
    crate::folder(app).ok().map(|dir| dir.join("closing-quits"))
}

pub fn keep_running() -> bool {
    KEEP_RUNNING.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_keep_running(app: &AppHandle, keep: bool) {
    KEEP_RUNNING.store(keep, std::sync::atomic::Ordering::Relaxed);
    if let Some(file) = quits_file(app) {
        if keep {
            let _ = std::fs::remove_file(file);
        } else {
            let _ = std::fs::write(file, b"");
        }
    }
}

/// Whether new versions install by themselves while the person is away
/// from the app (the default), or only when they say so.
pub fn updates_itself(app: &AppHandle) -> bool {
    !crate::folder(app).is_ok_and(|dir| dir.join("updates-by-hand").exists())
}

pub fn set_updates_itself(app: &AppHandle, on: bool) -> Result<(), String> {
    let file = crate::folder(app)?.join("updates-by-hand");
    if on { std::fs::remove_file(file).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(e) }) } else { std::fs::write(file, b"") }
        .map_err(|error| error.to_string())
}

/// Whether the app opens at login.
pub fn opens_at_login(app: &AppHandle) -> bool {
    #[cfg(target_os = "macos")]
    {
        let _ = app;
        mac::is_login_item()
    }
    #[cfg(not(target_os = "macos"))]
    app.autolaunch().is_enabled().unwrap_or(false)
}

pub fn set_opens_at_login(app: &AppHandle, on: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let _ = app;
        if on { mac::open_at_login().then_some(()).ok_or_else(|| "macOS didn't allow it.".to_string()) } else { mac::stop_opening_at_login() }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let manager = app.autolaunch();
        if on { manager.enable() } else { manager.disable() }.map_err(|error| error.to_string())
    }
}

/// What the window remembers between launches: its size, where it was, and
/// whether it filled the screen.
pub const WINDOW_STATE: tauri_plugin_window_state::StateFlags = tauri_plugin_window_state::StateFlags::SIZE
    .union(tauri_plugin_window_state::StateFlags::POSITION)
    .union(tauri_plugin_window_state::StateFlags::MAXIMIZED);

/// Saves the window's size and place for the next launch.
pub fn remember_window(app: &AppHandle) {
    use tauri_plugin_window_state::AppHandleExt;
    let _ = app.save_window_state(WINDOW_STATE);
}

/// Once, after the update that brought the wide layouts: a window still at
/// the old default width (719, so never widened) opens at the new default,
/// 1200 by 800, centred. A size someone chose is left alone, and so is
/// every window after this first look.
pub fn widen_once(app: &AppHandle) {
    let Ok(dir) = app.path().app_config_dir() else { return };
    let marker = dir.join("window-widened");
    if marker.exists() {
        return;
    }
    // The saved size is put back a moment after start: look once it has.
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(600));
        let Some(window) = app.get_webview_window("main") else { return };
        let (Ok(size), Ok(scale)) = (window.inner_size(), window.scale_factor()) else { return };
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(&marker, b"1");
        let width = size.width as f64 / scale;
        if (width - 719.0).abs() < 1.5 && !window.is_maximized().unwrap_or(false) {
            let _ = window.set_size(tauri::LogicalSize::new(1200.0, 800.0));
            let _ = window.center();
            remember_window(&app);
        }
    });
}

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
    let updates = MenuItem::with_id(app, "check-updates", "Check for Updates…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Orien Wallet", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &updates, &quit])?;
    // The Mac's menu bar wants a one-colour picture it tints itself.
    #[cfg(target_os = "macos")]
    let icon = Image::from_bytes(include_bytes!("../icons/tray-mac.png"))?;
    #[cfg(not(target_os = "macos"))]
    // The sunrise alone, as on the website's tab: the app's square icon
    // shrinks to an unreadable dot in a tray.
    let icon = Image::from_bytes(include_bytes!("../icons/tray.png"))?;
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
            "quit" => {
                remember_window(app);
                app.exit(0)
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show(tray.app_handle());
            }
        })
        .build(app);
    // On Macs, "Check for Updates…" in the app's menu, under About.
    #[cfg(target_os = "macos")]
    {
        let bar = Menu::default(app)?;
        if let Some(tauri::menu::MenuItemKind::Submenu(first)) = bar.items()?.into_iter().next() {
            let item = MenuItem::with_id(app, "check-updates", "Check for Updates…", true, None::<&str>)?;
            first.insert(&item, 1)?;
            let settings = MenuItem::with_id(app, "settings", "Settings…", true, Some("CmdOrCtrl+,"))?;
            first.insert(&settings, 2)?;
        }
        app.set_menu(bar)?;
    }
    // Every menu's clicks reach here, the tray's included: answered once.
    app.on_menu_event(|app, event| {
        match event.id().as_ref() {
            "check-updates" => crate::update::check_now(app.clone()),
            "settings" => {
                use tauri::Emitter;
                show(app);
                let _ = app.emit("open-settings", ());
            }
            _ => {}
        }
    });

    // Some Linux desktops have no tray. Then closing the window really
    // closes the app (a Mac always has its Dock icon to come back by).
    let can_come_back = tray.is_ok() || cfg!(target_os = "macos");
    if let Err(error) = tray {
        eprintln!("no tray icon: {error}");
    }

    // The size and place are saved a moment after each resize or move, so
    // they're kept however the app ends (a Mac's Quit skips the plugin's
    // own save).
    if let Some(window) = app.get_webview_window("main") {
        let handle = app.clone();
        window.on_window_event(move |event| {
            if let WindowEvent::Focused(focused) = event {
                crate::update::window_focus(*focused);
            }
            if matches!(event, WindowEvent::Resized(_) | WindowEvent::Moved(_)) {
                static MOVES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                let this = MOVES.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                let handle = handle.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(700));
                    if MOVES.load(std::sync::atomic::Ordering::Relaxed) == this {
                        remember_window(&handle);
                    }
                });
            }
        });
    }

    // Closing the window only hides it: the wallet keeps going.
    if let Some(window) = app.get_webview_window("main")
        && can_come_back
    {
        if quits_file(app).is_some_and(|file| file.exists()) {
            KEEP_RUNNING.store(false, std::sync::atomic::Ordering::Relaxed);
        }
        let hidden = window.clone();
        let handle = app.clone();
        window.on_window_event(move |event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                remember_window(&handle);
                if keep_running() {
                    api.prevent_close();
                    let _ = hidden.hide();
                } else {
                    handle.exit(0);
                }
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

    // After an update that installed by itself: back the way it was, never
    // in front.
    match crate::update::reopen_as(app).as_deref() {
        Some("hidden") if can_come_back => return Ok(()),
        Some("minimized") => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.minimize();
            }
            return Ok(());
        }
        Some("behind") | Some("hidden") => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
            }
            return Ok(());
        }
        _ => {}
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

    /// Whether the app is on the Mac's list of login items.
    pub fn is_login_item() -> bool {
        if AnyClass::get(c"SMAppService").is_none() {
            return false;
        }
        // SAFETY: the class exists on this macOS, checked just above.
        let service = unsafe { SMAppService::mainAppService() };
        let status = unsafe { service.status() };
        status == objc2_service_management::SMAppServiceStatus::Enabled
    }

    /// Takes the app off the Mac's list of login items.
    pub fn stop_opening_at_login() -> Result<(), String> {
        if AnyClass::get(c"SMAppService").is_none() {
            return Ok(());
        }
        // SAFETY: the class exists on this macOS, checked just above.
        let service = unsafe { SMAppService::mainAppService() };
        unsafe { service.unregisterAndReturnError() }.map_err(|error| error.localizedDescription().to_string())
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
