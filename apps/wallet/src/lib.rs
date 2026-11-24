//! The Orien wallet as a desktop app. The window is plain HTML in `ui/`;
//! everything that touches keys or the network happens here, in Rust,
//! through the same code the `orien` command uses.
//!
//! The wallet itself is the same file the `orien` command uses,
//! `~/.orien/wallet` (or `$ORIEN_HOME/wallet`), so the app and the command
//! line always agree. The app keeps its own things in its own folder (on a
//! Mac, `~/Library/Application Support/network.orien.wallet`): the network
//! file it was built with, how far it has followed the network, payment
//! history, contacts, and a note of any other wallet file the person chose.


mod activity;
mod approval;
#[cfg(desktop)]
mod background;
mod board;
mod biometric;
mod contacts;
mod haptics;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(desktop)]
mod large_pages;
#[cfg(desktop)]
mod mining;
mod money;
mod pair;
#[cfg(target_os = "ios")]
mod push;
#[cfg(desktop)]
mod extension;
#[cfg(desktop)]
mod firefox;
mod sitesignin;

/// iPhones: no "previous / next / done" bar above the keyboard. Web views
/// add it to every text box; apps don't have it.
#[cfg(target_os = "ios")]
mod keyboard {
    use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
    use objc2::sel;

    unsafe extern "C-unwind" fn none(_this: &AnyObject, _cmd: Sel) -> *mut AnyObject {
        std::ptr::null_mut()
    }

    /// The app's web view: the keyboard makes it shorter, as an iPhone app's
    /// screen gets shorter, so the page always fits above the keyboard.
    static WEB: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    /// Follows the keyboard: each time it moves, the web view is resized to
    /// end at its top edge (full height again when it goes), and the page is
    /// told how much it covers (0 when down).
    pub fn follow(app: tauri::AppHandle) {
        use objc2_foundation::{NSNotification, NSNotificationCenter, NSValue};
        use objc2_ui_kit::{UIKeyboardFrameEndUserInfoKey, UIKeyboardWillChangeFrameNotification, UIKeyboardWillHideNotification};
        use tauri::Emitter;
        let center = NSNotificationCenter::defaultCenter();
        for (name, hide) in [(unsafe { UIKeyboardWillChangeFrameNotification }, false), (unsafe { UIKeyboardWillHideNotification }, true)] {
            let app = app.clone();
            let block = block2::RcBlock::new(move |note: std::ptr::NonNull<NSNotification>| {
                let note = unsafe { note.as_ref() };
                let top = if hide {
                    None
                } else {
                    note.userInfo()
                        .and_then(|info| info.objectForKey(unsafe { UIKeyboardFrameEndUserInfoKey }))
                        .and_then(|value| value.downcast::<NSValue>().ok())
                        .map(|value| unsafe { objc2_ui_kit::NSValueUIGeometryExtensions::CGRectValue(&*value) }.origin.y)
                };
                let covered = fit(top);
                let _ = app.emit("keyboard", covered);
            });
            let observer = unsafe { center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block) };
            std::mem::forget(observer);
        }
    }

    /// Makes the web view end at `top` (in screen points), or fill its place
    /// when None. Returns how many points the keyboard covers.
    fn fit(top: Option<f64>) -> f64 {
        use objc2::msg_send;
        use objc2_core_foundation::CGRect;
        let web = WEB.load(std::sync::atomic::Ordering::Relaxed) as *mut AnyObject;
        if web.is_null() {
            return 0.0;
        }
        unsafe {
            let holder: *mut AnyObject = msg_send![web, superview];
            if holder.is_null() {
                return 0.0;
            }
            let room: CGRect = msg_send![holder, bounds];
            let covered = top.map(|top| (room.size.height - top).max(0.0)).unwrap_or(0.0);
            let mut frame = room;
            frame.size.height = room.size.height - covered;
            let _: () = msg_send![web, setFrame: frame];
            covered
        }
    }

    /// Holds the web view still and takes the keyboard away from it: left to
    /// itself it pads and slides the page for the keyboard, which fights the
    /// app fitting itself above it (the page jumps and jitters). Without
    /// that, the web view is simply made shorter, and nothing else moves.
    pub fn hold_still(window: &tauri::WebviewWindow) {
        let _ = window.with_webview(|webview| unsafe {
            use objc2::msg_send;
            use objc2_foundation::NSNotificationCenter;
            let web = webview.inner() as *mut AnyObject;
            if web.is_null() {
                return;
            }
            let scroll: *mut AnyObject = msg_send![web, scrollView];
            if !scroll.is_null() {
                let _: () = msg_send![scroll, setScrollEnabled: false];
                let _: () = msg_send![scroll, setBounces: false];
                // UIScrollViewContentInsetAdjustmentNever
                let _: () = msg_send![scroll, setContentInsetAdjustmentBehavior: 2isize];
            }
            let center = NSNotificationCenter::defaultCenter();
            for name in [
                objc2_ui_kit::UIKeyboardWillShowNotification,
                objc2_ui_kit::UIKeyboardDidShowNotification,
                objc2_ui_kit::UIKeyboardWillHideNotification,
                objc2_ui_kit::UIKeyboardDidHideNotification,
                objc2_ui_kit::UIKeyboardWillChangeFrameNotification,
                objc2_ui_kit::UIKeyboardDidChangeFrameNotification,
            ] {
                let _: () = msg_send![&*center, removeObserver: web, name: name, object: std::ptr::null_mut::<AnyObject>()];
            }
            // Behind the web view, the app's own colour (#1b1c1f), so the
            // moment the keyboard is still rising shows no black band.
            let holder: *mut AnyObject = msg_send![web, superview];
            let color: *mut AnyObject = msg_send![objc2::class!(UIColor), colorWithRed: 27.0f64 / 255.0, green: 28.0f64 / 255.0, blue: 31.0f64 / 255.0, alpha: 1.0f64];
            if !holder.is_null() {
                let _: () = msg_send![holder, setBackgroundColor: color];
            }
            // The scroll view's own colour too (black by default), for any
            // moment it shows past the page.
            if !scroll.is_null() {
                let _: () = msg_send![scroll, setBackgroundColor: color];
            }
            let _: () = msg_send![web, setOpaque: false];
            let _: () = msg_send![web, setBackgroundColor: color];
            SCROLL.store(scroll as usize, std::sync::atomic::Ordering::Relaxed);
            pin_page();
            WEB.store(web as usize, std::sync::atomic::Ordering::Relaxed);
        });
    }

    /// The app's web view's scroll view: its page is pinned at the top.
    static SCROLL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    static SET_OFFSET: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    static SET_OFFSET_ANIMATED: std::sync::OnceLock<usize> = std::sync::OnceLock::new();

    type SetOffset = unsafe extern "C-unwind" fn(*mut AnyObject, Sel, objc2_core_foundation::CGPoint);
    type SetOffsetAnimated = unsafe extern "C-unwind" fn(*mut AnyObject, Sel, objc2_core_foundation::CGPoint, objc2::runtime::Bool);

    /// The point the app's page is moved to: always the top. iOS moves it
    /// to show a tapped box (the dark band above), but the page never
    /// needs moving: it fits above the keyboard and screens scroll inside.
    fn pinned(this: *mut AnyObject, point: objc2_core_foundation::CGPoint) -> objc2_core_foundation::CGPoint {
        if this as usize == SCROLL.load(std::sync::atomic::Ordering::Relaxed) {
            objc2_core_foundation::CGPoint { x: 0.0, y: 0.0 }
        } else {
            point
        }
    }

    unsafe extern "C-unwind" fn set_offset(this: *mut AnyObject, cmd: Sel, point: objc2_core_foundation::CGPoint) {
        if let Some(&original) = SET_OFFSET.get() {
            let original = unsafe { std::mem::transmute::<usize, SetOffset>(original) };
            unsafe { original(this, cmd, pinned(this, point)) }
        }
    }

    unsafe extern "C-unwind" fn set_offset_animated(this: *mut AnyObject, cmd: Sel, point: objc2_core_foundation::CGPoint, animated: objc2::runtime::Bool) {
        if let Some(&original) = SET_OFFSET_ANIMATED.get() {
            let original = unsafe { std::mem::transmute::<usize, SetOffsetAnimated>(original) };
            unsafe { original(this, cmd, pinned(this, point), animated) }
        }
    }

    /// Replaces the web view's scroll view's offset setters (on its own class
    /// only, never UIScrollView's) with ones that keep the app's page at the
    /// top. Other scroll views pass straight through.
    fn pin_page() {
        let Some(class) = AnyClass::get(c"WKScrollView") else { return };
        let class = class as *const AnyClass as *mut AnyClass;
        unsafe {
            let plain = std::mem::transmute::<SetOffset, Imp>(set_offset);
            let animated = std::mem::transmute::<SetOffsetAnimated, Imp>(set_offset_animated);
            for (selector, replacement, keep, types) in [
                (sel!(setContentOffset:), plain, &SET_OFFSET, c"v@:{CGPoint=dd}"),
                (sel!(setContentOffset:animated:), animated, &SET_OFFSET_ANIMATED, c"v@:{CGPoint=dd}B"),
            ] {
                if keep.get().is_some() {
                    continue;
                }
                let method = objc2::ffi::class_getInstanceMethod(class, selector);
                if method.is_null() {
                    continue;
                }
                let original = objc2::ffi::method_getImplementation(method);
                let Some(original) = original else { continue };
                let _ = keep.set(original as usize);
                // An override of its own on WKScrollView, so UIScrollView's
                // stays as it is; or WKScrollView's own, replaced.
                if !objc2::ffi::class_addMethod(class, selector, replacement, types.as_ptr()).as_bool() {
                    objc2::ffi::method_setImplementation(method as *mut _, replacement);
                }
            }
        }
    }

    /// Sets the colour behind and around the web view, for the theme.
    /// Opens the share sheet for `text` and, when given, a picture (PNG).
    /// The web view's own share opens as a small panel pinned to the side
    /// and cut off by the screen's edge; iPhones get the usual sheet from
    /// the bottom this way.
    pub fn share(text: &str, png: Option<&[u8]>) {
        use objc2::{class, msg_send};
        use objc2_foundation::NSString;
        let web = WEB.load(std::sync::atomic::Ordering::Relaxed) as *mut AnyObject;
        if web.is_null() {
            return;
        }
        // SAFETY: plain UIKit calls on the main thread with live objects;
        // the controller made here is released once presented.
        unsafe {
            let window: *mut AnyObject = msg_send![web, window];
            if window.is_null() {
                return;
            }
            let mut top: *mut AnyObject = msg_send![window, rootViewController];
            if top.is_null() {
                return;
            }
            loop {
                let next: *mut AnyObject = msg_send![top, presentedViewController];
                if next.is_null() {
                    break;
                }
                top = next;
            }
            let items: *mut AnyObject = msg_send![class!(NSMutableArray), array];
            // The picture goes as a file, "Orien address.png", as the
            // computers share it.
            if let Some(png) = png {
                let path = std::env::temp_dir().join("Orien address.png");
                if std::fs::write(&path, png).is_ok() {
                    let path = NSString::from_str(&path.to_string_lossy());
                    let url: *mut AnyObject = msg_send![class!(NSURL), fileURLWithPath: &*path];
                    if !url.is_null() {
                        let _: () = msg_send![items, addObject: url];
                    }
                }
            }
            let text = NSString::from_str(text);
            let _: () = msg_send![items, addObject: &*text];
            let sheet: *mut AnyObject = msg_send![class!(UIActivityViewController), alloc];
            let sheet: *mut AnyObject = msg_send![sheet, initWithActivityItems: items, applicationActivities: std::ptr::null_mut::<AnyObject>()];
            if sheet.is_null() {
                return;
            }
            let device: *mut AnyObject = msg_send![class!(UIDevice), currentDevice];
            let idiom: isize = msg_send![device, userInterfaceIdiom];
            if idiom == 1 {
                // iPads show it as a panel and need a place to point at:
                // the bottom middle of the screen.
                let panel: *mut AnyObject = msg_send![sheet, popoverPresentationController];
                if !panel.is_null() {
                    let bounds: objc2_core_foundation::CGRect = msg_send![web, bounds];
                    let at = objc2_core_foundation::CGRect::new(
                        objc2_core_foundation::CGPoint::new(bounds.size.width / 2.0, bounds.size.height - 80.0),
                        objc2_core_foundation::CGSize::new(1.0, 1.0),
                    );
                    let _: () = msg_send![panel, setSourceView: web];
                    let _: () = msg_send![panel, setSourceRect: at];
                }
                let _: () = msg_send![top, presentViewController: sheet, animated: true, completion: std::ptr::null_mut::<AnyObject>()];
            } else {
                // iPhones: on newer iOS the share sheet insists on coming up
                // as a narrow floating panel in the corner, its rows cut off,
                // whatever style or place it is given. So it goes inside a
                // sheet of our own, the usual one from the bottom, full width,
                // half height and pulled up to full.
                let holder: *mut AnyObject = msg_send![class!(UIViewController), new];
                let _: () = msg_send![holder, setModalPresentationStyle: 1isize]; // page sheet
                let sizes: *mut AnyObject = msg_send![holder, sheetPresentationController];
                if !sizes.is_null() {
                    let half: *mut AnyObject = msg_send![class!(UISheetPresentationControllerDetent), mediumDetent];
                    let full: *mut AnyObject = msg_send![class!(UISheetPresentationControllerDetent), largeDetent];
                    let detents: *mut AnyObject = msg_send![class!(NSMutableArray), array];
                    let _: () = msg_send![detents, addObject: half];
                    let _: () = msg_send![detents, addObject: full];
                    let _: () = msg_send![sizes, setDetents: detents];
                    let _: () = msg_send![sizes, setPrefersGrabberVisible: true];
                }
                let inside: *mut AnyObject = msg_send![holder, view];
                let _: () = msg_send![holder, addChildViewController: sheet];
                let shown: *mut AnyObject = msg_send![sheet, view];
                let bounds: objc2_core_foundation::CGRect = msg_send![inside, bounds];
                let _: () = msg_send![shown, setFrame: bounds];
                let _: () = msg_send![shown, setAutoresizingMask: 18usize]; // follows the sheet's width and height
                let _: () = msg_send![inside, addSubview: shown];
                let _: () = msg_send![sheet, didMoveToParentViewController: holder];
                // Once something is shared, or the sheet's own close is
                // pressed, our sheet goes too.
                // (The share sheet holds this; while it runs, the share sheet
                // is alive and its parent is ours or gone.)
                let asked = sheet as usize;
                let done = block2::RcBlock::new(move |_: *mut AnyObject, _: objc2::runtime::Bool, _: *mut AnyObject, _: *mut AnyObject| {
                    let holder: *mut AnyObject = msg_send![asked as *mut AnyObject, parentViewController];
                    if holder.is_null() {
                        return;
                    }
                    let under: *mut AnyObject = msg_send![holder, presentingViewController];
                    if !under.is_null() {
                        let _: () = msg_send![holder, dismissViewControllerAnimated: true, completion: std::ptr::null_mut::<AnyObject>()];
                    }
                });
                let _: () = msg_send![sheet, setCompletionWithItemsHandler: &*done];
                let _: () = msg_send![top, presentViewController: holder, animated: true, completion: std::ptr::null_mut::<AnyObject>()];
                let _: () = msg_send![holder, release];
            }
            let _: () = msg_send![sheet, release];
        }
    }

    pub fn backdrop(red: u8, green: u8, blue: u8, light: bool) {
        use objc2::msg_send;
        let web = WEB.load(std::sync::atomic::Ordering::Relaxed) as *mut AnyObject;
        if web.is_null() {
            return;
        }
        unsafe {
            let (r, g, b) = (red as f64 / 255.0, green as f64 / 255.0, blue as f64 / 255.0);
            let color: *mut AnyObject = msg_send![objc2::class!(UIColor), colorWithRed: r, green: g, blue: b, alpha: 1.0f64];
            let _: () = msg_send![web, setBackgroundColor: color];
            let scroll: *mut AnyObject = msg_send![web, scrollView];
            if !scroll.is_null() {
                let _: () = msg_send![scroll, setBackgroundColor: color];
            }
            let holder: *mut AnyObject = msg_send![web, superview];
            if !holder.is_null() {
                let _: () = msg_send![holder, setBackgroundColor: color];
            }
            // The clock and battery at the top: dark on a light theme.
            let window: *mut AnyObject = msg_send![web, window];
            if !window.is_null() {
                // UIUserInterfaceStyleLight = 1, Dark = 2
                let _: () = msg_send![window, setOverrideUserInterfaceStyle: if light { 1isize } else { 2isize }];
            }
        }
    }

    pub fn hide_accessory_bar() {
        let Some(class) = AnyClass::get(c"WKContentView") else { return };
        // SAFETY: the replacement has the method's shape: an object, its
        // selector, an object (nil) back.
        unsafe {
            let imp = std::mem::transmute::<unsafe extern "C-unwind" fn(&AnyObject, Sel) -> *mut AnyObject, Imp>(none);
            let class = class as *const AnyClass as *mut AnyClass;
            let method = objc2::ffi::class_getInstanceMethod(class, sel!(inputAccessoryView));
            if method.is_null() {
                objc2::ffi::class_addMethod(class, sel!(inputAccessoryView), imp, c"@@:".as_ptr());
            } else {
                objc2::ffi::method_setImplementation(method as *mut _, imp);
            }
        }
    }
}
mod send;
mod solswaps;
mod swaps;
#[cfg(desktop)]
mod update;
mod vault;
/// iPhone copies come from the App Store, which updates them.
#[cfg(mobile)]
mod update {
    #[tauri::command]
    pub async fn check_update() -> Result<Option<()>, String> {
        Ok(None)
    }
    #[tauri::command]
    pub async fn install_update() -> Result<(), String> {
        Err("Updates come from the App Store.".into())
    }
    #[tauri::command]
    pub async fn download_update() -> Result<Option<String>, String> {
        Ok(None)
    }
    #[tauri::command]
    pub fn away_seconds() -> u64 {
        0
    }
}

