//! Taps you can feel, on iPhones. The haptics plugin made a new feedback
//! generator for each tap and fired it at once, before the phone's motor
//! had woken: after a quiet moment the first tap was often lost. Here the
//! generators live as long as the app, are made ready the moment a finger
//! touches the screen, and are made ready again after each tap, as Apple
//! asks. Everything runs on the main thread, where UIKit wants it.

use tauri::AppHandle;

#[cfg(target_os = "ios")]
mod ios {
    use objc2::rc::Retained;
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_ui_kit::{
        UIImpactFeedbackGenerator, UIImpactFeedbackStyle, UINotificationFeedbackGenerator, UINotificationFeedbackType,
        UISelectionFeedbackGenerator,
    };
    use std::cell::RefCell;

    struct Kit {
        light: Retained<UIImpactFeedbackGenerator>,
        medium: Retained<UIImpactFeedbackGenerator>,
        heavy: Retained<UIImpactFeedbackGenerator>,
        select: Retained<UISelectionFeedbackGenerator>,
        note: Retained<UINotificationFeedbackGenerator>,
    }

    thread_local! {
        static KIT: RefCell<Option<Kit>> = const { RefCell::new(None) };
    }

    fn with(f: impl FnOnce(&Kit)) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        KIT.with(|kit| {
            let mut kit = kit.borrow_mut();
            let kit = kit.get_or_insert_with(|| {
                let impact = |style| UIImpactFeedbackGenerator::initWithStyle(UIImpactFeedbackGenerator::alloc(mtm), style);
                Kit {
                    light: impact(UIImpactFeedbackStyle::Light),
                    medium: impact(UIImpactFeedbackStyle::Medium),
                    heavy: impact(UIImpactFeedbackStyle::Heavy),
                    select: UISelectionFeedbackGenerator::new(mtm),
                    note: UINotificationFeedbackGenerator::new(mtm),
                }
            });
            f(kit);
        });
    }

    /// A finger is down: the taps most likely next are made ready.
    pub fn ready() {
        with(|kit| {
            kit.light.prepare();
            kit.select.prepare();
        });
    }

    pub fn feel(kind: &str) {
        with(|kit| match kind {
            "success" => kit.note.notificationOccurred(UINotificationFeedbackType::Success),
            "warning" => kit.note.notificationOccurred(UINotificationFeedbackType::Warning),
            "error" => kit.note.notificationOccurred(UINotificationFeedbackType::Error),
            "select" | "selection" => {
                kit.select.selectionChanged();
                kit.select.prepare();
            }
            "medium" => {
                kit.medium.impactOccurred();
                kit.medium.prepare();
            }
            "heavy" => {
                kit.heavy.impactOccurred();
                kit.heavy.prepare();
            }
            _ => {
                kit.light.impactOccurred();
                kit.light.prepare();
            }
        });
    }
}

/// A tap to feel: light, medium or heavy; select; success, warning or error.
#[tauri::command]
pub fn haptic(app: AppHandle, kind: String) {
    #[cfg(target_os = "ios")]
    let _ = app.run_on_main_thread(move || ios::feel(&kind));
    #[cfg(not(target_os = "ios"))]
    let _ = (app, kind);
}

/// A finger touched the screen: wakes the motor so the tap that follows is felt.
#[tauri::command]
pub fn haptic_ready(app: AppHandle) {
    #[cfg(target_os = "ios")]
    let _ = app.run_on_main_thread(ios::ready);
    #[cfg(not(target_os = "ios"))]
    let _ = app;
}
