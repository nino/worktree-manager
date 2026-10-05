//! Linux implementations of the `wtm-platform` traits, plus the standard
//! directories. Nothing here touches GTK; it is plain process spawning, and
//! the choices (which terminal, which flag) are pure functions tested below.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;

use log::info;
use wtm_platform::{AppDirs, Platform};

pub struct LinuxPlatform;

/// `WTM_NO_LAUNCH=1` (dev only) logs what would be opened instead of opening
/// it, so a test run pressing row buttons never leaves terminals and file
/// managers behind.
fn launching_disabled(what: &str) -> bool {
    static DISABLED: OnceLock<bool> = OnceLock::new();
    let disabled = *DISABLED
        .get_or_init(|| std::env::var("WTM_NO_LAUNCH").is_ok_and(|v| !v.is_empty() && v != "0"));
    if disabled {
        info!("WTM_NO_LAUNCH: not opening {what}");
    }
    disabled
}

/// Start `cmd` with nothing attached, and reap it on a thread of its own: the
/// caller must never wait on what it launched, and an unreaped child would
/// stay a zombie for as long as the app runs.
fn spawn(mut cmd: Command) -> std::io::Result<()> {
    let child: Child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    std::thread::Builder::new()
        .name("wtm-reap".into())
        .spawn(move || {
            let mut child = child;
            let _ = child.wait();
        })?;
    Ok(())
}