use orien_ledger::AccountId;
use orien_net::client::{Client, Head};
use orien_net::keys::{OpenError, Wallet, load_genesis};
use orien_net::unlock::Signing;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

/// Sending and checking the balance take turns, so a check that runs while
/// a payment is going out never sees half of it and records it twice.
static TURNS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The network this copy of the app was built for.
const GENESIS: &str = include_str!(concat!(env!("OUT_DIR"), "/genesis.json"));

/// The balance the validators last agreed on, in units, for fee quotes.
static LAST_BALANCE: std::sync::Mutex<Option<i128>> = std::sync::Mutex::new(None);

/// What the window shows about a wallet.
#[derive(Serialize)]
struct Overview {
    address: String,
    /// As people read it, such as "1,250.5". `None` while too few
    /// validators are caught up to agree on it.
    balance: Option<String>,
    /// The most that can be sent once the fee is added, written plainly for
    /// the amount box.
    most: Option<String>,
    #[serde(skip)]
    units: Option<i128>,
    payments: u64,
    /// The wallet has a password, so sending needs it.
    locked: bool,
    /// The password is kept behind Touch ID or Face ID.
    biometric: bool,
    /// The wallet has no password at all.
    unprotected: bool,
    /// This system offers Touch ID or Face ID.
    biometric_available: bool,
    answered: usize,
    validators: usize,
    epoch: u64,
}

/// Which wallet the app uses, kept in `settings.json`.
#[derive(Default, Serialize, Deserialize)]
struct Settings {
    /// A wallet file the person chose. The app uses it where it is and never
    /// copies it: two copies of one wallet could sign two different payments
    /// with the same one-time key.
    wallet: Option<PathBuf>,
    /// The network the choice was made on. A choice made while trying the
    /// app on a test network is ignored on any other.
    #[serde(default)]
    network: Option<String>,
    /// Every wallet the app knows besides the shared one, and the names
    /// given to any of them (the shared one's included).
    #[serde(default)]
    wallets: Vec<Named>,
    /// The wallet open now; the first one when unset.
    #[serde(default)]
    current: Option<PathBuf>,
    /// How much of the processor mining used, in percent, before it was
    /// set in threads. Read once, for `mining_threads`.
    #[serde(default)]
    mining_percent: Option<u8>,
    /// How many threads mining uses.
    #[serde(default)]
    mining_threads: Option<usize>,
    /// Mining was running on this many threads when an update restarted
    /// the app: the new copy starts it again once, then clears this.
    #[serde(default)]
    resume_mining: Option<usize>,
    /// What the benchmark found on this computer.
    #[cfg(desktop)]
    #[serde(default)]
    mining_sweet: Option<mining::Sweet>,
    /// This app granted Windows' "Lock pages in memory" right to the
    /// account, so mining can use large pages from the next sign-in.
    #[serde(default)]
    large_pages_granted: bool,
    /// Signing in to websites: which one-time sign-in key this device uses
    /// next, for each wallet by address. Kept here, on this device only, so
    /// devices sharing a wallet never share a key.
    #[serde(default)]
    sign_ins: std::collections::BTreeMap<String, sitesignin::Count>,
}

/// The one-time sign-in key this device uses next for the wallet at
/// `address`, counted and saved before it signs. A new random bottom tree
/// once one is used up (or on the first sign-in).
fn next_sign_in_key(dir: &Path, address: &str) -> Result<(u32, u32), String> {
    let mut saved = settings(dir);
    let count = saved.sign_ins.entry(address.to_string()).or_default();
    if count.next == 0 || count.next >= 1 << orien_crypto::signin::BOTTOM_HEIGHT {
        let random = orien_net::keys::random_seed();
        count.slot = u32::from_le_bytes([random[0], random[1], random[2], 0]);
        count.next = 0;
    }
    let key = (count.slot, count.next);
    count.next += 1;
    save_settings(dir, &saved)?;
    Ok(key)
}

/// A wallet the app knows, with the short name the person gave it.
#[derive(Clone, Serialize, Deserialize)]
struct Named {
    path: PathBuf,
    name: String,
}

/// The id of the network this app was built for.
fn network_id() -> Option<String> {
    serde_json::from_str::<orien_ledger::Genesis>(GENESIS).ok().map(|genesis| genesis.id().to_string())
}

/// Where orien keeps its files: `$ORIEN_HOME`, else `~/.orien`, as the
/// `orien` command does.
fn orien_home() -> PathBuf {
    if let Some(home) = std::env::var_os("ORIEN_HOME") {
        return PathBuf::from(home);
    }
    let user = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).unwrap_or_default();
    // An iPhone app may write only inside its own folder, and no `orien`
    // command shares the wallet there: it lives in the app's support folder,
    // backed up with the phone like other app data.
    if cfg!(any(target_os = "ios", target_os = "android")) {
        return PathBuf::from(user).join("Library/Application Support/orien");
    }
    PathBuf::from(user).join(".orien")
}

/// The wallet a new or restored wallet is saved as: the one the `orien`
/// command uses too.
fn shared_wallet() -> PathBuf {
    orien_home().join("wallet")
}

fn folder(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|error| error.to_string())?;
    fs::create_dir_all(&dir).map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
    Ok(dir)
}

