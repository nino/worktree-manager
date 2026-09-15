//! Worktree Manager executable. This is the only place that knows which
//! platform backend exists: it builds the backend, hands it to the core, and
//! starts the backend's event loop.

use std::sync::Arc;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    #[cfg(target_os = "macos")]
    {
        let dirs = wtm_macos::app_dirs();
        let app = wtm_core::App::new(Arc::new(wtm_macos::MacPlatform), dirs);
        wtm_macos::run(app);
    }

    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("Worktree Manager: no UI backend for this platform yet (see rust/README.md).");
        std::process::exit(1);
    }
}
