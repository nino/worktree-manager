//! `wtm_platform::Platform` for Windows: plain process spawning, through
//! `cmd.exe` for the user's own command lines. The quoting and the command
//! lines themselves are in `shell.rs`, where they are tested on every OS.

use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use log::info;
use wtm_platform::{AppDirs, Platform};

use crate::shell;

/// No console window for a program started from this GUI process: `cmd`
/// would otherwise flash one up for every editor launch.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// A console of its own, for the terminal fallback.
const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
/// Not killed with this process's console group, and not waited on.
const DETACHED_PROCESS_GROUP: u32 = 0x0000_0200;

pub struct WindowsPlatform;

/// `WTM_NO_LAUNCH=1` (dev only) logs what would be opened instead of
/// opening it, as on macOS: a monkey run presses Open in Terminal, Reveal
/// and Open in Editor at random, and each would leave a window behind.
fn launching_disabled(what: &str) -> bool {
    static DISABLED: OnceLock<bool> = OnceLock::new();
    let disabled = *DISABLED
        .get_or_init(|| shell::launching_disabled(std::env::var("WTM_NO_LAUNCH").ok().as_deref()));
    if disabled {
        info!("WTM_NO_LAUNCH: not opening {what}");
    }
    disabled
}

fn quiet(mut c: Command) -> Command {
    c.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    c
}

impl Platform for WindowsPlatform {
    fn spawn_detached(&self, command_line: &str, cwd: Option<&Path>) -> std::io::Result<()> {
        if launching_disabled(command_line) {
            return Ok(());
        }
        let mut c = quiet(Command::new("cmd.exe"));
        // `raw_arg`: the line goes to `cmd` exactly as `cmd_args` built it;
        // Rust's own quoting follows the C runtime's rules, which `cmd`
        // does not.
        c.raw_arg(shell::cmd_args(command_line))
            .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS_GROUP);
        if let Some(dir) = cwd {
            c.current_dir(dir);
        }
        // The child is not waited for; dropping the handle leaves it running.
        c.spawn().map(|_| ())
    }

    fn quote(&self, value: &str) -> String {
        shell::quote(value)
    }

    /// Windows Terminal when it is installed (`wt` on PATH: its app
    /// execution alias), else a `cmd` console in the folder.
    fn open_in_terminal(&self, path: &Path) -> std::io::Result<()> {
        if launching_disabled(&format!("a terminal in {}", path.display())) {
            return Ok(());
        }
        let path_var = std::env::var("PATH").unwrap_or_default();
        let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
        if let Some(wt) = shell::find_on_path("wt", &path_var, &exts, |p| p.is_file()) {
            let mut c = quiet(Command::new(wt));
            c.args(shell::wt_args(path))
                .creation_flags(DETACHED_PROCESS_GROUP);
            if c.spawn().is_ok() {
                return Ok(());
            }
        }
        let mut c = Command::new("cmd.exe");
        c.arg("/K")
            .current_dir(path)
            .creation_flags(CREATE_NEW_CONSOLE | DETACHED_PROCESS_GROUP);
        c.spawn().map(|_| ())
    }

    fn reveal(&self, path: &Path) -> std::io::Result<()> {
        if launching_disabled(&format!("{} in File Explorer", path.display())) {
            return Ok(());
        }
        // Explorer exits non-zero even when it opened the folder, so its
        // status says nothing; it is not waited for.
        quiet(Command::new("explorer.exe"))
            .arg(shell::native_path(path))
            .spawn()
            .map(|_| ())
    }

    fn terminal_note(&self) -> &'static str {
        "“Open in terminal” uses Windows Terminal when it is installed, else a Command Prompt."
    }

    fn file_manager_name(&self) -> &'static str {
        "File Explorer"
    }

    fn configure_git(&self, command: &mut Command) {
        // A GUI process has no console, so each console program it starts
        // would get a window of its own: one flash per git call.
        command.creation_flags(CREATE_NO_WINDOW);
    }
}

/// `%APPDATA%\Worktree Manager`, or `WTM_USER_DATA`; see `shell::app_dirs_from`.
pub fn app_dirs() -> AppDirs {
    shell::app_dirs_from(|name| std::env::var(name).ok())
}
