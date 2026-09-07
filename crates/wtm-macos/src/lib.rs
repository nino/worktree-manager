//! macOS backend for Worktree Manager: AppKit UI driven through `objc2`, plus
//! the platform services (terminal, file manager, directories).
//!
//! Layering: this crate depends on `wtm-core` and `wtm-platform`; nothing in
//! the core depends on it. The executable calls [`run`] with a core `App`.

pub mod badge;
pub mod branchlabel;
pub mod button;
pub mod cells;
pub mod controller;
pub mod dialogs;
pub mod items;
pub mod menu;
pub mod outline;
pub mod picker;
pub mod platform;
pub mod rowview;
pub mod settings;
pub mod toolicon;
pub mod tooltip;
pub mod util;

pub use platform::{app_dirs, MacPlatform};

use objc2::runtime::ProtocolObject;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

/// Run the AppKit event loop with the given core. Never returns.
pub fn run(app: wtm_core::App) -> ! {
    let mtm = MainThreadMarker::new().expect("run() must be called from the main thread");
    let ns_app = NSApplication::sharedApplication(mtm);
    ns_app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    // Development aid: force an appearance to check both looks without
    // flipping the system setting (`WTM_APPEARANCE=dark|light`).
    if let Ok(name) = std::env::var("WTM_APPEARANCE") {
        let name = match name.as_str() {
            "dark" => Some(unsafe { objc2_app_kit::NSAppearanceNameDarkAqua }),
            "light" => Some(unsafe { objc2_app_kit::NSAppearanceNameAqua }),
            _ => None,
        };
        if let Some(a) = name.and_then(objc2_app_kit::NSAppearance::appearanceNamed) {
            ns_app.setAppearance(Some(&a));
        }
    }
    let controller = controller::Controller::new(app, mtm);
    ns_app.setDelegate(Some(ProtocolObject::from_ref(&*controller)));
    ns_app.activate();
    ns_app.run();
    std::process::exit(0)
}
