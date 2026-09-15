//! The operating-system seam.
//!
//! Everything the core needs from the host OS is expressed here as traits, and
//! every platform backend (`wtm-macos` today, `wtm-windows`/`wtm-linux` later)
//! implements them. The core never uses `cfg(target_os)`; the executable picks
//! one backend at startup and hands it in. Adding a platform means adding a
//! crate that implements these traits, not touching the core.

use std::path::{Path, PathBuf};

/// Where the app keeps its files. Resolved by the backend, since each OS has
/// its own convention (Application Support, XDG, AppData).
#[derive(Debug, Clone)]
pub struct AppDirs {
    /// Directory for the persisted configuration and the startup snapshot.
    pub config_dir: PathBuf,
    /// The user's home directory, used to abbreviate paths with `~`.
    pub home: PathBuf,
    /// Configuration files of earlier app generations worth importing on first
    /// launch, most preferred first. Missing files are skipped silently.
    pub legacy_config_files: Vec<PathBuf>,
}

/// Actions that reach outside the app: editors, terminals, file managers.
///
/// Implementations must be cheap to call from any thread and must never block
/// on the launched program.
pub trait Platform: Send + Sync + 'static {
    /// Run `command_line` detached, through the user's login shell so it sees
    /// the interactive `PATH` (GUI launches typically don't).
    fn spawn_detached(&self, command_line: &str) -> std::io::Result<()>;

    /// Open the user's default terminal with its working directory at `path`.
    fn open_in_terminal(&self, path: &Path) -> std::io::Result<()>;

    /// Reveal `path` in the system file manager.
    fn reveal(&self, path: &Path) -> std::io::Result<()>;

    /// Human-readable name of the file manager ("Finder", "Explorer"), for
    /// button captions.
    fn file_manager_name(&self) -> &'static str;
}
