//! The operating-system seam.
//!
//! Everything the core and the UI need from the host OS is expressed here as
//! traits, and every platform backend (`wtm-macos`, `wtm-gtk`,
//! `wtm-windows`) implements them. Neither the core nor the UI uses
//! `cfg(target_os)`; the executable picks one backend at startup and hands it
//! in. Adding a platform means adding a crate that implements these traits,
//! not touching the core.

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
    /// Run `command_line` detached, through the user's shell so it sees the
    /// interactive `PATH` (GUI launches typically don't), in `cwd` when given.
    fn spawn_detached(&self, command_line: &str, cwd: Option<&Path>) -> std::io::Result<()>;

    /// Quote `value` as one argument for the shell `spawn_detached` runs.
    /// POSIX single quotes unless the platform's shell is something else.
    fn quote(&self, value: &str) -> String {
        posix_quote(value)
    }

    /// Open the user's default terminal with its working directory at `path`.
    fn open_in_terminal(&self, path: &Path) -> std::io::Result<()>;

    /// Reveal `path` in the system file manager.
    fn reveal(&self, path: &Path) -> std::io::Result<()>;

    /// What Settings says about how "Open in terminal" picks a terminal.
    fn terminal_note(&self) -> &'static str {
        "“Open in terminal” uses your default terminal."
    }

    /// Human-readable name of the file manager ("Finder", "Explorer"), for
    /// button captions.
    fn file_manager_name(&self) -> &'static str;

    /// Adjust every git process the core starts before it runs (on
    /// Windows, so that a GUI app's git calls open no console window).
    fn configure_git(&self, _command: &mut std::process::Command) {}
}

/// Quote a string for safe use as one word of a POSIX shell command.
pub fn posix_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// The self-updater, where a platform has one (macOS today). Lives on the
/// main thread; every method returns at once and does its work in the
/// background, telling the UI through the callback given to `start` when
/// something it shows has changed.
pub trait Updater {
    /// Begin the periodic checks. `changed` is called on the main thread
    /// whenever `ready_version` or `pending_version` may have changed.
    fn start(&self, changed: Box<dyn Fn()>);

    /// Check now, saying in the notice bar what was found (the app menu's
    /// "Check for Updates…").
    fn check_now(&self);

    /// The channel in the config changed: read the new channel's feed now.
    fn channel_changed(&self);

    /// A downloaded version is in place and the app only has to restart.
    fn ready_version(&self) -> Option<String>;

    /// A downloaded, checked version waits for an administrator to put it
    /// in place.
    fn pending_version(&self) -> Option<String>;

    /// Restart into the ready version, or install the pending one first.
    fn install_or_restart(&self);

    /// Whether this build updates itself at all; the menu item is left out
    /// where it does not.
    fn available(&self) -> bool {
        true
    }
}

/// For platforms without a self-updater.
pub struct NoUpdater;

impl Updater for NoUpdater {
    fn start(&self, _: Box<dyn Fn()>) {}
    fn check_now(&self) {}
    fn channel_changed(&self) {}
    fn ready_version(&self) -> Option<String> {
        None
    }
    fn pending_version(&self) -> Option<String> {
        None
    }
    fn install_or_restart(&self) {}
    fn available(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::posix_quote;

    #[test]
    fn quotes_single_quotes() {
        assert_eq!(posix_quote("it's"), "'it'\\''s'");
    }
}