impl Platform for LinuxPlatform {
    /// Through a login shell, so the command sees the `PATH` the user set up
    /// in their profile: an app started from the desktop gets a bare one.
    fn spawn_detached(&self, command_line: &str, cwd: Option<&Path>) -> std::io::Result<()> {
        if launching_disabled(command_line) {
            return Ok(());
        }
        let shell = std::env::var("SHELL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "/bin/sh".into());
        let mut cmd = Command::new(shell);
        cmd.args(["-lc", command_line]);
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        spawn(cmd)
    }

    fn open_in_terminal(&self, path: &Path) -> std::io::Result<()> {
        if launching_disabled(&format!("a terminal in {}", path.display())) {
            return Ok(());
        }
        let env = std::env::var("TERMINAL").ok();
        let mut last = std::io::Error::new(std::io::ErrorKind::NotFound, "no terminal found");
        for candidate in terminal_candidates(env.as_deref()) {
            let Some((program, base)) = candidate_program(&candidate) else {
                continue;
            };
            let resolved = resolved_name(&program).unwrap_or(base);
            let mut cmd = Command::new(&program);
            cmd.args(candidate.split_whitespace().skip(1));
            cmd.args(terminal_args(&resolved, path));
            // Every terminal starts its shell in its own working directory
            // when it has no flag for one (xterm), and it costs nothing for
            // the ones that do.
            cmd.current_dir(path);
            match spawn(cmd) {
                Ok(()) => return Ok(()),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    fn reveal(&self, path: &Path) -> std::io::Result<()> {
        if launching_disabled(&format!("{} in Files", path.display())) {
            return Ok(());
        }
        // xdg-open shows a folder in the file manager but would open a file
        // in its editor, so a file is revealed by its folder.
        let target = if path.is_dir() {
            path.to_path_buf()
        } else {
            path.parent().unwrap_or(path).to_path_buf()
        };
        let mut cmd = Command::new("xdg-open");
        cmd.arg(target);
        spawn(cmd)
    }

    fn terminal_note(&self) -> &'static str {
        "“Open in terminal” uses $TERMINAL when it is set, else the system's default terminal."
    }

    fn file_manager_name(&self) -> &'static str {
        "Files"
    }
}

// MARK: Choosing a terminal

/// Terminals tried after `$TERMINAL` and the Debian alternative, roughly by
/// how likely a desktop is to ship them.
pub const KNOWN_TERMINALS: [&str; 8] = [
    "gnome-terminal",
    "konsole",
    "xfce4-terminal",
    "kitty",
    "alacritty",
    "foot",
    "wezterm",
    "xterm",
];

/// What to try, in order: the user's `$TERMINAL` (which may carry its own
/// arguments), the system's `x-terminal-emulator`, then the known ones.
pub fn terminal_candidates(env_terminal: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(t) = env_terminal.map(str::trim).filter(|t| !t.is_empty()) {
        out.push(t.to_string());
    }
    out.push("x-terminal-emulator".into());
    for t in KNOWN_TERMINALS {
        if !out.iter().any(|c| c == t) {
            out.push(t.into());
        }
    }
    out
}

/// The arguments that open `name` (a program's file name) with its shell in
/// `dir`. Unknown terminals get none and rely on the working directory they
/// are started in.
pub fn terminal_args(name: &str, dir: &Path) -> Vec<OsString> {
    let flag_eq = |flag: &str| {
        let mut s = OsString::from(flag);
        s.push(dir.as_os_str());
        vec![s]
    };
    let flag_sep = |flag: &str| vec![OsString::from(flag), dir.as_os_str().to_owned()];
    match terminal_kind(name) {
        Some("gnome-terminal") => flag_eq("--working-directory="),
        Some("xfce4-terminal") => flag_eq("--working-directory="),
        Some("foot") => flag_eq("--working-directory="),
        Some("konsole") => flag_sep("--workdir"),
        Some("kitty") => flag_sep("--directory"),
        Some("alacritty") => flag_sep("--working-directory"),
        Some("wezterm") => {
            let mut args = vec![OsString::from("start")];
            args.extend(flag_sep("--cwd"));
            args
        }
        _ => Vec::new(),
    }
}

/// Which known terminal a program file is, allowing for the wrappers
/// distributions install (`gnome-terminal.wrapper`, `wezterm-gui`).
fn terminal_kind(name: &str) -> Option<&'static str> {
    let base = name.rsplit('/').next().unwrap_or(name);
    let base = base.strip_suffix(".wrapper").unwrap_or(base);
    let base = base.strip_suffix(".real").unwrap_or(base);
    if base == "wezterm-gui" {
        return Some("wezterm");
    }
    KNOWN_TERMINALS.iter().copied().find(|t| *t == base)
}

/// The program a candidate names (its first word) found on `PATH`, with its
/// file name.
fn candidate_program(candidate: &str) -> Option<(PathBuf, String)> {
    let first = candidate.split_whitespace().next()?;
    let program = find_on_path(first, std::env::var_os("PATH").as_deref())?;
    let base = Path::new(first)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Some((program, base))
}

/// The real program behind a symlink chain (`x-terminal-emulator` →
/// `/etc/alternatives/…` → `gnome-terminal.wrapper`), so its flags are known.
fn resolved_name(program: &Path) -> Option<String> {
    let real = std::fs::canonicalize(program).ok()?;
    Some(real.file_name()?.to_string_lossy().into_owned())
}

/// `name` as the shell would find it: as given when it has a slash, else the
/// first executable of that name on `path`.
pub fn find_on_path(name: &str, path: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    if name.contains('/') {
        let p = PathBuf::from(name);
        return is_executable(&p).then_some(p);
    }
    std::env::split_paths(path?)
        .map(|dir| dir.join(name))
        .find(|p| is_executable(p))
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

// MARK: Directories

/// Where the config lives: `WTM_USER_DATA` for a sandboxed run, else
/// `$XDG_CONFIG_HOME/worktree-manager` (`~/.config/worktree-manager`).
pub fn app_dirs() -> AppDirs {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| "/".into()));
    dirs_from(
        home,
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        std::env::var_os("WTM_USER_DATA").map(PathBuf::from),
    )
}