fn settings(dir: &Path) -> Settings {
    let mut settings: Settings =
        fs::read(dir.join("settings.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
    // An iPhone moves an app's folder when the app is installed again, so a
    // wallet the app made is looked for in its folder as it is now, by name.
    let here = |path: &PathBuf| -> PathBuf {
        if path.is_file() {
            return path.clone();
        }
        let in_folder = path.parent().and_then(Path::file_name).is_some_and(|folder| folder == "wallets");
        match path.file_name().map(|name| dir.join("wallets").join(name)) {
            Some(moved) if in_folder && moved.is_file() => moved,
            _ => path.clone(),
        }
    };
    for wallet in &mut settings.wallets {
        wallet.path = here(&wallet.path);
    }
    settings.current = settings.current.as_ref().map(here);
    settings
}

fn save_settings(dir: &Path, settings: &Settings) -> Result<(), String> {
    let text = serde_json::to_string_pretty(settings).map_err(|error| error.to_string())?;
    fs::write(dir.join("settings.json"), text).map_err(|error| error.to_string())
}

/// Every wallet the app knows, in order: the one the `orien` command uses,
/// one the person chose from a file, then the ones added in the app. Only
/// those whose file is there.
fn known_wallets(dir: &Path) -> Vec<Named> {
    let settings = settings(dir);
    let name_of = |path: &Path, default: &str| {
        settings.wallets.iter().find(|w| w.path == path).map_or_else(|| default.to_string(), |w| w.name.clone())
    };
    let mut all = Vec::new();
    let shared = shared_wallet();
    if shared.is_file() {
        all.push(Named { name: name_of(&shared, "Main"), path: shared });
    }
    if let Some(chosen) = settings.wallet.clone().filter(|path| path.is_file() && settings.network == network_id())
        && !all.iter().any(|w| w.path == chosen)
    {
        all.push(Named { name: name_of(&chosen, "Chosen wallet"), path: chosen });
    }
    for wallet in &settings.wallets {
        if wallet.path.is_file() && !all.iter().any(|w| w.path == wallet.path) {
            all.push(wallet.clone());
        }
    }
    all
}

/// The wallet file in use, if there is one: the one open now, else the
/// first the app knows.
fn wallet_file(dir: &Path) -> Option<PathBuf> {
    let all = known_wallets(dir);
    let current = settings(dir).current;
    all.iter().find(|w| Some(&w.path) == current.as_ref()).or(all.first()).map(|w| w.path.clone())
}

/// Whether the wallet file is sealed with a password, read without it.
fn has_password(wallet_file: &Path) -> bool {
    fs::read(wallet_file)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .is_some_and(|file| file.get("sealed").is_some_and(|sealed| !sealed.is_null()))
}

/// The wallet, ready to sign, opened with its password if it has one. The
/// window recognises the two answers that start with "password:".
fn open_signing(wallet_file: &Path, password: Option<&str>) -> Result<Signing, String> {
    let password = password.filter(|password| !password.is_empty());
    // Unlocked by Touch ID or Face ID a moment ago.
    let remembered = if password.is_none() { biometric::unlocked(wallet_file) } else { None };
    let password = password.or(remembered.as_deref());
    Wallet::open(wallet_file, password).map(Signing::Here).map_err(|error| match error.downcast_ref::<OpenError>() {
        Some(OpenError::NeedsPassword) => "password: needed".to_string(),
        Some(OpenError::WrongPassword) => "password: wrong".to_string(),
        None => format!("could not open the wallet: {error}"),
    })
}

/// Writes the built-in network file to the app's folder, where the client
/// reads it and keeps its progress next to it.
fn genesis_file(dir: &Path) -> Result<PathBuf, String> {
    if GENESIS.is_empty() {
        return Err("this copy of the app was built without a network".into());
    }
    let path = dir.join("genesis.json");
    if fs::read_to_string(&path).ok().as_deref() != Some(GENESIS) {
        fs::write(&path, GENESIS).map_err(|error| error.to_string())?;
    }
    Ok(path)
}

fn head_file(genesis: &Path) -> PathBuf {
    let mut name = genesis.as_os_str().to_owned();
    name.push(".head");
    PathBuf::from(name)
}

/// A client caught up with the network's checkpoints, starting from
/// wherever this computer last followed it to.
fn connect(genesis_file: &Path) -> Result<Client, String> {
    let genesis =
        load_genesis(genesis_file).map_err(|error| format!("could not read the network file: {error}"))?;
    let client = match Head::load(&head_file(genesis_file), &genesis) {
        Some(head) => Client::resume(&genesis, head),
        None => Client::new(&genesis),
    };
    if client.follow() > 0 {
        let _ = client.head().save(&head_file(genesis_file));
    }
    Ok(client)
}

/// The wallet's address and how many of its payments it knows are final,
/// read without its password.
fn peek(wallet_file: &Path) -> Result<(AccountId, u64), String> {
    Wallet::peek(wallet_file).map_err(|error| format!("could not read the wallet: {error}"))
}

fn overview(wallet_file: &Path, genesis_file: &Path) -> Result<(Overview, Client, AccountId), String> {
    let (address, next_seq) = peek(wallet_file)?;
    let client = connect(genesis_file)?;

    let status = client.account(&address, next_seq);
    let answered = status.views.iter().flatten().count();
    if answered == 0 {
        return Err("no validator could be reached".into());
    }
    let view = Overview {
        address: address.to_string(),
        balance: status.spendable.map(money::show),
        most: status.spendable.map(|balance| orien_ledger::coins(money::most(&money::fees(GENESIS), balance) as i128)),
        units: status.spendable,
        payments: status.seq,
        locked: has_password(wallet_file) && biometric::unlocked(wallet_file).is_none(),
        biometric: biometric::enabled(wallet_file),
        unprotected: !has_password(wallet_file),
        biometric_available: biometric::available(),
        answered,
        validators: client.validators(),
        epoch: client.epoch(),
    };
    Ok((view, client, address))
}

/// What the validators' history says came into the account since the app
/// last asked, the epoch under way, and the closed epochs history has not
/// recorded yet. None if no validator keeps history.
fn history(
    client: &Client,
    dir: &Path,
    address: &AccountId,
) -> Option<(u64, Vec<u64>, Vec<orien_net::history::Received>)> {
    // A wallet new to this app (or restored) is read from the start, so its
    // past shows too; after that, from where the app left off.
    let mut from = activity::history_from(dir, &address.to_string()).unwrap_or(0);
    let mut received = Vec::new();
    let mut unknown = Vec::new();
    // What the account received and sent, from validators that record what
    // it sent (1.7.0 on); else what it received.
    let uses_activity = client.activity(address, from, 1).is_some();
    // Pages are whole epochs; a long absence takes a few of them.
    for _ in 0..200 {
        let page = if uses_activity {
            client.activity(address, from, orien_net::history::MAX_ENTRIES)?
        } else {
            client.history(address, from, orien_net::history::MAX_ENTRIES)?
        };
        received.extend(page.entries);
        unknown.extend(page.unknown);
        match page.next {
            Some(next) if next > from => from = next,
            _ => return Some((page.epoch, unknown, received)),
        }
    }
    None
}

/// Everything the validators' history holds for the account, from the
/// first epoch it was kept. None if it can't all be read now.
fn whole_history(client: &Client, address: &AccountId) -> Option<Vec<orien_net::history::Received>> {
    let mut from = client.history(address, 0, 1)?.since;
    let mut all = Vec::new();
    for _ in 0..200 {
        let page = client.history(address, from, orien_net::history::MAX_ENTRIES)?;
        all.extend(page.entries);
        match page.next {
            Some(next) if next > from => from = next,
            _ => return Some(all),
        }
    }
    None
}

/// Everything the validators' activity holds for the account, sent and
/// received, from the start. None if it can't all be read now.
fn whole_activity(client: &Client, address: &AccountId) -> Option<Vec<orien_net::history::Received>> {
    let mut from = 0;
    let mut all = Vec::new();
    for _ in 0..200 {
        let page = client.activity(address, from, orien_net::history::MAX_ENTRIES)?;
        all.extend(page.entries);
        match page.next {
            Some(next) if next > from => from = next,
            _ => return Some(all),
        }
    }
    None
}

/// Whether the app has a wallet yet.
#[tauri::command]
fn has_wallet(app: AppHandle) -> Result<bool, String> {
    Ok(wallet_file(&folder(&app)?).is_some())
}

/// Makes the folder the shared wallet lives in, readable by this user only.
fn make_home() -> Result<PathBuf, String> {
    let path = shared_wallet();
    if path.exists() {
        return Err(format!("{} already exists, and the app never overwrites a wallet", path.display()));
    }
    let home = orien_home();
    fs::create_dir_all(&home).map_err(|error| format!("cannot create {}: {error}", home.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&home, fs::Permissions::from_mode(0o700));
    }
    Ok(path)
}

/// Uses the shared wallet from now on, forgetting any other file chosen.
fn use_shared(app: &AppHandle) -> Result<(), String> {
    let dir = folder(app)?;
    let mut settings = settings(&dir);
    settings.wallet = None;
    settings.current = Some(shared_wallet());
    save_settings(&dir, &settings)
}

/// Makes a new wallet, as `orien wallet new` does, and returns its 24-word
/// recovery phrase for the person to write down.
#[tauri::command]
async fn create_wallet(app: AppHandle) -> Result<Vec<String>, String> {
    let path = make_home()?;
    let words = tauri::async_runtime::spawn_blocking(move || {
        let wallet = Wallet::create(&path).map_err(|error| error.to_string())?;
        Ok::<_, String>(wallet.phrase().unwrap_or_default().into_iter().map(str::to_string).collect())
    })
    .await
    .map_err(|error| error.to_string())??;
    use_shared(&app)?;
    Ok(words)
}

/// Remakes a wallet from its 24-word recovery phrase, as `orien wallet
/// restore` does: the same wallet, address and ORN as on any other computer.
#[tauri::command]
async fn restore_wallet(app: AppHandle, words: String) -> Result<(), String> {
    let master = orien_net::phrase::from_words(&words).map_err(|error| {
        let text = error.to_string();
        let mut chars = text.chars();
        chars.next().map(|first| first.to_uppercase().chain(chars).collect::<String>() + ".").unwrap_or(text)
    })?;
    let path = make_home()?;
    tauri::async_runtime::spawn_blocking(move || Wallet::from_master(&path, master, 1).map_err(|error| error.to_string()))
        .await
        .map_err(|error| error.to_string())??;
    use_shared(&app)
}

/// Whether `words` are a whole, valid recovery phrase, said plainly if not.
#[tauri::command]
fn phrase_check(words: String) -> Result<(), String> {
    orien_net::phrase::from_words(&words).map(|_| ()).map_err(|error| {
        let text = error.to_string();
        let mut chars = text.chars();
        chars.next().map(|first| first.to_uppercase().chain(chars).collect::<String>() + ".").unwrap_or(text)
    })
}

/// Puts a password on the wallet, which then has to be typed to send.
#[tauri::command]
async fn set_password(app: AppHandle, password: String) -> Result<(), String> {
    if password.chars().count() < 8 {
        return Err("Use at least 8 characters.".into());
    }
    let dir = folder(&app)?;
    let path = wallet_file(&dir).ok_or("no wallet yet")?;
    tauri::async_runtime::spawn_blocking(move || {
        // Before it's sealed: the vault keys, kept for syncing contacts.
        let _ = vault::keys(&dir, &path, None);
        let mut wallet = Wallet::load(&path).map_err(|error| error.to_string())?;
        wallet.set_password(Some(&password)).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Asks for Touch ID or Face ID, and unlocks the open wallet for a few
/// minutes.
#[tauri::command]
async fn biometric_unlock(app: AppHandle, path: Option<String>) -> Result<(), String> {
    // The open wallet, or another of the app's, such as the one a taken
    // swap offer sells from.
    let wallet = match path {
        Some(path) => known_wallets(&folder(&app)?).into_iter().map(|w| w.path).find(|known| known.display().to_string() == path).ok_or("That wallet is not in this app.")?,
        None => wallet_file(&folder(&app)?).ok_or("no wallet yet")?,
    };
    let dir = folder(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        biometric::unlock(&wallet)?;
        // Unlocked: work out its vault keys too, if not kept yet, so its
        // contacts and wallet icons sync from now on without asking.
        if let Some(password) = biometric::unlocked(&wallet) {
            let _ = vault::keys(&dir, &wallet, Some(&password));
        }
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Protects the open wallet with Touch ID or Face ID instead of a typed
/// password: with `password`, the one it already has (checked first);
/// without, a new random one it's sealed with, kept only in the Keychain.
#[tauri::command]
async fn biometric_enable(app: AppHandle, password: Option<String>) -> Result<(), String> {
    let dir = folder(&app)?;
    let wallet = wallet_file(&dir).ok_or("no wallet yet")?;
    tauri::async_runtime::spawn_blocking(move || {
        match password.filter(|p| !p.is_empty()) {
            Some(password) => {
                Wallet::open(&wallet, Some(&password)).map_err(|_| "That is not this wallet's password.".to_string())?;
                biometric::remember(&wallet, &password)
            }
            None => {
                let password = biometric::new_password();
                // Kept before it's sealed, so the wallet can't end up sealed
                // with a password nobody has.
                biometric::remember(&wallet, &password)?;
                let _ = vault::keys(&dir, &wallet, None);
                let mut opened = Wallet::load(&wallet).map_err(|error| error.to_string())?;
                opened.set_password(Some(&password)).map_err(|error| error.to_string())
            }
        }
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Saves a new wallet's recovery phrase as a text file where the person
/// chooses: through the save window on a computer, or the Files app on an
/// iPhone. Returns false if they closed it.
#[tauri::command]
async fn save_phrase(app: AppHandle, words: Vec<String>) -> Result<bool, String> {
    if words.len() != 24 {
        return Err("There is no recovery phrase to save.".into());
    }
    let mut text = String::from("Orien Wallet recovery phrase\n\n");
    for (n, word) in words.iter().enumerate() {
        text.push_str(&format!("{:>2}. {word}\n", n + 1));
    }
    text.push_str("\nAnyone with these 24 words can spend this wallet's ORN. Keep this file private.\n");
    let name = "Orien recovery phrase.txt";
    tauri::async_runtime::spawn_blocking(move || {
        // An iPhone can only hand a finished file to the Files app: written
        // first to the app's own documents, then removed once handed over.
        #[cfg(target_os = "ios")]
        {
            let documents = PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Documents");
            let draft = documents.join(name);
            fs::create_dir_all(&documents).map_err(|error| error.to_string())?;
            fs::write(&draft, &text).map_err(|error| error.to_string())?;
            let saved = app.dialog().file().set_file_name(name).blocking_save_file().is_some();
            let _ = fs::remove_file(&draft);
            Ok(saved)
        }
        #[cfg(not(target_os = "ios"))]
        {
            let Some(path) = app.dialog().file().set_file_name(name).add_filter("Text", &["txt"]).blocking_save_file() else {
                return Ok(false);
            };
            let path = path.into_path().map_err(|error| error.to_string())?;
            orien_net::store::write_replacing(&path, text.as_bytes(), true).map_err(|error| format!("could not save it: {error}"))?;
            Ok(true)
        }
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Lets the person pick a wallet file they already have. Returns false if
/// they closed the chooser.
#[tauri::command]
async fn choose_wallet(app: AppHandle) -> Result<bool, String> {
    let picked = app.dialog().file().add_filter("Orien wallet", &["wallet"]).blocking_pick_file();
    let Some(path) = picked.and_then(|file| file.into_path().ok()) else { return Ok(false) };
    Wallet::peek(&path).map_err(|_| "that file is not an Orien wallet".to_string())?;
    let dir = folder(&app)?;
    let mut settings = settings(&dir);
    settings.wallet = Some(path.clone());
    settings.network = network_id();
    settings.current = Some(path);
    save_settings(&dir, &settings)?;
    Ok(true)
}

/// A wallet as the switcher shows it.
#[derive(Serialize)]
struct WalletView {
    path: String,
    name: String,
    address: String,
    current: bool,
    /// The one the `orien` command uses, which stays first and can't be
    /// forgotten here.
    shared: bool,
}

/// Every wallet the app knows.
#[tauri::command]
fn wallets(app: AppHandle) -> Result<Vec<WalletView>, String> {
    let dir = folder(&app)?;
    let current = wallet_file(&dir);
    Ok(known_wallets(&dir)
        .into_iter()
        .filter_map(|w| {
            let address = Wallet::peek(&w.path).ok()?.0.to_string();
            Some(WalletView {
                current: Some(&w.path) == current.as_ref(),
                shared: w.path == shared_wallet(),
                path: w.path.display().to_string(),
                name: w.name,
                address,
            })
        })
        .collect())
}

/// Each known wallet's balance, by address, as the validators report it.
#[tauri::command]
async fn wallet_balances(app: AppHandle) -> Result<std::collections::BTreeMap<String, String>, String> {
    let dir = folder(&app)?;
    let genesis = genesis_file(&dir)?;
    let addresses: Vec<AccountId> = known_wallets(&dir).iter().filter_map(|w| Wallet::peek(&w.path).ok().map(|(a, _)| a)).collect();
    tauri::async_runtime::spawn_blocking(move || {
        let client = connect(&genesis)?;
        let balances = addresses
            .iter()
            .filter_map(|address| client.account(address, 0).spendable.map(|units| (address.to_string(), money::show(units))))
            .collect();
        let _ = client.head().save(&head_file(&genesis));
        Ok(balances)
    })
    .await
    .map_err(|error| error.to_string())?
}

fn known(dir: &Path, path: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(path);
    known_wallets(dir).iter().any(|w| w.path == path).then_some(path).ok_or_else(|| "That wallet is not in this app.".into())
}

/// Opens another of the app's wallets.
#[tauri::command]
fn use_wallet(app: AppHandle, path: String) -> Result<(), String> {
    let dir = folder(&app)?;
    let path = known(&dir, &path)?;
    let mut settings = settings(&dir);
    settings.current = Some(path);
    save_settings(&dir, &settings)
}

/// Gives a wallet a short name of the person's own.
#[tauri::command]
fn rename_wallet(app: AppHandle, path: String, name: String) -> Result<(), String> {
    let dir = folder(&app)?;
    let path = known(&dir, &path)?;
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 24 {
        return Err("A wallet's name is 1 to 24 characters.".into());
    }
    let mut settings = settings(&dir);
    match settings.wallets.iter_mut().find(|w| w.path == path) {
        Some(named) => named.name = name.to_string(),
        None => settings.wallets.push(Named { path, name: name.to_string() }),
    }
    save_settings(&dir, &settings)
}

/// The folder, readable by this user only, that wallets added in the app
/// live in.
fn wallets_folder(dir: &Path) -> Result<PathBuf, String> {
    let folder = dir.join("wallets");
    fs::create_dir_all(&folder).map_err(|error| format!("cannot create {}: {error}", folder.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o700)).map_err(|error| error.to_string())?;
    }
    Ok(folder)
}

/// Adds a wallet: a new one, whose 24 words it returns, or one rebuilt
/// from `words`. It opens at once. Never overwrites a file.
#[tauri::command]
async fn add_wallet(app: AppHandle, name: String, words: Option<String>) -> Result<Vec<String>, String> {
    let dir = folder(&app)?;
    let name = name.trim().to_string();
    if name.is_empty() || name.chars().count() > 24 {
        return Err("Give the wallet a name of 1 to 24 characters.".into());
    }
    let master = match words.as_deref().map(str::trim).filter(|w| !w.is_empty()) {
        Some(words) => Some(orien_net::phrase::from_words(words).map_err(|error| {
            let text = error.to_string();
            let mut chars = text.chars();
            chars.next().map(|first| first.to_uppercase().chain(chars).collect::<String>() + ".").unwrap_or(text)
        })?),
        None => None,
    };
    let folder = wallets_folder(&dir)?;
    let path = (1..10_000).map(|n| folder.join(format!("wallet-{n}.wallet"))).find(|p| !p.exists()).ok_or("too many wallets")?;
    let made = path.clone();
    let words = tauri::async_runtime::spawn_blocking(move || match master {
        Some(master) => {
            let wallet = Wallet::from_master(&made, master, 1).map_err(|error| error.to_string())?;
            let _ = wallet;
            Ok::<_, String>(Vec::new())
        }
        None => {
            let wallet = Wallet::create(&made).map_err(|error| error.to_string())?;
            Ok(wallet.phrase().unwrap_or_default().into_iter().map(str::to_string).collect())
        }
    })
    .await
    .map_err(|error| error.to_string())??;
    // The same wallet twice would sign with the same one-time keys from two
    // files: a restored wallet already here is refused, and its copy removed.
    let address = Wallet::peek(&path).map_err(|error| error.to_string())?.0;
    if known_wallets(&dir).iter().any(|w| Wallet::peek(&w.path).is_ok_and(|(a, _)| a == address)) {
        let _ = fs::remove_file(&path);
        return Err("That wallet is already in this app.".into());
    }
    let mut settings = settings(&dir);
    settings.wallets.push(Named { path: path.clone(), name });
    settings.current = Some(path);
    save_settings(&dir, &settings)?;
    Ok(words)
}

/// Takes a wallet off the app's list. Its file stays where it is: this only
/// forgets it. The shared one can't be forgotten here.
#[tauri::command]
fn forget_wallet(app: AppHandle, path: String) -> Result<(), String> {
    let dir = folder(&app)?;
    let path = known(&dir, &path)?;
    if path == shared_wallet() {
        return Err("The orien command uses this wallet, so it stays.".into());
    }
    biometric::forget(&path);
    vault::forget(&dir, &path);
    let mut settings = settings(&dir);
    settings.wallets.retain(|w| w.path != path);
    if settings.wallet.as_ref() == Some(&path) {
        settings.wallet = None;
    }
    if settings.current.as_ref() == Some(&path) {
        settings.current = None;
    }
    save_settings(&dir, &settings)
}

/// Mining, on desktops only.
#[cfg(desktop)]
mod mine_commands {
    use super::*;

/// The threads mining uses: as set, or the old percent turned into threads.
fn mine_threads(saved: &Settings) -> usize {
    let cores = orien_net::miner::cores();
    saved.mining_threads.unwrap_or_else(|| orien_net::miner::threads_for(saved.mining_percent.unwrap_or(50))).clamp(1, cores)
}

/// Starts mining for the open wallet on `threads` threads, and remembers
/// them.
#[tauri::command]
pub fn mine_start(app: AppHandle, threads: usize) -> Result<(), String> {
    let dir = folder(&app)?;
    let wallet = wallet_file(&dir).ok_or("no wallet yet")?;
    let account = Wallet::peek(&wallet).map_err(|error| error.to_string())?.0;
    let genesis = genesis_file(&dir)?;
    let mut saved = settings(&dir);
    saved.mining_threads = Some(threads.max(1));
    save_settings(&dir, &saved)?;
    let file = genesis.clone();
    mining::start(account, threads, move || connect(&file), genesis);
    Ok(())
}

/// Notes that mining is running, just before an update restarts the app,
/// so the new copy starts it again. Quitting or stopping never sets this.
pub fn remember_for_update(app: &AppHandle) {
    let status = mining::status();
    if !status.running {
        return;
    }
    let Ok(dir) = folder(app) else { return };
    let mut saved = settings(&dir);
    saved.resume_mining = Some(status.threads.max(1));
    let _ = save_settings(&dir, &saved);
}

/// An update that didn't install: nothing to start again.
pub fn forget_resume(app: &AppHandle) {
    let Ok(dir) = folder(app) else { return };
    let mut saved = settings(&dir);
    if saved.resume_mining.take().is_some() {
        let _ = save_settings(&dir, &saved);
    }
}

/// Starts mining again if an update stopped it, on the same threads, once.
/// True when it did.
#[tauri::command]
pub fn mine_resume(app: AppHandle) -> Result<bool, String> {
    let dir = folder(&app)?;
    let mut saved = settings(&dir);
    let Some(threads) = saved.resume_mining.take() else { return Ok(false) };
    save_settings(&dir, &saved)?;
    if mining::status().running {
        return Ok(false);
    }
    mine_start(app, threads)?;
    Ok(true)
}

#[tauri::command]
pub async fn mine_stop() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(mining::stop).await.map_err(|error| error.to_string())
}

/// Changes how many threads mine while it runs, without restarting it,
/// and remembers them.
#[tauri::command]
pub fn mine_set(app: AppHandle, threads: usize) -> Result<bool, String> {
    let dir = folder(&app)?;
    let mut saved = settings(&dir);
    saved.mining_threads = Some(threads.max(1));
    save_settings(&dir, &saved)?;
    Ok(mining::set_threads(threads))
}

/// Finds this computer's sweet spot while mining runs, then mines there
/// and remembers it. False if mining isn't running.
#[tauri::command]
pub fn mine_benchmark(app: AppHandle) -> Result<bool, String> {
    let dir = folder(&app)?;
    let back = mine_threads(&settings(&dir));
    Ok(mining::benchmark(back, move |sweet| {
        let mut saved = settings(&dir);
        saved.mining_sweet = Some(sweet);
        saved.mining_threads = Some(sweet.threads);
        let _ = save_settings(&dir, &saved);
    }))
}

#[tauri::command]
pub fn mine_benchmark_cancel() {
    mining::cancel_benchmark();
}

/// Whether mining can use large pages here, and whether they're set up.
#[tauri::command]
pub fn large_pages_state(app: AppHandle) -> Result<large_pages::State, String> {
    Ok(large_pages::state(settings(&folder(&app)?).large_pages_granted))
}

/// Sets this computer up for large pages, with an administrator's approval.
#[tauri::command]
pub async fn large_pages_set_up(app: AppHandle) -> Result<large_pages::State, String> {
    let dir = folder(&app)?;
    tauri::async_runtime::spawn_blocking(large_pages::set_up).await.map_err(|error| error.to_string())??;
    let mut saved = settings(&dir);
    saved.large_pages_granted = true;
    save_settings(&dir, &saved)?;
    Ok(large_pages::state(true))
}

/// How mining is going, with the remembered percent, which wallet it mines
/// to, and the last mining reward that wallet received.
#[tauri::command]
pub fn mine_status(app: AppHandle) -> Result<serde_json::Value, String> {
    let dir = folder(&app)?;
    let status = mining::status();
    let open = wallet_file(&dir).and_then(|w| Wallet::peek(&w).ok()).map(|(a, _)| a.to_string());
    let address = status.address.clone().or(open);
    let name = address.as_ref().and_then(|address| {
        known_wallets(&dir).into_iter().find(|w| Wallet::peek(&w.path).is_ok_and(|(a, _)| &a.to_string() == address)).map(|w| w.name)
    });
    let reward = address.as_ref().and_then(|address| {
        activity::list(&dir, address).into_iter().find(|e| e.kind == Some(activity::Kind::Mining)).map(|e| {
            serde_json::json!({ "amount": money::show(e.amount as i128), "time": e.time })
        })
    });
    Ok(serde_json::json!({
        "status": status,
        "threads": mine_threads(&settings(&dir)),
        "sweet": settings(&dir).mining_sweet.filter(|s| s.cores == orien_net::miner::cores()),
        "wallet": name,
        "last_reward": reward,
    }))
}
}
/// No mining on iPhones: Apple's rules forbid it, and RandomX needs 2 GB.
#[cfg(mobile)]
mod mine_commands {
    #[tauri::command]
    pub fn mine_start() -> Result<(), String> {
        Err("Mining is on the desktop app and the orien command.".into())
    }
    #[tauri::command]
    pub async fn mine_stop() -> Result<(), String> {
        Ok(())
    }
    #[tauri::command]
    pub fn mine_resume() -> Result<bool, String> {
        Ok(false)
    }
    #[tauri::command]
    pub fn mine_set() -> Result<bool, String> {
        Ok(false)
    }
    #[tauri::command]
    pub fn mine_benchmark() -> Result<bool, String> {
        Ok(false)
    }
    #[tauri::command]
    pub fn mine_benchmark_cancel() {}
    #[tauri::command]
    pub fn large_pages_state() -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({ "supported": false }))
    }
    #[tauri::command]
    pub async fn large_pages_set_up() -> Result<(), String> {
        Err("Mining is on the desktop app.".into())
    }
    #[tauri::command]
    pub fn mine_status() -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({ "status": { "running": false, "cores": 0 }, "threads": 1, "unavailable": true }))
    }
}
use mine_commands::{large_pages_set_up, large_pages_state, mine_benchmark, mine_benchmark_cancel, mine_resume, mine_set, mine_start, mine_status, mine_stop};

/// Asks the validators about the wallet's account.
#[tauri::command]
async fn overview_now(app: AppHandle) -> Result<Overview, String> {
    let dir = folder(&app)?;
    let wallet = wallet_file(&dir).ok_or("no wallet yet")?;
    let genesis = genesis_file(&dir)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _turn = TURNS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (view, client, address) = overview(&wallet, &genesis)?;
        if let Some(balance) = view.units {
            // History first, so the balance is only asked to explain what
            // history did not.
            let fees = money::fees(GENESIS);
            let fee_of = |amount: u64| fees.on(&orien_ledger::Kind::Payment, amount);
            if let Some((epoch, unknown, received)) = history(&client, &dir, &address) {
                activity::heard(&dir, &view.address, epoch, &unknown, received, &fee_of);
            }
            // Once: a record started before 1.6.0 gets the wallet's past.
            if activity::needs_past(&dir, &view.address)
                && let Some(all) = whole_activity(&client, &address)
            {
                activity::past(&dir, &view.address, all, &fee_of);
            }
            // Once: lines worked out from the balance before history was read
            // for their hour take what history says they were.
            if activity::needs_repair(&dir, &view.address)
                && let Some(all) = whole_history(&client, &address)
            {
                activity::repair(&dir, &view.address, all);
            }
            activity::observe(&dir, &view.address, balance);
        }
        *LAST_BALANCE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = view.units;
        #[cfg(desktop)]
        extension::share(&view.address, view.balance.as_deref(), view.most.as_deref());
        Ok(view)
    })
    .await
    .map_err(|error| error.to_string())?
}

/// The address book, sorted by name.
#[tauri::command]
fn contacts(app: AppHandle) -> Result<Vec<contacts::Contact>, String> {
    Ok(contacts::list(&folder(&app)?))
}

/// Adds a contact, or edits the one at `previous`.
#[tauri::command]
fn save_contact(app: AppHandle, name: String, address: String, previous: Option<String>) -> Result<(), String> {
    let dir = folder(&app)?;
    contacts::save(&dir, &name, &address, previous.as_deref())?;
    sync_contacts_later(dir);
    Ok(())
}

/// Orien Network members saved as contacts.
#[tauri::command]
fn members(app: AppHandle) -> Result<Vec<contacts::Member>, String> {
    Ok(contacts::members(&folder(&app)?))
}

/// Saves a member as a contact.
#[tauri::command]
fn save_member(app: AppHandle, id: i64, name: String) -> Result<(), String> {
    let dir = folder(&app)?;
    contacts::save_member(&dir, id, &name)?;
    sync_contacts_later(dir);
    Ok(())
}

/// Removes a member from the contacts.
#[tauri::command]
fn delete_member(app: AppHandle, id: i64) -> Result<(), String> {
    let dir = folder(&app)?;
    contacts::delete_member(&dir, id)?;
    sync_contacts_later(dir);
    Ok(())
}

/// Puts the changed address book in the vault, in the background.
fn sync_contacts_later(dir: PathBuf) {
    std::thread::spawn(move || {
        let _ = vault::sync_all(&dir, wallet_file(&dir).as_deref());
    });
}

/// Brings the address book into step with the open wallet's vault, so
/// contacts added on another device show here. False when the vault keys
/// aren't known yet: a wallet with a password works them out the first time
/// its password is typed in this app.
#[tauri::command]
async fn contacts_sync(app: AppHandle) -> Result<bool, String> {
    let dir = folder(&app)?;
    let wallet = wallet_file(&dir).ok_or("no wallet yet")?;
    tauri::async_runtime::spawn_blocking(move || vault::sync_all(&dir, Some(&wallet)))
    .await
    .map_err(|error| error.to_string())?
}

/// The wallets' icons, by address: shared across devices with the contacts.
#[tauri::command]
fn wallet_icons(app: AppHandle) -> Result<std::collections::BTreeMap<String, String>, String> {
    let dir = folder(&app)?;
    Ok(contacts::icons(&dir).into_iter().map(|(address, icon)| (address, icon.emoji)).collect())
}

/// The app's theme, as last chosen on any of the person's devices.
#[tauri::command]
fn app_theme(app: AppHandle) -> Result<String, String> {
    let dir = folder(&app)?;
    Ok(contacts::prefs(&dir).get("theme").map(|pref| pref.value.clone()).unwrap_or_default())
}

/// Sets the app's theme and shares it with the person's other devices.
#[tauri::command]
fn set_app_theme(app: AppHandle, theme: String) -> Result<(), String> {
    let dir = folder(&app)?;
    contacts::set_pref(&dir, "theme", &theme)?;
    sync_contacts_later(dir);
    Ok(())
}

/// The person's choices that follow them to every device: the theme, hidden
/// balances, shorter balances.
#[tauri::command]
fn app_prefs(app: AppHandle) -> Result<std::collections::BTreeMap<String, String>, String> {
    let dir = folder(&app)?;
    Ok(contacts::prefs(&dir).into_iter().map(|(name, pref)| (name, pref.value)).collect())
}

#[tauri::command]
fn set_app_pref(app: AppHandle, name: String, value: String) -> Result<(), String> {
    if !["theme", "themes", "hide", "short", "arrow"].contains(&name.as_str()) {
        return Err("unknown setting".into());
    }
    let dir = folder(&app)?;
    contacts::set_pref(&dir, &name, &value)?;
    sync_contacts_later(dir);
    Ok(())
}

/// The kinds of notification this device has turned off: "received",
/// "mining", "market", "swaps". Kept on this device only.
pub(crate) fn muted(dir: &Path) -> Vec<String> {
    fs::read(dir.join("notifications-off.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

#[tauri::command]
fn notifications_muted(app: AppHandle) -> Result<Vec<String>, String> {
    Ok(muted(&folder(&app)?))
}

/// Turns one kind of notification on or off on this device (and tells the
/// server, which sends an iPhone's).
#[tauri::command]
fn set_notification(app: AppHandle, kind: String, on: bool) -> Result<(), String> {
    if !["received", "mining", "market", "community", "swaps"].contains(&kind.as_str()) {
        return Err("unknown notification".into());
    }
    let dir = folder(&app)?;
    let mut off = muted(&dir);
    off.retain(|k| *k != kind);
    if !on {
        off.push(kind);
    }
    off.sort();
    fs::write(dir.join("notifications-off.json"), serde_json::to_vec(&off).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    #[cfg(target_os = "ios")]
    std::thread::spawn(push::sync);
    Ok(())
}

/// Asks for Touch ID or Face ID to open the app, when the person turned
/// that on.
#[tauri::command]
async fn app_unlock() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(biometric::check).await.map_err(|error| error.to_string())?
}

/// This computer's own settings: opening at login, whether closing the
/// window keeps the wallet running, and whether updates install by
/// themselves. None on iPhones.
#[tauri::command]
fn computer_settings(app: AppHandle) -> Option<(bool, bool, bool)> {
    #[cfg(desktop)]
    return Some((background::opens_at_login(&app), background::keep_running(), background::updates_itself(&app)));
    #[cfg(not(desktop))]
    {
        let _ = app;
        None
    }
}

#[tauri::command]
fn set_computer_setting(app: AppHandle, name: String, on: bool) -> Result<(), String> {
    #[cfg(desktop)]
    match name.as_str() {
        "login" => background::set_opens_at_login(&app, on),
        "running" => {
            background::set_keep_running(&app, on);
            Ok(())
        }
        "updates" => background::set_updates_itself(&app, on),
        _ => Err("unknown setting".into()),
    }
    #[cfg(not(desktop))]
    {
        let _ = (app, name, on);
        Err("Not on this device.".into())
    }
}

/// The colour behind the app's page, so its edges and the moments before
/// it draws match the theme.
#[tauri::command]
fn set_backdrop(app: AppHandle, red: u8, green: u8, blue: u8, light: Option<bool>) {
    let light = light.unwrap_or(false);
    #[cfg(desktop)]
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_background_color(Some(tauri::window::Color(red, green, blue, 255)));
        let _ = window.set_theme(Some(if light { tauri::Theme::Light } else { tauri::Theme::Dark }));
    }
    #[cfg(target_os = "ios")]
    let _ = app.run_on_main_thread(move || keyboard::backdrop(red, green, blue, light));
    #[cfg(not(any(desktop, target_os = "ios")))]
    let _ = (app, red, green, blue, light);
}

/// Sets a wallet's icon and shares it with this wallet's other devices.
#[tauri::command]
fn set_wallet_icon(app: AppHandle, address: String, emoji: String) -> Result<(), String> {
    let dir = folder(&app)?;
    contacts::set_icon(&dir, &address, &emoji)?;
    sync_contacts_later(dir);
    Ok(())
}

/// Whether contacts and wallet icons can sync on this device: the open
/// wallet's vault keys are known (they need the wallet unlocked once).
#[tauri::command]
fn vault_ready(app: AppHandle) -> Result<bool, String> {
    let dir = folder(&app)?;
    let Some(wallet) = wallet_file(&dir) else { return Ok(false) };
    Ok(vault::keys(&dir, &wallet, None).is_some())
}

/// Unlocks the open wallet once, with its password or Touch ID / Face ID
/// a moment ago, to work out its vault keys, then syncs. After this,
/// contacts and wallet icons sync by themselves on this device.
#[tauri::command]
async fn vault_unlock(app: AppHandle, password: Option<String>) -> Result<bool, String> {
    let dir = folder(&app)?;
    let wallet = wallet_file(&dir).ok_or("no wallet yet")?;
    tauri::async_runtime::spawn_blocking(move || {
        let password = password.filter(|p| !p.is_empty()).or_else(|| biometric::unlocked(&wallet));
        if vault::keys(&dir, &wallet, password.as_deref()).is_none() {
            return Err(if has_password(&wallet) { "That is not this wallet's password.".to_string() } else { "This wallet can't be opened.".to_string() });
        }
        vault::sync_all(&dir, Some(&wallet))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
fn delete_contact(app: AppHandle, address: String) -> Result<(), String> {
    let dir = folder(&app)?;
    contacts::delete(&dir, &address)?;
    sync_contacts_later(dir);
    Ok(())
}

/// One payment as the window shows it.
#[derive(Serialize)]
struct ActivityView {
    direction: activity::Direction,
    kind: Option<activity::Kind>,
    amount: String,
    fee: Option<String>,
    counterparty: Option<String>,
    time: u64,
}

/// The wallet's payments, newest first.
#[tauri::command]
fn activity(app: AppHandle) -> Result<Vec<ActivityView>, String> {
    let dir = folder(&app)?;
    let (address, _) = peek(&wallet_file(&dir).ok_or("no wallet yet")?)?;
    Ok(activity::list(&dir, &address.to_string())
        .into_iter()
        .map(|entry| ActivityView {
            direction: entry.direction,
            kind: entry.kind,
            amount: money::show(entry.amount as i128),
            fee: (entry.fee > 0).then(|| money::show(entry.fee as i128)),
            counterparty: entry.counterparty,
            time: entry.time,
        })
        .collect())
}

/// The fee and the balance afterwards for an amount being typed.
#[tauri::command]
fn quote(amount: String) -> money::Quote {
    let balance = *LAST_BALANCE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    money::quote(&money::fees(GENESIS), balance, &amount)
}

/// Sends `amount` to the address `to` and waits until it is final.
#[tauri::command]
async fn send_payment(app: AppHandle, to: String, amount: String, password: Option<String>) -> Result<send::Sent, String> {
    let to: AccountId = to.trim().parse().map_err(|_| "That is not an Orien address. An address is 64 letters and digits.".to_string())?;
    let amount = money::read(&amount)?;
    let dir = folder(&app)?;
    let wallet_file = wallet_file(&dir).ok_or("no wallet yet")?;
    let genesis = genesis_file(&dir)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _turn = TURNS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut wallet = open_signing(&wallet_file, password.as_deref())?;
        let _ = vault::keys(&dir, &wallet_file, password.as_deref());
        if to == wallet.address() {
            return Err("That is this wallet's own address.".into());
        }
        let client = connect(&genesis)?;
        let sent = send::send(&client, &mut wallet, money::fees(GENESIS), to, amount);
        let _ = client.head().save(&head_file(&genesis));
        if let Ok(sent) = &sent {
            activity::sent(&dir, &wallet.address().to_string(), sent.amount, sent.fee, &sent.to, Some(sent.seq));
        }
        sent
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Swaps run with the app's client and wallet.
fn with_swaps<T: Send + 'static>(
    app: &AppHandle,
    password: Option<String>,
    work: impl FnOnce(&Path, &swaps::AppOrien) -> Result<T, String> + Send + 'static,
) -> tauri::async_runtime::JoinHandle<Result<T, String>> {
    let dir = folder(app);
    tauri::async_runtime::spawn_blocking(move || {
        let dir = dir?;
        let wallet_file = wallet_file(&dir).ok_or("no wallet yet")?;
        let genesis = genesis_file(&dir)?;
        let _turn = TURNS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let client = connect(&genesis)?;
        let orien = swaps::AppOrien {
            client: &client,
            wallet_file,
            password,
            fees: money::fees(GENESIS),
            network: network_id().unwrap_or_default(),
            epoch_seconds: serde_json::from_str::<orien_ledger::Genesis>(GENESIS).map_or(3_600, |genesis| genesis.epoch_seconds),
        };
        let result = work(&dir, &orien);
        let _ = client.head().save(&head_file(&genesis));
        result
    })
}

fn views(swaps: &[swaps::Kept]) -> Vec<swaps::View> {
    let mut views: Vec<swaps::View> = swaps.iter().map(swaps::view).collect();
    views.sort_by_key(|view| std::cmp::Reverse(view.created));
    views
}

/// Every swap this wallet has made, newest first.
#[tauri::command]
fn swap_list(app: AppHandle) -> Result<Vec<swaps::View>, String> {
    Ok(views(&swaps::load(&folder(&app)?)))
}

/// Sells `orn` ORN for `btc` Bitcoin to the Orien address `buyer`: locks the
/// ORN for them and makes the offer code.
#[tauri::command]
async fn swap_sell(app: AppHandle, buyer: String, orn: String, btc: String, payout: String, password: Option<String>) -> Result<swaps::View, String> {
    let orn = money::read(&orn)?;
    let sats = swaps::read_btc(&btc)?;
    with_swaps(&app, password, move |dir, orien| {
        // Saved with its secret before anything is signed; then the ORN are
        // locked, and the swap saved again.
        let kept = swaps::sell(orien, &buyer, orn, sats, &payout)?;
        let mut all = swaps::load(dir);
        all.push(kept);
        swaps::save(dir, &all)?;
        let (_, chain) = swaps::bitcoin();
        let kept = all.last_mut().expect("just added");
        swaps::advance(orien, &chain, kept);
        let view = swaps::view(kept);
        let problem = kept.problem.clone();
        swaps::save(dir, &all)?;
        match problem {
            Some(problem) if problem.starts_with("password:") => Err(problem),
            _ => Ok(view),
        }
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Takes up a seller's offer: checks it on Orien and makes the reply code.
#[tauri::command]
async fn swap_buy(app: AppHandle, offer: String, refund: String) -> Result<swaps::View, String> {
    with_swaps(&app, None, move |dir, orien| {
        let kept = swaps::buy(orien, &offer, &refund)?;
        let mut all = swaps::load(dir);
        if all.iter().any(|known| known.id == kept.id) {
            return Err("This offer is already taken up here.".into());
        }
        all.push(kept.clone());
        swaps::save(dir, &all)?;
        Ok(swaps::view(&kept))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// The seller pastes the buyer's reply.
#[tauri::command]
async fn swap_accept(app: AppHandle, id: String, reply: String) -> Result<swaps::View, String> {
    with_swaps(&app, None, move |dir, orien| {
        let mut all = swaps::load(dir);
        let kept = all.iter_mut().find(|kept| kept.id == id).ok_or("That swap is not here.")?;
        swaps::accept(orien, kept, &reply)?;
        let view = swaps::view(kept);
        swaps::save(dir, &all)?;
        Ok(view)
    })
    .await
    .map_err(|error| error.to_string())?
}

/// The smallest swap that can complete at Bitcoin's fee rate now, in BTC,
/// as the explorers report it.
#[tauri::command]
async fn swap_smallest() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let (_, chain) = swaps::bitcoin();
        let rate = chain.fee_rate().map_err(|error| error.to_string())?;
        Ok(swaps::show_btc(orien_swap::swap::smallest_now(rate)))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Moves every unfinished swap along, and this wallet's swaps on the swap
/// board, starting their swaps as their turn comes. The window calls this
/// every so often. Claiming ORN signs, so it may need the wallet's password.
#[tauri::command]
async fn swap_step(app: AppHandle, password: Option<String>) -> Result<serde_json::Value, String> {
    with_swaps(&app, password, move |dir, orien| {
        let mut all = swaps::load(dir);
        // The board is asked first, so a swap it starts is moved along at
        // once; if it can't be reached, the swaps here still move.
        let (mine, board_problem) = match board::drive(orien, dir, &mut all) {
            Ok(mine) => (mine, None),
            Err(error) => (serde_json::json!([]), Some(error)),
        };
        // SOL swaps, the main kind: the board's first, then each one's steps.
        let mut sol = solswaps::load(dir);
        if let Err(error) = board::drive_sol(orien, dir, &mine, &mut sol) {
            eprintln!("swap board SOL offers: {error}");
        }
        if swaps::active(&all) {
            swaps::step(orien, dir, &mut all)?;
            // A swap that just finished is marked on the board now.
            let _ = board::drive(orien, dir, &mut all);
        }
        if solswaps::active(&sol) {
            solswaps::step_all(orien, dir, &mut sol)?;
        }
        let mut sol_views: Vec<solswaps::View> = sol.iter().map(solswaps::view).collect();
        sol_views.sort_by_key(|view| std::cmp::Reverse(view.created));
        // Offers taken down because the wallet couldn't lock their ORN.
        let withdrawn: Vec<serde_json::Value> =
            board::withdrawn().into_iter().map(|(offer, orn)| serde_json::json!({ "offer": offer, "orn": money::show(orn as i128) })).collect();
        Ok(serde_json::json!({ "swaps": views(&all), "sol_swaps": sol_views, "waiting": board::waiting(dir, &mine), "problem": board_problem, "withdrawn": withdrawn }))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// The swap board's open offers, and Bitcoin's dollar price. Needs no
/// sign-in.
#[tauri::command]
async fn board_offers(app: AppHandle) -> Result<serde_json::Value, String> {
    let dir = folder(&app)?;
    tauri::async_runtime::spawn_blocking(move || board::offers(&dir)).await.map_err(|error| error.to_string())?
}

/// Who is signed in to the swap board, if anyone.
#[tauri::command]
async fn board_me(app: AppHandle) -> Result<Option<serde_json::Value>, String> {
    let dir = folder(&app)?;
    tauri::async_runtime::spawn_blocking(move || board::me(&dir)).await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn board_email_start(app: AppHandle, email: String) -> Result<(), String> {
    let dir = folder(&app)?;
    tauri::async_runtime::spawn_blocking(move || board::email_start(&dir, &email)).await.map_err(|error| error.to_string())?
}

/// Signs in with the emailed code: false when the email has no account yet
/// and a name is wanted.
#[tauri::command]
async fn board_email_finish(app: AppHandle, email: String, code: String, name: Option<String>) -> Result<bool, String> {
    let dir = folder(&app)?;
    tauri::async_runtime::spawn_blocking(move || board::email_finish(&dir, &email, &code, name.as_deref())).await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn board_sign_out(app: AppHandle) -> Result<(), String> {
    let dir = folder(&app)?;
    tauri::async_runtime::spawn_blocking(move || board::sign_out(&dir)).await.map_err(|error| error.to_string())?
}

/// Posts an offer of `orn` ORN for `btc` Bitcoin, paid to `payout`.
#[tauri::command]
async fn board_post(app: AppHandle, orn: String, btc: String, payout: String, password: Option<String>) -> Result<(), String> {
    let orn = money::read(&orn)?;
    let sats = swaps::read_btc(&btc)?;
    let dir = folder(&app)?;
    let wallet = wallet_file(&dir).ok_or("no wallet yet")?;
    tauri::async_runtime::spawn_blocking(move || {
        // Posting approves the offer: the wallet must open now, with the
        // password typed or Touch ID a moment ago, so it can lock the ORN
        // later with no one there.
        let password = password.filter(|p| !p.is_empty()).or_else(|| biometric::unlocked(&wallet));
        if has_password(&wallet) {
            Wallet::open(&wallet, password.as_deref()).map_err(|error| match error.downcast_ref::<OpenError>() {
                Some(OpenError::NeedsPassword) => "password: needed".to_string(),
                Some(OpenError::WrongPassword) => "password: wrong".to_string(),
                None => format!("could not open the wallet: {error}"),
            })?;
        }
        board::post(&dir, &wallet, orn, sats, &payout, password.as_deref().filter(|_| has_password(&wallet)))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Whether SOL swaps are open in this copy of the app: on Solana's test
/// network for test copies; on Solana itself once the lock is live there.
/// What's wrong with a SOL payout address as the seller types it, if
/// anything.
#[tauri::command]
async fn sol_payout_check(payout: String) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || solswaps::payout_problem(&payout)).await.unwrap_or(None)
}

/// A signed-in Market call: listings, messages, payment requests, reports.
#[tauri::command]
async fn market_call(app: AppHandle, method: String, path: String, body: Option<serde_json::Value>) -> Result<serde_json::Value, String> {
    let dir = folder(&app)?;
    tauri::async_runtime::spawn_blocking(move || board::market_call(&dir, &method, &path, body)).await.map_err(|error| error.to_string())?
}

/// Pictures the person picked, the only files a listing picture may be
/// sent from.
static PICKED: std::sync::Mutex<Vec<std::path::PathBuf>> = std::sync::Mutex::new(Vec::new());

/// Picks up to `room` pictures for a listing: their paths, and each as a
/// data: URL to show before it's sent.
#[tauri::command]
async fn market_pick_pictures(app: AppHandle, room: usize) -> Result<Vec<serde_json::Value>, String> {
    use base64::Engine;
    let picked = app.dialog().file().add_filter("Pictures", &["jpg", "jpeg", "png", "webp", "gif"]).blocking_pick_files().unwrap_or_default();
    let mut out = Vec::new();
    for file in picked.into_iter().take(room) {
        let Ok(path) = file.into_path() else { continue };
        PICKED.lock().unwrap_or_else(|p| p.into_inner()).push(path.clone());
        let bytes = fs::read(&path).map_err(|error| format!("could not read that picture: {error}"))?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err("One of those pictures is too big. Pick ones under 8 MB.".into());
        }
        let kind = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) { "image/png" } else if bytes.starts_with(b"RIFF") { "image/webp" } else if bytes.starts_with(b"GIF") { "image/gif" } else { "image/jpeg" };
        out.push(serde_json::json!({ "path": path, "url": format!("data:{kind};base64,{}", base64::engine::general_purpose::STANDARD.encode(&bytes)) }));
    }
    Ok(out)
}

/// Sends a picked picture to listing `id`.
#[tauri::command]
async fn market_listing_picture(app: AppHandle, id: i64, path: String) -> Result<serde_json::Value, String> {
    let dir = folder(&app)?;
    let path = std::path::PathBuf::from(path);
    if !PICKED.lock().unwrap_or_else(|p| p.into_inner()).contains(&path) {
        return Err("Pick the picture again.".into());
    }
    tauri::async_runtime::spawn_blocking(move || board::listing_picture(&dir, id, &path)).await.map_err(|error| error.to_string())?
}

/// GIFs from KLIPY, trending or searched, a page at a time.
#[tauri::command]
async fn klipy_gifs(app: AppHandle, q: String, page: u32) -> Result<serde_json::Value, String> {
    let dir = folder(&app)?;
    tauri::async_runtime::spawn_blocking(move || board::klipy(&dir, &q, page)).await.map_err(|error| error.to_string())?
}

/// Sends a picked picture for a Community post.
#[tauri::command]
async fn community_picture(app: AppHandle, path: String) -> Result<serde_json::Value, String> {
    let dir = folder(&app)?;
    let path = std::path::PathBuf::from(path);
    if !PICKED.lock().unwrap_or_else(|p| p.into_inner()).contains(&path) {
        return Err("Pick the picture again.".into());
    }
    tauri::async_runtime::spawn_blocking(move || board::community_picture(&dir, &path)).await.map_err(|error| error.to_string())?
}

/// Shows a notification the Mac's own way (UNUserNotificationCenter, which
/// the app asks permission from), as the notification plugin's older route
/// is dropped silently on current macOS. False where this isn't a Mac.
#[tauri::command]
fn mac_notify(title: String, body: String) -> bool {
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::NSString;
        use objc2_user_notifications::{UNMutableNotificationContent, UNNotificationRequest, UNNotificationSound, UNUserNotificationCenter};
        if objc2_foundation::NSBundle::mainBundle().bundleIdentifier().is_none() {
            return false;
        }
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(&title));
        content.setBody(&NSString::from_str(&body));
        content.setSound(Some(&UNNotificationSound::defaultSound()));
        let id = NSString::from_str(&format!("orien-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)));
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(&id, &content, None);
        UNUserNotificationCenter::currentNotificationCenter().addNotificationRequest_withCompletionHandler(&request, None);
        return true;
    }
    #[allow(unreachable_code)]
    {
        let _ = (title, body);
        false
    }
}

/// iPhones: shares the address (and its QR picture, a PNG in base64)
/// with the system's share sheet. Elsewhere the page shares by itself.
#[tauri::command]
fn share_address(app: AppHandle, text: String, picture: Option<String>) {
    #[cfg(target_os = "ios")]
    {
        use base64::Engine;
        let png = picture.and_then(|p| base64::engine::general_purpose::STANDARD.decode(p).ok());
        let _ = app.run_on_main_thread(move || keyboard::share(&text, png.as_deref()));
    }
    #[cfg(not(target_os = "ios"))]
    let _ = (app, text, picture);
}

/// The number on the Dock icon (none for 0). Tauri's badge call says it
/// worked but leaves the Dock blank here, so this asks AppKit directly.
#[tauri::command]
fn dock_badge(app: AppHandle, count: u32) {
    #[cfg(target_os = "macos")]
    let _ = app.run_on_main_thread(move || unsafe {
        use objc2::msg_send;
        use objc2::runtime::{AnyClass, AnyObject};
        use objc2_foundation::NSString;
        let Some(class) = AnyClass::get(c"NSApplication") else { return };
        let shared: *mut AnyObject = msg_send![class, sharedApplication];
        if shared.is_null() {
            return;
        }
        let tile: *mut AnyObject = msg_send![shared, dockTile];
        if tile.is_null() {
            return;
        }
        let label = (count > 0).then(|| NSString::from_str(&count.to_string()));
        let ptr: *const NSString = label.as_deref().map_or(std::ptr::null(), |l| l as *const NSString);
        let _: () = msg_send![tile, setBadgeLabel: ptr];
    });
    #[cfg(not(target_os = "macos"))]
    let _ = (app, count);
}


/// A page of Market listings.
#[tauri::command]
async fn market_listings(app: AppHandle, category: String, text: String, offset: u32) -> Result<serde_json::Value, String> {
    let dir = folder(&app)?;
    tauri::async_runtime::spawn_blocking(move || board::listings(&dir, &category, &text, offset)).await.map_err(|error| error.to_string())?
}

/// One Market listing with its seller.
#[tauri::command]
async fn market_listing(app: AppHandle, id: i64) -> Result<serde_json::Value, String> {
    let dir = folder(&app)?;
    tauri::async_runtime::spawn_blocking(move || board::listing(&dir, id)).await.map_err(|error| error.to_string())?
}

/// A seller's Market profile and listings.
#[tauri::command]
async fn market_profile(app: AppHandle, id: i64) -> Result<serde_json::Value, String> {
    let dir = folder(&app)?;
    tauri::async_runtime::spawn_blocking(move || board::profile(&dir, id)).await.map_err(|error| error.to_string())?
}

/// Marketplace profiles matching a name, for Send.
#[tauri::command]
async fn market_search(app: AppHandle, text: String) -> Result<serde_json::Value, String> {
    let dir = folder(&app).ok();
    tauri::async_runtime::spawn_blocking(move || board::search(dir.as_deref(), &text)).await.map_err(|error| error.to_string())?
}

/// A marketplace picture by name, as a data: URL, from the app's own copy.
#[tauri::command]
async fn market_image(app: AppHandle, name: String) -> Option<String> {
    let dir = folder(&app).ok()?;
    tauri::async_runtime::spawn_blocking(move || board::image(&dir, &name)).await.ok().flatten()
}

/// Deletes the Orien Network account for good, `confirm` being its name.
#[tauri::command]
async fn board_delete_account(app: AppHandle, confirm: String) -> Result<(), String> {
    let dir = folder(&app)?;
    tauri::async_runtime::spawn_blocking(move || board::delete_account(&dir, &confirm)).await.map_err(|error| error.to_string())?
}

/// Takes the profile's banner away.
#[tauri::command]
async fn board_banner_remove(app: AppHandle) -> Result<(), String> {
    let dir = folder(&app)?;
    tauri::async_runtime::spawn_blocking(move || board::remove_banner(&dir)).await.map_err(|error| error.to_string())?
}

/// What proving the profile's wallet takes: a tiny payment, its amount and
/// where it goes.
#[tauri::command]
async fn board_verify(app: AppHandle, address: String) -> Result<serde_json::Value, String> {
    let dir = folder(&app)?;
    tauri::async_runtime::spawn_blocking(move || board::verify(&dir, &address)).await.map_err(|error| error.to_string())?
}

/// The account's wallets: which are verified, which wait for their payment.
#[tauri::command]
async fn board_wallet_states(app: AppHandle) -> Result<serde_json::Value, String> {
    let dir = folder(&app)?;
    tauri::async_runtime::spawn_blocking(move || board::wallet_states(&dir)).await.map_err(|error| error.to_string())?
}

/// Saves the marketplace profile: name, a few lines about you, your
/// Telegram, X and Discord usernames, and who can message you.
#[tauri::command]
async fn board_profile_save(app: AppHandle, name: String, bio: String, telegram: String, x: String, discord: Option<String>, messages_from: Option<String>) -> Result<(), String> {
    let dir = folder(&app)?;
    let discord = discord.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || board::save_profile(&dir, &name, &bio, &telegram, &x, &discord, messages_from.as_deref()))
        .await
        .map_err(|error| error.to_string())?
}

/// Picks a picture and makes it the profile picture, or with `banner` the
/// profile's wide banner. False if none was picked.
#[tauri::command]
async fn board_picture_pick(app: AppHandle, banner: Option<bool>) -> Result<bool, String> {
    let banner = banner.unwrap_or(false);
    let dir = folder(&app)?;
    let picked = app.dialog().file().add_filter("Pictures", &["jpg", "jpeg", "png", "webp", "gif"]).blocking_pick_file();
    let Some(path) = picked.and_then(|file| file.into_path().ok()) else { return Ok(false) };
    tauri::async_runtime::spawn_blocking(move || board::set_picture(&dir, &path, banner)).await.map_err(|error| error.to_string())??;
    Ok(true)
}

/// Marketplace names and pictures for wallet addresses, where the
/// address's owner has a profile.
#[tauri::command]
async fn wallet_profiles(app: AppHandle, addresses: Vec<String>) -> Result<serde_json::Value, String> {
    let dir = folder(&app).ok();
    tauri::async_runtime::spawn_blocking(move || board::profiles(dir.as_deref(), &addresses)).await.map_err(|error| error.to_string())?
}

/// Adds a wallet rebuilt from its recovery phrase on another device: the
/// app's first, or one more. Its name and icon come along.
fn rebuild_wallet(app: &AppHandle, parcel: pair::Parcel) -> Result<String, String> {
    let dir = folder(app)?;
    let master = orien_net::phrase::from_words(&parcel.words).map_err(|error| error.to_string())?;
    let name: String = parcel.name.trim().chars().take(24).collect();
    let name = if name.is_empty() { "Wallet".to_string() } else { name };
    let first = wallet_file(&dir).is_none();
    let path = if first { make_home()? } else {
        let folder = wallets_folder(&dir)?;
        (1..10_000).map(|n| folder.join(format!("wallet-{n}.wallet"))).find(|p| !p.exists()).ok_or("too many wallets")?
    };
    Wallet::from_master(&path, master, parcel.account.max(1)).map_err(|error| error.to_string())?;
    let address = Wallet::peek(&path).map_err(|error| error.to_string())?.0;
    if !first && known_wallets(&dir).iter().any(|w| w.path != path && Wallet::peek(&w.path).is_ok_and(|(a, _)| a == address)) {
        let _ = fs::remove_file(&path);
        return Err(format!("{name} is already on this device."));
    }
    if first {
        use_shared(app)?;
    }
    let mut settings = settings(&dir);
    settings.wallets.retain(|w| w.path != path);
    settings.wallets.push(Named { path: path.clone(), name: name.clone() });
    settings.current = Some(path);
    save_settings(&dir, &settings)?;
    if let Some(icon) = parcel.icon.filter(|icon| !icon.is_empty()) {
        let _ = contacts::set_icon(&dir, &address.to_string(), &icon);
    }
    Ok(name)
}

/// A wallet's 24-word recovery phrase, to show its owner: needs the wallet
/// unlocked (Touch ID or Face ID a moment before, or its password).
#[tauri::command]
async fn wallet_phrase(app: AppHandle, path: String, password: Option<String>) -> Result<Vec<String>, String> {
    let dir = folder(&app)?;
    let path = known(&dir, &path)?;
    tauri::async_runtime::spawn_blocking(move || {
        let wallet = match open_signing(&path, password.as_deref())? {
            Signing::Here(wallet) => wallet,
            _ => Wallet::open(&path, password.as_deref()).map_err(|_| "password: needed".to_string())?,
        };
        let words = wallet.phrase().ok_or("This wallet was brought in from a file, so it has no recovery phrase.")?;
        Ok(words.into_iter().map(str::to_string).collect())
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Starts moving a wallet to another device: the code to show.
#[tauri::command]
async fn pair_offer() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(pair::offer).await.map_err(|error| error.to_string())?
}

/// The emoji to compare once the other device has joined; None until then.
#[tauri::command]
async fn pair_offer_wait() -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(pair::offer_wait).await.map_err(|error| error.to_string())?
}

/// Sends the wallet at `path`, sealed, once the emoji match. Needs the
/// wallet unlocked: Touch ID or Face ID a moment before, or its password.
#[tauri::command]
async fn pair_offer_send(app: AppHandle, path: String, password: Option<String>) -> Result<(), String> {
    let dir = folder(&app)?;
    let path = known(&dir, &path)?;
    tauri::async_runtime::spawn_blocking(move || {
        let wallet = match open_signing(&path, password.as_deref())? {
            Signing::Here(wallet) => wallet,
            _ => Wallet::open(&path, password.as_deref()).map_err(|_| "password: needed".to_string())?,
        };
        let words = wallet.phrase().ok_or("This wallet has no recovery phrase, so it can't be moved this way.")?.join(" ");
        let account = wallet.account_number().unwrap_or(1);
        let name = known_wallets(&dir).into_iter().find(|w| w.path == path).map(|w| w.name).unwrap_or_else(|| "Wallet".into());
        let icon = contacts::icons(&dir).get(&wallet.address().to_string()).map(|icon| icon.emoji.clone());
        pair::offer_send(&pair::Parcel { words, account, name, icon })
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Joins with a code from the other device: the emoji to compare.
#[tauri::command]
async fn pair_join(code: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || pair::join(&code)).await.map_err(|error| error.to_string())?
}

/// The wallet's name once it has arrived and been added ("3 wallets" when
/// several came together); None until then. The first of them opens.
#[tauri::command]
async fn pair_join_wait(app: AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(parcels) = pair::join_wait()? else { return Ok(None) };
        if parcels.len() == 1 {
            return rebuild_wallet(&app, parcels.into_iter().next().ok_or("nothing arrived")?).map(Some);
        }
        let dir = folder(&app)?;
        let mut added: Vec<(String, Option<PathBuf>)> = Vec::new();
        let mut last_error = None;
        for parcel in parcels {
            match rebuild_wallet(&app, parcel) {
                Ok(name) => added.push((name, settings(&dir).current)),
                Err(error) => last_error = Some(error),
            }
        }
        let Some((first, path)) = added.first().cloned() else {
            return Err(match last_error {
                Some(error) if error.ends_with("already on this device.") => "These wallets are already on this device.".into(),
                Some(error) => error,
                None => "Nothing arrived.".into(),
            });
        };
        let mut chosen = settings(&dir);
        chosen.current = path;
        save_settings(&dir, &chosen)?;
        Ok(Some(if added.len() == 1 { first } else { format!("{} wallets", added.len()) }))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// "Use my other device": the code this new device shows.
#[tauri::command]
async fn pair_invite() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(pair::invite).await.map_err(|error| error.to_string())?
}

/// The emoji to compare once the other device has typed the code; None
/// until then.
#[tauri::command]
async fn pair_invite_wait() -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(pair::invite_wait).await.map_err(|error| error.to_string())?
}

/// "Link a new device": the code the new device shows, typed or scanned.
/// The emoji to compare.
#[tauri::command]
async fn pair_accept(code: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || pair::accept(&code)).await.map_err(|error| error.to_string())?
}

/// The wallets "Link a new device" would send: names, and which need a
/// typed password (no Touch ID or Face ID kept for them).
#[tauri::command]
async fn pair_wallets(app: AppHandle) -> Result<Vec<serde_json::Value>, String> {
    let dir = folder(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        Ok(known_wallets(&dir)
            .into_iter()
            .map(|w| {
                let typed = has_password(&w.path) && !biometric::enabled(&w.path);
                serde_json::json!({ "path": w.path.display().to_string(), "name": w.name, "password": typed })
            })
            .collect())
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Sends every wallet, sealed, once the emoji match: one Touch ID or Face
/// ID for those that keep their password in the Keychain, and `passwords`
/// (by path) for the rest. Wallets brought in from a file, with no recovery
/// phrase, stay behind. Errors "password: needed <path>" or "password:
/// wrong <path>" name the wallet to ask about.
#[tauri::command]
async fn pair_accept_send(app: AppHandle, passwords: std::collections::HashMap<String, String>) -> Result<usize, String> {
    let dir = folder(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let all = known_wallets(&dir);
        let open = |w: &Named| {
            let typed = passwords.get(&w.path.display().to_string()).map(String::as_str);
            match open_signing(&w.path, typed) {
                Ok(Signing::Here(wallet)) => Ok(wallet),
                Ok(_) => Wallet::open(&w.path, typed).map_err(|_| "password: needed".to_string()),
                Err(error) => Err(error),
            }
        };
        // One Touch ID or Face ID for every wallet that needs it.
        let locked: Vec<PathBuf> = all.iter().filter(|w| matches!(open(w), Err(ref e) if e == "password: needed") && biometric::enabled(&w.path)).map(|w| w.path.clone()).collect();
        if !locked.is_empty() {
            biometric::unlock_all(&locked)?;
        }
        let icons = contacts::icons(&dir);
        let mut wallets = Vec::new();
        for w in &all {
            let wallet = open(w).map_err(|error| match error.as_str() {
                "password: needed" | "password: wrong" => format!("{error} {}", w.path.display()),
                _ => error,
            })?;
            let Some(words) = wallet.phrase() else { continue };
            let icon = icons.get(&wallet.address().to_string()).map(|icon| icon.emoji.clone());
            wallets.push(pair::Parcel { words: words.join(" "), account: wallet.account_number().unwrap_or(1), name: w.name.clone(), icon });
        }
        if wallets.is_empty() {
            return Err("None of these wallets has a recovery phrase, so they can't be moved this way.".into());
        }
        let count = wallets.len();
        pair::accept_send(&pair::Bundle { wallets })?;
        Ok(count)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn pair_cancel() {
    let _ = tauri::async_runtime::spawn_blocking(|| {
        pair::cancel_sending();
        pair::cancel_receiving();
    })
    .await;
}

/// A QR code, as an SVG picture, for `text`: the Receive screen's address.
#[tauri::command]
fn qr_code(text: String) -> Option<String> {
    swaps::qr(&text)
}

/// Your address as a QR code with Orien's sun in the middle. It uses the
/// strongest error correction, so the code still reads with its centre
/// covered by the sun.
#[tauri::command]
fn address_qr(text: String) -> Option<String> {
    use qrcode::{Color, EcLevel, QrCode};
    let code = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::H).ok()?;
    let width = code.width();
    let colors = code.to_colors();
    let quiet = 3;
    let size = width + quiet * 2;
    // A round clearing in the middle for the sun: a white disc drawn over
    // the code, cutting the squares under its edge into a clean circle.
    // About a quarter of the code across; the strongest error correction
    // reads the code without what it covers.
    let middle = width as f64 / 2.0;
    let radius = width as f64 * 0.15;
    let mut path = String::new();
    for y in 0..width {
        for x in 0..width {
            if colors[y * width + x] == Color::Dark {
                path.push_str(&format!("M{} {}h1v1h-1z", x + quiet, y + quiet));
            }
        }
    }
    // The sun, drawn in a 400-wide box (the favicon's), scaled to fit well
    // inside the circle.
    let centre = middle + quiet as f64;
    let sun = radius * 2.0 * 0.62;
    let box_at = centre - sun / 2.0;
    let scale = sun / 400.0;
    Some(format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {size} {size}" shape-rendering="crispEdges"><defs><linearGradient id="orien-sun" x1="76" y1="74" x2="436" y2="442" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="#f4b6d4"/><stop offset="0.25" stop-color="#b8c7ff"/><stop offset="0.48" stop-color="#d3b6ff"/><stop offset="0.72" stop-color="#a6ead6"/><stop offset="1" stop-color="#f9dda4"/></linearGradient></defs><rect width="{size}" height="{size}" fill="#fff"/><path d="{path}" fill="#16131f"/><circle cx="{centre}" cy="{centre}" r="{radius}" fill="#fff" shape-rendering="geometricPrecision"/><g transform="translate({box_at} {box_at}) scale({scale}) translate(-56 -58)" fill="url(#orien-sun)" shape-rendering="geometricPrecision"><rect x="239" y="74" width="36" height="66" rx="18"/><rect x="239" y="74" width="36" height="66" rx="18" transform="rotate(-44 256 298)"/><rect x="239" y="74" width="36" height="66" rx="18" transform="rotate(44 256 298)"/><path d="M138 298 A118 118 0 0 1 374 298 Z"/><rect x="76" y="338" width="360" height="40" rx="20"/><rect x="156" y="406" width="200" height="36" rx="18"/></g></svg>"##
    ))
}

/// Opens Orien's website in the phone's or computer's browser. Only this
/// one address, so the page can't be made to open anything else.
#[tauri::command]
fn open_site(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_url("https://orien.network", None::<&str>).map_err(|error| error.to_string())
}

/// Opens the network map's details view in the browser, from a Community
/// post about a new validator.
#[tauri::command]
fn open_network_map(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_url("https://explorer.orien.network/map/?view=details", None::<&str>).map_err(|error| error.to_string())
}

/// Opens the place to add a browser extension: Safari's own settings at
/// Orien Wallet's extension (Macs, where the extension comes with the app),
/// Orien Wallet's page on the Chrome Web Store, which Chrome, Edge and
/// Brave all add extensions from, or on Firefox Add-ons. The page opens in the browser named when
/// it's on this computer, otherwise in the usual one.
#[tauri::command]
fn open_extension(app: AppHandle, browser: String) -> Result<(), String> {
    const STORE: &str = "https://chromewebstore.google.com/detail/orien-wallet/oogifphigefggbdlcomcohbjfbgomcmn";
    match browser.as_str() {
        "chrome" | "edge" | "brave" => {
            if !open_in(&browser, STORE) {
                use tauri_plugin_opener::OpenerExt;
                app.opener().open_url(STORE, None::<&str>).map_err(|error| error.to_string())?;
            }
            Ok(())
        }
        #[cfg(target_os = "macos")]
        "safari" => {
            #[link(name = "SafariServices", kind = "framework")]
            unsafe extern "C" {}
            let _ = app.run_on_main_thread(|| {
                use objc2::runtime::AnyObject;
                use objc2::{class, msg_send};
                let id = objc2_foundation::NSString::from_str("network.orien.wallet.safari");
                let done = block2::RcBlock::new(|_: *mut AnyObject| {});
                // SAFETY: a class method of SafariServices with a string and a block.
                unsafe {
                    let _: () = msg_send![class!(SFSafariApplication), showPreferencesForExtensionWithIdentifier: &*id, completionHandler: &*done];
                }
            });
            Ok(())
        }
        "firefox" => {
            const ADDONS: &str = "https://addons.mozilla.org/firefox/addon/orien-wallet/";
            if !open_in("firefox", ADDONS) {
                use tauri_plugin_opener::OpenerExt;
                app.opener().open_url(ADDONS, None::<&str>).map_err(|error| error.to_string())?;
            }
            Ok(())
        }
        _ => Err("That can't be opened.".into()),
    }
}

/// Opens `url` in that browser if it's installed here; false if it isn't.
#[cfg(not(any(target_os = "ios", target_os = "android")))]
fn open_in(browser: &str, url: &str) -> bool {
    use std::path::PathBuf;
    use std::process::Command;
    #[cfg(target_os = "macos")]
    {
        let name = match browser { "chrome" => "Google Chrome", "edge" => "Microsoft Edge", "firefox" => "Firefox", _ => "Brave Browser" };
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        let found = [PathBuf::from("/Applications"), home.join("Applications")].into_iter().map(|dir| dir.join(format!("{name}.app"))).find(|app| app.exists());
        found.is_some_and(|app| Command::new("open").arg("-a").arg(app).arg(url).spawn().is_ok())
    }
    #[cfg(windows)]
    {
        let inside = match browser {
            "chrome" => r"Google\Chrome\Application\chrome.exe",
            "edge" => r"Microsoft\Edge\Application\msedge.exe",
            "firefox" => r"Mozilla Firefox\firefox.exe",
            _ => r"BraveSoftware\Brave-Browser\Application\brave.exe",
        };
        let found = ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"]
            .into_iter()
            .filter_map(std::env::var_os)
            .map(|dir| PathBuf::from(dir).join(inside))
            .find(|exe| exe.exists());
        found.is_some_and(|exe| Command::new(exe).arg(url).spawn().is_ok())
    }
    #[cfg(target_os = "linux")]
    {
        let names: &[&str] = match browser {
            "chrome" => &["google-chrome", "google-chrome-stable"],
            "edge" => &["microsoft-edge", "microsoft-edge-stable"],
            "firefox" => &["firefox"],
            _ => &["brave-browser", "brave"],
        };
        let path = std::env::var_os("PATH").unwrap_or_default();
        let found = names.iter().flat_map(|name| std::env::split_paths(&path).map(move |dir| dir.join(name))).find(|exe| exe.is_file());
        found.is_some_and(|exe| Command::new(exe).arg(url).spawn().is_ok())
    }
}
#[cfg(any(target_os = "ios", target_os = "android"))]
fn open_in(_: &str, _: &str) -> bool {
    false
}

/// Opens the web page a Community post links to, in the browser. Only
/// web addresses.
#[tauri::command]
fn open_post_link(app: AppHandle, url: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let url = url.trim();
    if !(url.starts_with("https://") || url.starts_with("http://")) || url.len() > 2048 || url.chars().any(char::is_whitespace) {
        return Err("That link can't be opened.".into());
    }
    app.opener().open_url(url, None::<&str>).map_err(|error| error.to_string())
}

/// Opens orien.network's privacy policy or terms of service.
#[tauri::command]
fn open_legal(app: AppHandle, page: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let url = match page.as_str() {
        "privacy" => "https://orien.network/privacy/",
        "terms" => "https://orien.network/terms/",
        _ => return Err("That can't be opened.".into()),
    };
    app.opener().open_url(url, None::<&str>).map_err(|error| error.to_string())
}

/// Opens a way to reach someone from their profile: an email in the mail
/// app, a Telegram or X handle on its site. Only those, built here.
#[tauri::command]
fn open_contact(app: AppHandle, kind: String, value: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let handle = |v: &str| !v.is_empty() && v.len() <= 32 && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    let url = match kind.as_str() {
        "email" if value.len() <= 254 && value.contains('@') && !value.chars().any(|c| c.is_whitespace() || "<>\"'?&#/:".contains(c)) => format!("mailto:{value}"),
        "telegram" if handle(&value) => format!("https://t.me/{value}"),
        "x" if handle(&value) => format!("https://x.com/{value}"),
        _ => return Err("That can't be opened.".into()),
    };
    app.opener().open_url(url, None::<&str>).map_err(|error| error.to_string())
}

#[tauri::command]
async fn sol_open() -> bool {
    tauri::async_runtime::spawn_blocking(|| solswaps::lock_live(solswaps::cluster())).await.unwrap_or(false)
}

/// Puts a SOL offer on the board: `orn` ORN for `sol` SOL, paid at
/// `payout`, the seller's own SOL wallet. Approves it, as for Bitcoin.
#[tauri::command]
async fn board_post_sol(app: AppHandle, orn: String, sol: String, payout: String, password: Option<String>) -> Result<(), String> {
    let orn = money::read(&orn)?;
    let lamports = solswaps::read_sol(&sol)?;
    let dir = folder(&app)?;
    let wallet = wallet_file(&dir).ok_or("no wallet yet")?;
    tauri::async_runtime::spawn_blocking(move || {
        let password = password.filter(|p| !p.is_empty()).or_else(|| biometric::unlocked(&wallet));
        if has_password(&wallet) {
            Wallet::open(&wallet, password.as_deref()).map_err(|error| match error.downcast_ref::<OpenError>() {
                Some(OpenError::NeedsPassword) => "password: needed".to_string(),
                Some(OpenError::WrongPassword) => "password: wrong".to_string(),
                None => format!("could not open the wallet: {error}"),
            })?;
        }
        board::post_sol(&dir, &wallet, orn, lamports, &payout, password.as_deref().filter(|_| has_password(&wallet)))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Takes SOL offer `offer` for this wallet; the buyer pays from `refund`,
/// their own SOL wallet, where anything left over goes back.
#[tauri::command]
async fn board_take_sol(app: AppHandle, offer: i64, refund: String, orn: String, password: Option<String>) -> Result<(), String> {
    let orn = money::read(&orn)?;
    let dir = folder(&app)?;
    let wallet = wallet_file(&dir).ok_or("no wallet yet")?;
    let address = Wallet::peek(&wallet).map_err(|error| error.to_string())?.0.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let password = approve_take(&wallet, password)?;
        board::take_sol(&dir, &wallet, offer, &address, &refund)?;
        approval::approve(&dir, offer, orn, &wallet, password.as_deref());
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())?
}

/// A buyer approves once, when taking an offer, so the ORN are claimed by
/// themselves at the end: the wallet must open with what was given (the
/// password, or Touch ID a moment ago). What unlocks it, for the approval.
fn approve_take(wallet: &std::path::Path, password: Option<String>) -> Result<Option<String>, String> {
    let password = password.filter(|p| !p.is_empty()).or_else(|| biometric::unlocked(wallet));
    if !has_password(wallet) {
        return Ok(None);
    }
    Wallet::open(wallet, password.as_deref()).map_err(|error| match error.downcast_ref::<OpenError>() {
        Some(OpenError::NeedsPassword) => "password: needed".to_string(),
        Some(OpenError::WrongPassword) => "password: wrong".to_string(),
        None => format!("could not open the wallet: {error}"),
    })?;
    Ok(password)
}

/// Takes offer `offer` for this wallet; a refund would go to `refund`.
#[tauri::command]
async fn board_take(app: AppHandle, offer: i64, refund: String, orn: String, password: Option<String>) -> Result<(), String> {
    let orn = money::read(&orn)?;
    let dir = folder(&app)?;
    let wallet = wallet_file(&dir).ok_or("no wallet yet")?;
    let address = orien_net::keys::Wallet::peek(&wallet).map_err(|error| error.to_string())?.0.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let password = approve_take(&wallet, password)?;
        board::take(&dir, &wallet, offer, &address, &refund)?;
        approval::approve(&dir, offer, orn, &wallet, password.as_deref());
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Links this wallet to the signed-in marketplace account, as signing in
/// through the app does. Nothing is paid. False when no one is signed in.
#[tauri::command]
async fn board_link(app: AppHandle) -> Result<bool, String> {
    let dir = folder(&app)?;
    let address = wallet_file(&dir).and_then(|file| orien_net::keys::Wallet::peek(&file).ok()).map(|(address, _)| address.to_string()).ok_or("no wallet yet")?;
    tauri::async_runtime::spawn_blocking(move || board::link_wallet(&dir, &address)).await.map_err(|error| error.to_string())?
}

/// Market buyers pay this wallet from now on.
#[tauri::command]
async fn board_paid_wallet(app: AppHandle, address: String) -> Result<(), String> {
    let dir = folder(&app)?;
    tauri::async_runtime::spawn_blocking(move || board::set_paid_wallet(&dir, &address)).await.map_err(|error| error.to_string())?
}

/// Takes a wallet off the Orien profile.
#[tauri::command]
async fn board_wallet_remove(app: AppHandle, address: String) -> Result<(), String> {
    let dir = folder(&app)?;
    tauri::async_runtime::spawn_blocking(move || board::remove_wallet(&dir, &address)).await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn board_withdraw(app: AppHandle, offer: i64) -> Result<(), String> {
    let dir = folder(&app)?;
    tauri::async_runtime::spawn_blocking(move || board::withdraw(&dir, offer)).await.map_err(|error| error.to_string())?
}

/// Asks the Mac, once, whether the wallet may show notifications, so the
/// question comes as the wallet opens and not with the first payment,
/// which macOS would drop while asking. Elsewhere there is nothing to ask.
#[tauri::command]
fn notifications_ask(app: AppHandle) {
    // iPhones also sign up with Apple, to be told of payments while closed.
    #[cfg(target_os = "ios")]
    push::start(&app);
    #[cfg(not(target_os = "ios"))]
    let _ = app;
    #[cfg(target_os = "macos")]
    {
        use objc2_user_notifications::{UNAuthorizationOptions, UNUserNotificationCenter};
        // Only a bundled app may ask; a bare program would be stopped.
        if objc2_foundation::NSBundle::mainBundle().bundleIdentifier().is_none() {
            return;
        }
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let done = block2::RcBlock::new(|_granted: objc2::runtime::Bool, _error: *mut objc2_foundation::NSError| {});
        center.requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound | UNAuthorizationOptions::Badge,
            &done,
        );
    }
}

/// Tells the Market this device's wallets and their names, while signed in,
/// when they changed.
#[tauri::command]
async fn board_wallets_sync(app: AppHandle) -> Result<(), String> {
    let dir = folder(&app)?;
    tauri::async_runtime::spawn_blocking(move || board::report_wallets(&dir, false)).await.map_err(|error| error.to_string())?
}

/// Tells the Market server which wallets this iPhone wants to hear about,
/// when that changed. Nothing to do elsewhere.
#[tauri::command]
async fn push_sync() {
    #[cfg(target_os = "ios")]
    let _ = tauri::async_runtime::spawn_blocking(push::sync).await;
}

/// Starts the app: the desktop program's main, and the iPhone app's entry.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Started by Firefox for its extension: pass its questions to the
    // running app and nothing else.
    #[cfg(desktop)]
    if firefox::asked_by_firefox() {
        firefox::answer_firefox();
        return;
    }
    // Started as administrator to grant the right for large pages: do
    // that and nothing else.
    #[cfg(desktop)]
    large_pages::grant_if_asked();
    #[cfg(target_os = "linux")]
    linux::register_appimage();
    // Linux with NVIDIA graphics: WebKit's newer way of drawing crashes the
    // app (seen switching themes); the older way is steady everywhere.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        // SAFETY: set before any other thread starts.
        unsafe { std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1") };
    }
    let builder = tauri::Builder::default();
    // Reopening after an update that installed by itself: stay behind the
    // app the person is using (a Mac brings a starting app in front).
    #[cfg(target_os = "macos")]
    let builder = builder.activate_ignoring_other_apps(
        !std::env::var_os("HOME").is_some_and(|home| std::path::Path::new(&home).join("Library/Application Support/network.orien.wallet/reopen-as").exists()),
    );
    // One copy at a time on computers: opening it again shows the one that
    // is running (from the tray, too) instead of starting another.
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| background::show(app)));
    // The window opens at the size and place it was left, never shown by
    // it: a start at login stays out of sight.
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_window_state::Builder::default().with_state_flags(background::WINDOW_STATE).build());
    let builder = builder.plugin(tauri_plugin_dialog::init()).plugin(tauri_plugin_haptics::init()).plugin(tauri_plugin_notification::init()).plugin(tauri_plugin_opener::init());
    // iPhones scan the QR code another device shows, to move a wallet here.
    #[cfg(mobile)]
    let builder = builder.plugin(tauri_plugin_barcode_scanner::init());
    // Desktop copies update themselves; iPhone copies come from the App Store.
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_updater::Builder::new().build()).plugin(
        tauri_plugin_autostart::Builder::new().args([background::QUIET]).build(),
    );
    builder
        .setup(|app| {
            // Test copies of the app swap SOL on Solana's free test network.
            solswaps::set_cluster(app.config().identifier.contains(".check"));
            // Computers keep the wallet running in the menu bar or tray; the
            // window starts hidden so a login start shows nothing.
            #[cfg(desktop)]
            background::setup(app.handle())?;
            #[cfg(desktop)]
            extension::serve(app.handle().clone());
            #[cfg(desktop)]
            std::thread::spawn(firefox::register);
            #[cfg(mobile)]
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                #[cfg(target_os = "ios")]
                keyboard::hold_still(&window);
            }
            #[cfg(target_os = "ios")]
            {
                keyboard::hide_accessory_bar();
                keyboard::follow(app.handle().clone());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![share_address, has_wallet, create_wallet, save_phrase, restore_wallet, set_password, choose_wallet, overview_now, send_payment, activity, quote, contacts, save_contact, delete_contact, contacts_sync, members, save_member, delete_member, biometric_unlock, biometric_enable, update::check_update, update::install_update, update::download_update, update::away_seconds, mine_start, mine_stop, mine_resume, mine_set, mine_status, mine_benchmark, mine_benchmark_cancel, large_pages_state, large_pages_set_up, wallets, wallet_balances, use_wallet, rename_wallet, add_wallet, forget_wallet, swap_list, swap_sell, swap_buy, swap_accept, swap_step, swap_smallest, board_offers, board_me, board_email_start, board_email_finish, board_sign_out, board_post, board_take, board_withdraw, board_link, notifications_ask, push_sync, open_network_map, board_wallets_sync, board_post_sol, board_take_sol, sol_open, sol_payout_check, open_site, qr_code, address_qr, wallet_profiles, board_profile_save, board_picture_pick, board_verify, board_wallet_states, market_image, board_banner_remove, market_search, market_listings, market_listing, market_profile, market_call, market_pick_pictures, market_listing_picture, community_picture, klipy_gifs, dock_badge, mac_notify, board_delete_account, wallet_icons, set_wallet_icon, vault_ready, vault_unlock, open_contact, pair_offer, pair_offer_wait, pair_offer_send, pair_join, pair_join_wait, pair_invite, pair_invite_wait, pair_accept, pair_wallets, pair_accept_send, pair_cancel, wallet_phrase, phrase_check, app_theme, set_app_theme, set_backdrop, app_prefs, set_app_pref, app_unlock, computer_settings, set_computer_setting, notifications_muted, set_notification, open_legal, open_post_link, open_extension, haptics::haptic, haptics::haptic_ready, sitesignin::site_signin_answer, board_paid_wallet, board_wallet_remove])
        .build(tauri::generate_context!())
        .expect("the wallet window could not start")
        .run(|_app, _event| {
            // On a Mac, a click on the Dock icon brings the window back.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = _event {
                background::show(_app);
            }
        });
}

#[cfg(test)]
mod tests {
    /// Installed again, an iPhone app finds itself in a new folder: the
    /// wallets it made are found there, under the names they were given.
    #[test]
    fn wallets_follow_the_apps_folder() {
        let dir = std::env::temp_dir().join(format!("orien-moved-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("wallets")).unwrap();
        std::fs::write(dir.join("wallets/wallet-1.wallet"), "{}").unwrap();
        let old = std::path::PathBuf::from("/var/mobile/Containers/Data/Application/OLD/Library/wallets/wallet-1.wallet");
        let saved = super::Settings {
            wallets: vec![super::Named { name: "Savings".into(), path: old.clone() }],
            current: Some(old),
            ..Default::default()
        };
        super::save_settings(&dir, &saved).unwrap();
        let found = super::settings(&dir);
        assert_eq!(found.wallets[0].path, dir.join("wallets/wallet-1.wallet"));
        assert_eq!(found.current, Some(dir.join("wallets/wallet-1.wallet")));
        assert_eq!(found.wallets[0].name, "Savings");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A whole swap with the app's own code: a real lock and a real claim on
    /// a running local test network, and a pretend Bitcoin network. Start one
    /// first, with epochs short enough for a swap's 48 hours to fit in 720:
    /// `orien devnet init --dir /tmp/orien-swap-net --epoch-seconds 600 && orien devnet
    /// run --dir /tmp/orien-swap-net`, then run with ORIEN_DEVNET set to it.
    #[test]
    fn a_whole_swap_on_a_local_network() {
        use orien_swap::bitcoin::{Address, CompressedPublicKey, Network};
        use orien_swap::swap::{Offer, Orien, Reply, Stage, Swap};
        use orien_swap::testing::TestChain;
        let Ok(root) = std::env::var("ORIEN_DEVNET") else {
            eprintln!("ORIEN_DEVNET is not set; skipped");
            return;
        };
        let root = std::path::Path::new(&root);
        let genesis = root.join("genesis.json");
        let client = super::connect(&genesis).unwrap();
        let network = orien_net::keys::load_genesis(&genesis).unwrap();
        let side = |wallet: std::path::PathBuf| super::swaps::AppOrien {
            client: &client,
            wallet_file: wallet,
            password: None,
            fees: network.fee,
            network: network.id().to_string(),
            epoch_seconds: network.epoch_seconds,
        };
        let seller = side(root.join("wallets/alice.wallet"));
        let fresh = root.join(format!("buyer-{}.wallet", std::process::id()));
        orien_net::keys::Wallet::create(&fresh).unwrap();
        let buyer = side(fresh.clone());
        let btc = || {
            let (_, key) = orien_swap::htlc::new_key();
            Address::p2wpkh(&CompressedPublicKey(key.inner), Network::Regtest).to_string()
        };
        let bitcoin = TestChain::at(100);
        let chain = orien_swap::chain::Explorers::one(bitcoin.clone());

        let orn = 5 * orien_ledger::COIN;
        let mut sell = Swap::sell(&seller, Network::Regtest, buyer.me(), orn, 1_000_000, &btc()).unwrap();
        sell.advance(&chain, &seller).unwrap();
        let offer = Offer::read(&sell.offer(seller.network()).unwrap().code()).unwrap();
        let (mut buy, reply) = Swap::buy(&buyer, &chain, Network::Regtest, &offer, &btc()).unwrap();
        sell.accept(&chain, &seller, &Reply::read(&reply.code()).unwrap()).unwrap();
        bitcoin.pay(&buy.bitcoin_address().unwrap(), 1_000_000);
        bitcoin.mine(3);
        for _ in 0..4 {
            sell.advance(&chain, &seller).unwrap();
            buy.advance(&chain, &buyer).unwrap();
        }
        bitcoin.mine(3);
        sell.advance(&chain, &seller).unwrap();
        buy.advance(&chain, &buyer).unwrap();
        assert_eq!((sell.stage, buy.stage), (Stage::Done, Stage::Done));
        let status = client.account(&buyer.me(), 0);
        assert_eq!(status.spendable, Some(orn as i128 - network.fee.min as i128), "the buyer has the ORN, less the claim's fee");
        let _ = std::fs::remove_file(fresh);
    }

    /// A whole swap through a running test copy of the swap board, between
    /// alice and bob of a running local test network, on a pretend Bitcoin
    /// network: alice posts, bob takes, and each side's wallet does the rest
    /// through the board. Needs ORIEN_DEVNET, ORIEN_MARKET (a debug build)
    /// and ORIEN_BOARD_TEST, a folder with `alice` and `bob` folders whose
    /// board.json holds a session of a board account.
    #[test]
    #[ignore = "needs a local network and a test copy of the swap board"]
    fn a_whole_swap_through_the_board() {
        use orien_swap::bitcoin::{Address, CompressedPublicKey, Network};
        use orien_swap::swap::Stage;
        use orien_swap::testing::TestChain;
        let root = std::path::PathBuf::from(std::env::var("ORIEN_DEVNET").unwrap());
        let folders = std::path::PathBuf::from(std::env::var("ORIEN_BOARD_TEST").unwrap());
        let genesis = root.join("genesis.json");
        let client = super::connect(&genesis).unwrap();
        let network = orien_net::keys::load_genesis(&genesis).unwrap();
        let side = |name: &str| super::swaps::AppOrien {
            client: &client,
            wallet_file: root.join(format!("wallets/{name}.wallet")),
            password: None,
            fees: network.fee,
            network: network.id().to_string(),
            epoch_seconds: network.epoch_seconds,
        };
        let (alice, bob) = (side("alice"), side("bob"));
        let (alice_dir, bob_dir) = (folders.join("alice"), folders.join("bob"));
        let btc = || {
            let (_, key) = orien_swap::htlc::new_key();
            Address::p2wpkh(&CompressedPublicKey(key.inner), Network::Regtest).to_string()
        };
        let bitcoin = TestChain::at(100);
        let chain = orien_swap::chain::Explorers::one(bitcoin.clone());
        let bob_before = client.account(&orien_net::keys::Wallet::peek(&bob.wallet_file).unwrap().0, 0).spendable.unwrap();

        // Alice posts 5 ORN for 0.01 BTC; bob takes it.
        let orn = 5 * orien_ledger::COIN;
        super::board::post_on(&alice_dir, &alice.wallet_file, Network::Regtest, orn, 1_000_000, &btc()).unwrap();
        let posted = super::board::mine(&alice_dir).unwrap();
        let id = posted.as_array().unwrap().iter().find(|s| s["offer"]["state"] == "open").unwrap()["offer"]["id"].as_i64().unwrap();
        let bob_address = orien_net::keys::Wallet::peek(&bob.wallet_file).unwrap().0.to_string();
        super::board::take_on(&bob_dir, &bob.wallet_file, Network::Regtest, id, &bob_address, &btc()).unwrap();

        let (mut alice_swaps, mut bob_swaps) = (Vec::new(), Vec::new());
        let mut paid = false;
        let mut board_state = String::new();
        for round in 0..40 {
            super::board::drive_on(&alice, Network::Regtest, &chain, &alice_dir, &mut alice_swaps).unwrap();
            super::swaps::step_on(&alice, &chain, &alice_dir, &mut alice_swaps).unwrap();
            super::board::drive_on(&bob, Network::Regtest, &chain, &bob_dir, &mut bob_swaps).unwrap();
            super::swaps::step_on(&bob, &chain, &bob_dir, &mut bob_swaps).unwrap();
            // Bob pays the Bitcoin, as a person would, once his wallet shows
            // where.
            if !paid
                && let Some(kept) = bob_swaps.first()
                && kept.swap.stage == Stage::WaitingForBitcoin
            {
                bitcoin.pay(&kept.swap.bitcoin_address().unwrap(), 1_000_000);
                paid = true;
            }
            if paid {
                bitcoin.mine(1);
            }
            let stages: Vec<_> = alice_swaps.iter().chain(&bob_swaps).map(|k| format!("{:?} {:?}", k.swap.stage, k.problem)).collect();
            board_state = super::board::mine(&alice_dir).unwrap().as_array().unwrap().iter()
                .find(|s| s["offer"]["id"] == id).map(|s| s["offer"]["state"].as_str().unwrap().to_string()).unwrap_or_default();
            println!("round {round}: board {board_state}; {stages:?}");
            if board_state == "done" && alice_swaps.iter().chain(&bob_swaps).all(|k| k.swap.stage == Stage::Done) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
        assert_eq!(board_state, "done");
        assert_eq!((alice_swaps[0].swap.stage.clone(), bob_swaps[0].swap.stage.clone()), (Stage::Done, Stage::Done));
        let bob_after = client.account(&orien_net::keys::Wallet::peek(&bob.wallet_file).unwrap().0, 0).spendable.unwrap();
        assert_eq!(bob_after - bob_before, orn as i128 - network.fee.min as i128, "bob has the ORN, less the claim's fee");
    }
    /// The same swap, with Bob's app closed from the moment he has paid: Alice's
    /// wallet takes the Bitcoin and delivers Bob's ORN to him all the same.
    #[test]
    #[ignore = "needs a local network and a test copy of the swap board"]
    fn the_seller_delivers_when_the_buyer_is_away() {
        use orien_swap::bitcoin::{Address, CompressedPublicKey, Network};
        use orien_swap::swap::Stage;
        use orien_swap::testing::TestChain;
        let root = std::path::PathBuf::from(std::env::var("ORIEN_DEVNET").unwrap());
        let folders = std::path::PathBuf::from(std::env::var("ORIEN_BOARD_TEST").unwrap());
        let genesis = root.join("genesis.json");
        let client = super::connect(&genesis).unwrap();
        let network = orien_net::keys::load_genesis(&genesis).unwrap();
        let side = |name: &str| super::swaps::AppOrien {
            client: &client,
            wallet_file: root.join(format!("wallets/{name}.wallet")),
            password: None,
            fees: network.fee,
            network: network.id().to_string(),
            epoch_seconds: network.epoch_seconds,
        };
        let (alice, bob) = (side("alice"), side("bob"));
        let (alice_dir, bob_dir) = (folders.join("alice"), folders.join("bob"));
        let btc = || {
            let (_, key) = orien_swap::htlc::new_key();
            Address::p2wpkh(&CompressedPublicKey(key.inner), Network::Regtest).to_string()
        };
        let bitcoin = TestChain::at(100);
        let chain = orien_swap::chain::Explorers::one(bitcoin.clone());
        let bob_before = client.account(&orien_net::keys::Wallet::peek(&bob.wallet_file).unwrap().0, 0).spendable.unwrap();

        // Alice posts 5 ORN for 0.01 BTC; bob takes it.
        let orn = 5 * orien_ledger::COIN;
        super::board::post_on(&alice_dir, &alice.wallet_file, Network::Regtest, orn, 1_000_000, &btc()).unwrap();
        let posted = super::board::mine(&alice_dir).unwrap();
        let id = posted.as_array().unwrap().iter().find(|s| s["offer"]["state"] == "open").unwrap()["offer"]["id"].as_i64().unwrap();
        let bob_address = orien_net::keys::Wallet::peek(&bob.wallet_file).unwrap().0.to_string();
        super::board::take_on(&bob_dir, &bob.wallet_file, Network::Regtest, id, &bob_address, &btc()).unwrap();

        let (mut alice_swaps, mut bob_swaps) = (Vec::new(), Vec::new());
        let mut paid = false;
        let mut board_state = String::new();
        // The seller waits an epoch before delivering, so an open buyer's
        // wallet can claim first: give it a few epochs.
        for round in 0..900 {
            super::board::drive_on(&alice, Network::Regtest, &chain, &alice_dir, &mut alice_swaps).unwrap();
            super::swaps::step_on(&alice, &chain, &alice_dir, &mut alice_swaps).unwrap();
            // Bob's app closes once he has paid and Alice's wallet has his
            // reply: only Alice's runs on.
            let bob_away = paid && alice_swaps.first().is_some_and(|k| !matches!(k.swap.stage, Stage::Preparing | Stage::WaitingForReply));
            if !bob_away {
                super::board::drive_on(&bob, Network::Regtest, &chain, &bob_dir, &mut bob_swaps).unwrap();
                super::swaps::step_on(&bob, &chain, &bob_dir, &mut bob_swaps).unwrap();
            }
            // Bob pays the Bitcoin, as a person would, once his wallet shows
            // where.
            if !paid
                && let Some(kept) = bob_swaps.first()
                && kept.swap.stage == Stage::WaitingForBitcoin
            {
                bitcoin.pay(&kept.swap.bitcoin_address().unwrap(), 1_000_000);
                paid = true;
            }
            if paid {
                bitcoin.mine(1);
            }
            let stages: Vec<_> = alice_swaps.iter().chain(&bob_swaps).map(|k| format!("{:?} {:?}", k.swap.stage, k.problem)).collect();
            board_state = super::board::mine(&alice_dir).unwrap().as_array().unwrap().iter()
                .find(|s| s["offer"]["id"] == id).map(|s| s["offer"]["state"].as_str().unwrap().to_string()).unwrap_or_default();
            println!("round {round}: board {board_state}; {stages:?}");
            if alice_swaps.first().is_some_and(|k| k.swap.finished()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1000));
        }
        let _ = board_state;
        assert!(alice_swaps[0].swap.finished(), "Alice's wallet took the Bitcoin and delivered");
        let bob_after = client.account(&orien_net::keys::Wallet::peek(&bob.wallet_file).unwrap().0, 0).spendable.unwrap();
        assert_eq!(bob_after - bob_before, orn as i128 - network.fee.min as i128, "bob has the ORN, less the claim's fee");
    }

    /// Signing up to a test copy of the swap board with an emailed code,
    /// taking an offer at once with no linked wallet, and then linking bob's
    /// wallet, as signing in in the app does. Needs ORIEN_DEVNET, ORIEN_MARKET (a debug
    /// build) pointing at a board that watches that network and prints its
    /// codes to the file ORIEN_BOARD_LOG.
    #[test]
    #[ignore = "needs a local network and a test copy of the swap board"]
    fn signing_up_links_the_wallet_on_the_board() {
        let root = std::path::PathBuf::from(std::env::var("ORIEN_DEVNET").unwrap());
        let log = std::path::PathBuf::from(std::env::var("ORIEN_BOARD_LOG").unwrap());
        let dir = std::env::temp_dir().join(format!("orien-board-signup-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let email = format!("signup-{}@example.com", std::process::id());

        super::board::email_start(&dir, &email).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        let text = std::fs::read_to_string(&log).unwrap();
        let line = text.lines().rev().find(|line| line.contains(&email)).expect("the code in the log");
        let code = line.rsplit(' ').next().unwrap().trim();
        assert!(!super::board::email_finish(&dir, &email, code, None).unwrap(), "a new email asks for a name");
        assert!(super::board::email_finish(&dir, &email, code, Some("Bob (signup test)")).unwrap());
        let me = super::board::me(&dir).unwrap().expect("signed in");
        assert_eq!((me["name"].as_str(), me["wallet"].as_str()), (Some("Bob (signup test)"), None));

        // Signed in with no verified wallet, an account can take an offer
        // at once, for this wallet's address.
        let bob_file = root.join("wallets/bob.wallet");
        let bob = orien_net::keys::Wallet::peek(&bob_file).unwrap().0.to_string();
        let offers = super::board::offers(&dir).unwrap();
        if let Some(offer) = offers["offers"].as_array().and_then(|o| o.first()).and_then(|o| o["id"].as_i64()) {
            let refund = "bcrt1qw508d6qejxtdg4y5r3zarvary0c5xw7kygt080";
            super::board::take_on(&dir, &bob_file, orien_swap::bitcoin::Network::Regtest, offer, &bob, refund).unwrap();
        }

        // Signing in links the wallet, with no payment.
        assert!(super::board::link_wallet(&dir, &bob).unwrap());
        assert_eq!(super::board::me(&dir).unwrap().unwrap()["wallet"].as_str(), Some(bob.as_str()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Reads a wallet from a running local test network, if there is one:
    /// `orien devnet init && orien devnet run` in the repository root, then
    /// run with ORIEN_DEVNET=devnet.
    #[test]
    fn reads_a_wallet_from_the_local_network() {
        let Ok(root) = std::env::var("ORIEN_DEVNET") else {
            eprintln!("ORIEN_DEVNET does not name a running local test network; skipped");
            return;
        };
        let root = std::path::Path::new(&root);
        if !root.join("genesis.json").is_file() {
            eprintln!("no local test network at {}; skipped", root.display());
            return;
        }
        let view = super::overview(&root.join("wallets/alice.wallet"), &root.join("genesis.json")).unwrap().0;
        assert_eq!(view.address.len(), 64);
        assert!(view.balance.is_some());
        assert_eq!(view.answered, view.validators);
        println!("{} holds {:?}", view.address, view.balance);
    }

    /// Reads this computer's wallet's balance from the live network the app
    /// is built for. Read-only: nothing is signed or sent. Run with
    /// `cargo test -- --ignored live`.
    #[test]
    #[ignore = "reaches the live network"]
    fn reads_the_live_network() {
        let dir = std::env::temp_dir().join("orien-wallet-live-check");
        std::fs::create_dir_all(&dir).unwrap();
        let genesis = super::genesis_file(&dir).unwrap();
        let wallet = super::shared_wallet();
        assert!(wallet.is_file(), "no wallet at {}", wallet.display());
        let (view, client, address) = super::overview(&wallet, &genesis).unwrap();
        if let Some(page) = client.activity(&address, 0, orien_net::history::MAX_ENTRIES) {
            let sent = page.entries.iter().filter(|e| e.income == orien_ledger::Income::Sent).count();
            println!("activity since epoch {}: {} entries, {} of them sent", page.since, page.entries.len(), sent);
        }
        match client.history(&address, 0, orien_net::history::MAX_ENTRIES) {
            Some(page) => {
                println!("history kept since epoch {}, now epoch {}:", page.since, page.epoch);
                for item in &page.entries {
                    println!("  epoch {} {:?} {} ORN", item.epoch, item.income, super::money::show(item.amount as i128));
                }
            }
            None => println!("no validator keeps history yet"),
        }
        println!(
            "{} holds {:?} ORN after {} payments; {} of {} validators answered; epoch {}; password: {}",
            view.address, view.balance, view.payments, view.answered, view.validators, view.epoch, view.locked
        );
        assert!(view.balance.is_some());
    }

    /// Sends 1 from alice to bob on a running local test network, through
    /// the app's own sending code, and checks both balances moved.
    #[test]
    fn sends_a_payment_on_the_local_network() {
        let Ok(root) = std::env::var("ORIEN_DEVNET") else {
            eprintln!("ORIEN_DEVNET does not name a running local test network; skipped");
            return;
        };
        let root = std::path::Path::new(&root);
        if !root.join("genesis.json").is_file() {
            eprintln!("no local test network at {}; skipped", root.display());
            return;
        }
        let (alice, bob, genesis) =
            (root.join("wallets/alice.wallet"), root.join("wallets/bob.wallet"), root.join("genesis.json"));
        let balance = |file: &std::path::Path| super::overview(file, &genesis).unwrap().0.units.unwrap();
        let (alice_before, bob_before) = (balance(&alice), balance(&bob));
        let fees = orien_net::keys::load_genesis(&genesis).unwrap().fee;

        let client = super::connect(&genesis).unwrap();
        let mut wallet = super::open_signing(&alice, None).unwrap();
        let to = super::peek(&bob).unwrap().0;
        let amount = 250_000_000; // 0.25 coin
        let sent = super::send::send(&client, &mut wallet, fees, to, amount).unwrap();
        assert!(!sent.earlier);
        assert_eq!(sent.fee, fees.on(&orien_ledger::Kind::Payment, amount));
        println!("sent {} with a fee of {} in {:.3} s", sent.amount_text, sent.fee_text, sent.seconds);

        let (alice_after, bob_after) = (balance(&alice), balance(&bob));
        assert_eq!(alice_after, alice_before - amount as i128 - sent.fee as i128);
        assert_eq!(bob_after, bob_before + amount as i128);

        // Bob's history names the payment, and who sent it.
        let alice_id = super::peek(&alice).unwrap().0;
        let page = client.history(&to, 0, orien_net::history::MAX_ENTRIES).expect("validators keep history");
        assert!(
            page.entries.iter().any(|item| item.income == orien_ledger::Income::Payment
                && item.amount == amount
                && item.counterparty == Some(alice_id)),
            "the payment is in bob's history"
        );
    }
}
