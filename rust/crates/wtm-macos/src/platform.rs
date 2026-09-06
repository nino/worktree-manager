//! macOS implementations of the `wtm-platform` traits, plus the standard
//! directories. Nothing here touches AppKit; it is plain process spawning.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use wtm_platform::{AppDirs, Platform};

/// Bundle id macOS uses when no default terminal override has been set.
pub const DEFAULT_TERMINAL_BUNDLE_ID: &str = "com.apple.Terminal";

pub struct MacPlatform;

impl Platform for MacPlatform {
    fn spawn_detached(&self, command_line: &str) -> std::io::Result<()> {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        Command::new(shell)
            .args(["-lc", command_line])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
    }

    /// Hand the folder to the default terminal's bundle via `open -b`
    /// (without `-n`, so a running instance is reused). Opening a directory
    /// makes the terminal start a shell there.
    fn open_in_terminal(&self, path: &Path) -> std::io::Result<()> {
        Command::new("open")
            .arg("-b")
            .arg(default_terminal_bundle_id())
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
    }

    fn reveal(&self, path: &Path) -> std::io::Result<()> {
        Command::new("open")
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
    }

    fn file_manager_name(&self) -> &'static str {
        "Finder"
    }
}

/// `~/Library/Application Support/Worktree Manager`, importing the Electron
/// app's electron-store file on first launch.
pub fn app_dirs() -> AppDirs {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()));
    let config_dir = std::env::var_os("WTM_USER_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("Library/Application Support/Worktree Manager"));
    let legacy = home.join("Library/Application Support/worktree-manager/worktree-manager.json");
    AppDirs {
        config_dir,
        home,
        legacy_config_files: vec![legacy],
    }
}

/// Find the bundle id of the user's system-wide default terminal.
///
/// There is no dedicated "default terminal" preference; apps like Ghostty and
/// iTerm register themselves as the Launch Services handler for the
/// `public.unix-executable` content type when you click "Set as default
/// terminal". The LS database is read via `plutil` as JSON.
fn default_terminal_bundle_id() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let plist = format!(
        "{home}/Library/Preferences/com.apple.LaunchServices/com.apple.launchservices.secure.plist"
    );
    let out = Command::new("plutil")
        .args(["-convert", "json", "-o", "-", &plist])
        .output();
    if let Ok(out) = out {
        if out.status.success() {
            if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&out.stdout) {
                if let Some(id) = parse_default_terminal_bundle_id(&json) {
                    return id;
                }
            }
        }
    }
    DEFAULT_TERMINAL_BUNDLE_ID.to_string()
}

/// The `LSHandlers` entry for `public.unix-executable`, preferring the shell
/// role. `None` when no override exists ("-" means "no app").
pub fn parse_default_terminal_bundle_id(json: &serde_json::Value) -> Option<String> {
    let handlers = json.get("LSHandlers")?.as_array()?;
    for h in handlers {
        if h.get("LSHandlerContentType").and_then(|v| v.as_str()) != Some("public.unix-executable")
        {
            continue;
        }
        let id = ["LSHandlerRoleShell", "LSHandlerRoleAll"]
            .iter()
            .filter_map(|k| h.get(k).and_then(|v| v.as_str()))
            .find(|v| !v.is_empty());
        if let Some(id) = id {
            if id != "-" {
                return Some(id.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn returns_role_all_handler() {
        let j = json!({"LSHandlers": [
            {"LSHandlerContentType": "public.html", "LSHandlerRoleAll": "com.example.browser"},
            {"LSHandlerContentType": "public.unix-executable", "LSHandlerRoleAll": "com.mitchellh.ghostty"}
        ]});
        assert_eq!(
            parse_default_terminal_bundle_id(&j).as_deref(),
            Some("com.mitchellh.ghostty")
        );
    }

    #[test]
    fn prefers_role_shell() {
        let j = json!({"LSHandlers": [{"LSHandlerContentType": "public.unix-executable", "LSHandlerRoleShell": "com.googlecode.iterm2", "LSHandlerRoleAll": "com.apple.Terminal"}]});
        assert_eq!(
            parse_default_terminal_bundle_id(&j).as_deref(),
            Some("com.googlecode.iterm2")
        );
    }

    #[test]
    fn ignores_placeholder_and_missing() {
        let j = json!({"LSHandlers": [{"LSHandlerContentType": "public.unix-executable", "LSHandlerRoleAll": "-"}]});
        assert_eq!(parse_default_terminal_bundle_id(&j), None);
        assert_eq!(
            parse_default_terminal_bundle_id(&json!({"LSHandlers": []})),
            None
        );
        assert_eq!(parse_default_terminal_bundle_id(&json!({})), None);
    }
}
