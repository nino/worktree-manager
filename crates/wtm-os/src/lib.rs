//! The `wtm-platform` services for each OS: opening terminals, editors and
//! the file manager, and where the app keeps its files.
//!
//! None of this touches a UI toolkit, so every front end can use it: the
//! native backends re-export it, and the Tauri app, whose webview on Linux
//! is GTK 3 and must never share a process with `wtm-gtk`'s GTK 4, links it
//! without them.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{app_dirs, LinuxPlatform};

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
pub use macos::{app_dirs, MacPlatform};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
mod windows_shell;
#[cfg(windows)]
pub use windows::{app_dirs, WindowsPlatform};

use std::sync::Arc;

use wtm_platform::{AppDirs, Platform};

/// This OS's platform services and directories, for a front end that does
/// not want to name the OS itself.
pub fn native() -> (Arc<dyn Platform>, AppDirs) {
    #[cfg(target_os = "linux")]
    let platform = LinuxPlatform;
    #[cfg(target_os = "macos")]
    let platform = MacPlatform;
    #[cfg(windows)]
    let platform = WindowsPlatform;
    (Arc::new(platform), app_dirs())
}
