//! Worktree Manager executable. This is the only place that knows which
//! toolkit exists: it builds the platform services and the updater for the
//! OS it was compiled for, and runs the shared UI on that OS's toolkit.

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

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (Rc::new(()), Arc::new(()));
        eprintln!("Worktree Manager: no UI backend for this platform yet.");
        std::process::exit(1);
    }
}
