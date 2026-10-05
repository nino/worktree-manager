//! The Linux backend: `wtm_toolkit`'s vocabulary shown with GTK 4, and the
//! Linux `Platform` (terminals, the file manager, XDG directories).
//!
//! [`Gtk`] is the [`Toolkit`]: it owns the GTK application and its main loop,
//! and hands the program a backend that keeps one widget per list row,
//! dialog and panel, patched in place on every render. The layout and look
//! follow the AppKit backend's cards as closely as GTK's theme allows; see
//! `style.rs` for the colours.
//!
//! Only Linux builds any of this: the crate is empty elsewhere, so the
//! workspace still builds and tests on macOS and Windows.

#![cfg(target_os = "linux")]

mod backend;
mod dialogs;
mod draw;
mod elements;
mod list;
mod menus;
mod panels;
mod platform;
mod popover;
mod rich;
mod rows;
mod screenshot;
mod style;
mod util;

#[doc(hidden)]
pub mod testing;

/// For the conformance tests, which look at the widgets themselves.
#[doc(hidden)]
pub use gtk;

pub use platform::{app_dirs, LinuxPlatform};

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gdk, gio, glib};

use wtm_toolkit::{Backend, Frame, Poster, Program, Runtime, Toolkit};

use backend::GtkBackend;

/// The GTK application id: the bundle id the other platforms use.
pub const APP_ID: &str = "uk.org.plinth.worktree-manager";

/// The GTK 4 toolkit. `wtm_ui::run(Gtk, app, updater)` shows the app with it.
pub struct Gtk;

impl Toolkit for Gtk {
    fn run<P: Program>(self, program: P) -> ! {
        // A sandboxed run (`WTM_USER_DATA`) must never hand itself over to
        // a copy already running against the real profile.
        let flags = if std::env::var_os("WTM_USER_DATA").is_some() {
            gio::ApplicationFlags::NON_UNIQUE
        } else {
            gio::ApplicationFlags::empty()
        };
        let app = gtk::Application::builder()
            .application_id(APP_ID)
            .flags(flags)
            .build();
        let program = Cell::new(Some(program));
        // The runtime lives as long as the application; held type-erased.
        let runtime: Rc<RefCell<Option<Rc<dyn Any>>>> = Rc::default();
        app.connect_activate(move |app| {
            let Some(program) = program.take() else {
                // Launched again: bring the window forward.
                if let Some(w) = app
                    .active_window()
                    .or_else(|| app.windows().into_iter().next())
                {
                    w.present();
                }
                return;
            };
            style::install();
            style::init_appearance();
            let backend = Rc::new(GtkBackend::new(Some(app.clone())));
            let b = backend.clone();
            let rt = Runtime::new(program, poster(), move || b as Rc<dyn Backend>);
            rt.start(screens());
            *runtime.borrow_mut() = Some(rt as Rc<dyn Any>);
            // The window comes with the first render, a turn after this
            // returns, and an application with no window and no hold exits
            // when `activate` does. Held for good: quitting is explicit.
            std::mem::forget(app.hold());
        });
        let code = app.run_with_args::<&str>(&[]);
        wtm_toolkit::flush();
        std::process::exit(code.get() as i32)
    }
}

/// Runs closures on the main loop, from any thread, always on a later turn:
/// `MainContext::invoke` would run one posted from the main thread at once,
/// inside the native callback that posted it.
fn poster() -> Poster {
    Arc::new(|f| {
        glib::timeout_add_once(Duration::ZERO, f);
    })
}

/// The monitors' areas. GTK 4 offers no work area (the screen less panels)
/// on every backend, so these are the whole monitors.
fn screens() -> Vec<Frame> {
    let Some(display) = gdk::Display::default() else {
        return Vec::new();
    };
    let monitors = display.monitors();
    (0..monitors.n_items())
        .filter_map(|i| monitors.item(i))
        .filter_map(|o| o.downcast::<gdk::Monitor>().ok())
        .map(|m| {
            let g = m.geometry();
            Frame {
                x: g.x() as f64,
                y: g.y() as f64,
                width: g.width() as f64,
                height: g.height() as f64,
            }
        })
        .collect()
}
