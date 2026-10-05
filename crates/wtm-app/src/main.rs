//! Worktree Manager executable. This is the only place that knows which
//! toolkit exists: it builds the platform services and the updater for the
//! OS it was compiled for, and runs the shared UI on that OS's toolkit.

// A release build on Windows is a GUI program, with no console window of its
// own; a debug build keeps the console for the log.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::rc::Rc;
use std::sync::Arc;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    #[cfg(target_os = "macos")]
    {
        let app = wtm_core::App::new(Arc::new(wtm_macos::MacPlatform), wtm_macos::app_dirs());
        // Versions before `ui-state.json` had AppKit keep the window's frame;
        // the first launch after the update opens it there.
        let state = app.ui_state();
        if state.window.is_none() {
            if let Some(frame) = wtm_macos::legacy_autosaved_frame() {
                app.store_ui_state(wtm_core::UiState {
                    window: Some(frame),
                    ..state
                });
            }
        }
        let updater = Rc::new(wtm_macos::MacUpdater::new(app.clone()));
        wtm_ui::run(wtm_macos::AppKit, app, updater);
    }

    #[cfg(target_os = "linux")]
    {
        let app = wtm_core::App::new(Arc::new(wtm_gtk::LinuxPlatform), wtm_gtk::app_dirs());
        // Releases are built for macOS only, so there is nothing to update to.
        wtm_ui::run(wtm_gtk::Gtk, app, Rc::new(wtm_platform::NoUpdater));
    }

    #[cfg(windows)]
    {
        let app = wtm_core::App::new(
            Arc::new(wtm_windows::WindowsPlatform),
            wtm_windows::app_dirs(),
        );
        wtm_ui::run(wtm_windows::Win32, app, Rc::new(wtm_platform::NoUpdater));
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    {
        let _ = (Rc::new(()), Arc::new(()));
        eprintln!("Worktree Manager: no UI backend for this platform yet.");
        std::process::exit(1);
    }
}
