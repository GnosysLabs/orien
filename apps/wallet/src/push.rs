//! iPhone notifications when ORN arrives, even with the app closed.
//!
//! Only Apple can wake an iPhone, so the phone signs up with Apple for a
//! notification address (a "device token") and hands it to the Market
//! server with the addresses of the wallets in this app. The server watches
//! the network and, when one of them is paid, asks Apple to tell the phone
//! "You received 25 ORN". The server learns which addresses share a phone,
//! nothing more: payments are public on the network anyway.
//!
//! Apple gives the token to the app's delegate, an object Tauri owns, so
//! the method Apple calls is added to it here, as the app starts asking.

use std::sync::{Mutex, OnceLock};

use block2::RcBlock;
use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, Sel};
use objc2::{MainThreadMarker, sel};
use objc2_foundation::{NSData, NSError};
use objc2_ui_kit::UIApplication;
use objc2_user_notifications::{UNAuthorizationOptions, UNUserNotificationCenter};
use tauri::AppHandle;

/// The app, to get back to the main thread and find the wallets' folder.
static APP: OnceLock<AppHandle> = OnceLock::new();
/// This phone's token, once Apple has given it.
static TOKEN: Mutex<Option<String>> = Mutex::new(None);
/// What the server was last told, so it is only told again on a change.
static TOLD: Mutex<Option<(String, Vec<String>, bool, Vec<String>)>> = Mutex::new(None);

/// Builds installed from this Mac use Apple's test service; TestFlight and
/// the App Store use the real one. The server must be told which. A build
/// for this Mac's phones carries its signing profile, which says so
/// (aps-environment "development"); TestFlight and App Store builds carry
/// none. Checked, not guessed from debug or release: a release build put on
/// the phone from here is still on the test service.
fn sandbox() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        let Ok(exe) = std::env::current_exe() else { return cfg!(debug_assertions) };
        let Some(bundle) = exe.parent() else { return cfg!(debug_assertions) };
        let Ok(profile) = std::fs::read(bundle.join("embedded.mobileprovision")) else { return false };
        let text = String::from_utf8_lossy(&profile);
        text.find("<key>aps-environment</key>").is_some_and(|at| text[at..].chars().take(120).collect::<String>().contains("development"))
    })
}

/// Asks once whether the app may show notifications (iPhone asks the
/// person the first time only) and, if so, signs this phone up with Apple.
pub fn start(app: &AppHandle) {
    let _ = APP.set(app.clone());
    let center = UNUserNotificationCenter::currentNotificationCenter();
    let done = RcBlock::new(|granted: Bool, _error: *mut NSError| {
        if !granted.as_bool() {
            return;
        }
        if let Some(app) = APP.get() {
            let _ = app.run_on_main_thread(register);
        }
    });
    center.requestAuthorizationWithOptions_completionHandler(
        UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound | UNAuthorizationOptions::Badge,
        &done,
    );
}

/// Tells the server which wallets this phone wants to hear about, if they
/// changed since it was last told. Runs off the main thread.
pub fn sync() {
    let Some(token) = TOKEN.lock().unwrap().clone() else { return };
    let Some(dir) = APP.get().and_then(|app| crate::folder(app).ok()) else { return };
    let addresses = addresses(&dir);
    // Signed in to the Market, the token is tied to the account too, so
    // messages and swap news reach this phone; signed out, it's untied.
    let session = crate::board::session(&dir);
    let mute = crate::muted(&dir);
    let now = Some((token.clone(), addresses.clone(), session.is_some(), mute.clone()));
    if *TOLD.lock().unwrap() == now {
        return;
    }
    let url = format!("{}/api/market/push/register", crate::board::base());
    let body = serde_json::json!({ "token": token, "sandbox": sandbox(), "addresses": addresses, "mute": mute });
    let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(15)).user_agent("orien-wallet").build();
    let mut request = agent.post(&url);
    if let Some(cookie) = &session {
        request = request.set("Cookie", &format!("orien_market={cookie}"));
    }
    if request.send_json(body).is_ok() {
        *TOLD.lock().unwrap() = now;
    }
}

/// Every wallet in the app, by address, at most the 50 the server keeps.
fn addresses(dir: &std::path::Path) -> Vec<String> {
    let mut addresses: Vec<String> = crate::known_wallets(dir)
        .into_iter()
        .filter_map(|w| orien_net::keys::Wallet::peek(&w.path).ok().map(|(address, _)| address.to_string()))
        .collect();
    addresses.sort();
    addresses.dedup();
    addresses.truncate(50);
    addresses
}

/// On the main thread: teaches the app's delegate the two answers Apple
/// gives, then asks Apple for the token.
fn register() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let application = UIApplication::sharedApplication(mtm);
    // SAFETY: the delegate lives as long as the app; only its class is used.
    let Some(delegate) = (unsafe { application.delegate() }) else { return };
    let object: &AnyObject = delegate.as_ref();
    let class: &AnyClass = object.class();
    let got = registered as unsafe extern "C-unwind" fn(&AnyObject, Sel, &AnyObject, &NSData);
    let failed = refused as unsafe extern "C-unwind" fn(&AnyObject, Sel, &AnyObject, &NSError);
    // SAFETY: each function has the method's exact shape, which the type
    // strings describe: no result, the object, the selector, two objects.
    // A delegate that already answers keeps its own answer.
    unsafe {
        let class = class as *const AnyClass as *mut AnyClass;
        objc2::ffi::class_addMethod(
            class,
            sel!(application:didRegisterForRemoteNotificationsWithDeviceToken:),
            std::mem::transmute::<_, Imp>(got),
            c"v@:@@".as_ptr(),
        );
        objc2::ffi::class_addMethod(
            class,
            sel!(application:didFailToRegisterForRemoteNotificationsWithError:),
            std::mem::transmute::<_, Imp>(failed),
            c"v@:@@".as_ptr(),
        );
    }
    application.registerForRemoteNotifications();
}

/// Apple's answer: this phone's token. Keep it and tell the server.
unsafe extern "C-unwind" fn registered(_this: &AnyObject, _cmd: Sel, _application: &AnyObject, token: &NSData) {
    let hex: String = token.to_vec().iter().map(|byte| format!("{byte:02x}")).collect();
    *TOKEN.lock().unwrap() = Some(hex);
    std::thread::spawn(sync);
}

/// Apple couldn't sign the phone up (no network, or no permission in the
/// app's build). The app works as before, without notifications.
unsafe extern "C-unwind" fn refused(_this: &AnyObject, _cmd: Sel, _application: &AnyObject, _error: &NSError) {}