/// [`app_dirs`] from its inputs.
pub fn dirs_from(home: PathBuf, xdg_config: Option<PathBuf>, sandbox: Option<PathBuf>) -> AppDirs {
    // The XDG spec says a relative value is invalid and to be ignored.
    let config_home = xdg_config
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".config"));
    // A sandboxed run looks for the Electron file inside the sandbox only:
    // reading the real one would fill an empty sandbox with the real repos,
    // whose worktrees a test run could then delete.
    let legacy = match &sandbox {
        Some(dir) => dir.join("worktree-manager.json"),
        None => home.join(".config/worktree-manager/worktree-manager.json"),
    };
    AppDirs {
        config_dir: sandbox.unwrap_or_else(|| config_home.join("worktree-manager")),
        home,
        legacy_config_files: vec![legacy],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(name: &str) -> Vec<String> {
        terminal_args(name, Path::new("/w/my repo"))
            .into_iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn each_terminal_gets_its_own_working_directory_flag() {
        assert_eq!(args("gnome-terminal"), ["--working-directory=/w/my repo"]);
        assert_eq!(
            args("gnome-terminal.wrapper"),
            ["--working-directory=/w/my repo"]
        );
        assert_eq!(args("konsole"), ["--workdir", "/w/my repo"]);
        assert_eq!(args("kitty"), ["--directory", "/w/my repo"]);
        assert_eq!(args("alacritty"), ["--working-directory", "/w/my repo"]);
        assert_eq!(args("foot"), ["--working-directory=/w/my repo"]);
        assert_eq!(args("/usr/bin/wezterm"), ["start", "--cwd", "/w/my repo"]);
        assert_eq!(args("wezterm-gui"), ["start", "--cwd", "/w/my repo"]);
        // No flag: started in the folder instead.
        assert!(args("xterm").is_empty());
        assert!(args("st").is_empty());
    }

    #[test]
    fn terminal_env_comes_first_and_nothing_twice() {
        let c = terminal_candidates(Some("kitty"));
        assert_eq!(c[0], "kitty");
        assert_eq!(c[1], "x-terminal-emulator");
        assert_eq!(c.iter().filter(|t| *t == "kitty").count(), 1);
        assert_eq!(c.len(), 1 + KNOWN_TERMINALS.len());
        let c = terminal_candidates(Some("  "));
        assert_eq!(c[0], "x-terminal-emulator");
        assert_eq!(terminal_candidates(None).len(), 1 + KNOWN_TERMINALS.len());
    }

    #[test]
    fn finds_programs_on_the_path() {
        let dir = std::env::temp_dir().join(format!("wtm-gtk-path-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("my-term");
        std::fs::write(&exe, "#!/bin/sh\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let plain = dir.join("not-exec");
        std::fs::write(&plain, "").unwrap();
        let path = std::env::join_paths([Path::new("/nonexistent"), &dir]).unwrap();
        assert_eq!(find_on_path("my-term", Some(&path)), Some(exe.clone()));
        assert_eq!(find_on_path("not-exec", Some(&path)), None);
        assert_eq!(find_on_path("absent", Some(&path)), None);
        assert_eq!(find_on_path(exe.to_str().unwrap(), None), Some(exe.clone()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn config_follows_xdg_unless_sandboxed() {
        let home = PathBuf::from("/home/ada");
        let d = dirs_from(home.clone(), None, None);
        assert_eq!(
            d.config_dir,
            Path::new("/home/ada/.config/worktree-manager")
        );
        assert_eq!(
            d.legacy_config_files,
            [Path::new(
                "/home/ada/.config/worktree-manager/worktree-manager.json"
            )]
        );
        let d = dirs_from(home.clone(), Some("/xdg".into()), None);
        assert_eq!(d.config_dir, Path::new("/xdg/worktree-manager"));
        // A relative XDG_CONFIG_HOME is invalid and ignored.
        let d = dirs_from(home.clone(), Some("rel".into()), None);
        assert_eq!(
            d.config_dir,
            Path::new("/home/ada/.config/worktree-manager")
        );
        assert_eq!(d.home, home);
    }

    #[test]
    fn a_sandbox_never_reads_the_real_profile() {
        let d = dirs_from(
            "/home/ada".into(),
            Some("/xdg".into()),
            Some("/tmp/sandbox".into()),
        );
        assert_eq!(d.config_dir, Path::new("/tmp/sandbox"));
        assert_eq!(
            d.legacy_config_files,
            [Path::new("/tmp/sandbox/worktree-manager.json")]
        );
    }
}
