//! macOS backend for Worktree Manager: the `wtm-toolkit` vocabulary in
//! AppKit, driven through `objc2`, plus the platform services (terminal,
//! file manager, directories) and the self-updater.
//!
//! Layering: this crate depends on `wtm-toolkit`, `wtm-core` and
//! `wtm-platform`; nothing in them depends on it. The executable runs the
//! shared UI with [`AppKit`].

#![cfg(target_os = "macos")]

pub mod badge;
pub mod branchlabel;
pub mod button;
pub mod cells;
pub mod controller;
pub mod dialogs;
pub mod elements;
pub mod items;
pub mod menu;
pub mod outline;
pub mod picker;
pub mod platform;
pub mod rowview;
pub mod settings;
pub mod toolicon;
pub mod tooltip;
pub mod updater;
pub mod util;

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

pub use controller::legacy_autosaved_frame;
pub use platform::{app_dirs, MacPlatform};
pub use updater::MacUpdater;

use dispatch2::DispatchQueue;
use objc2::runtime::ProtocolObject;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use wtm_toolkit::{Backend, Effect, Poster, Program, Runtime, Toolkit, View};

thread_local! {
    /// The running program, kept for the life of the process.
    static RUNTIME: RefCell<Option<Rc<dyn Any>>> = const { RefCell::new(None) };
}

/// The AppKit toolkit. Owns the main run loop.
pub struct AppKit;

struct MacBackend {
    mtm: MainThreadMarker,
}

impl Backend for MacBackend {
    fn render(&self, view: &View) {
        if let Some(c) = controller::controller(self.mtm) {
            c.render(view);
        }
    }

    fn perform(&self, effect: Effect) {
        if let Some(c) = controller::controller(self.mtm) {
            c.perform(effect);
        }
    }
}

impl Toolkit for AppKit {
    fn run<P: Program>(self, program: P) -> ! {
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
        let post: Poster = Arc::new(|f| DispatchQueue::main().exec_async(f));
        let runtime = Runtime::new(program, post, || Rc::new(MacBackend { mtm }));
        let starting = runtime.clone();
        // The program starts once AppKit has finished launching, which is
        // when windows can be shown and the screens are known.
        let controller = controller::Controller::new(move |screens| starting.start(screens), mtm);
        RUNTIME.with(|r| *r.borrow_mut() = Some(runtime as Rc<dyn Any>));
        ns_app.setDelegate(Some(ProtocolObject::from_ref(&*controller)));
        ns_app.activate();
        ns_app.run();
        std::process::exit(0)
    }
}
