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
///
/// The child gets a session of its own: started from a terminal, the app's
/// children would otherwise share its controlling terminal and be hung up
/// with it when that terminal closes, taking an editor or a terminal window
/// the user opened from the app with them.
fn spawn(mut cmd: Command) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    extern "C" {
        fn setsid() -> i32;
    }
    // SAFETY: `setsid` is async-signal-safe, the only kind of call allowed
    // between fork and exec, and touches nothing of this process. It fails
    // only for a process group leader, which a fresh child never is.
    unsafe {
        cmd.pre_exec(|| {
            setsid();
            Ok(())
        });
    }
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
        for argv in terminal_candidates(env.as_deref()) {
            let Some((program, base)) = candidate_program(&argv[0]) else {
                continue;
            };
            let resolved = resolved_name(&program).unwrap_or(base);
            let mut args = terminal_args(&resolved, path);
            // `--dir` is recent in xdg-terminal-exec, and an older one would
            // take it for the command to run. Without it the terminal starts
            // in the working directory it inherits.
            if terminal_kind(&resolved) == Some("xdg-terminal-exec")
                && !std::fs::read(&program).is_ok_and(|b| mentions_dir_option(&b))
            {
                args.clear();
            }
            let mut cmd = Command::new(&program);
            cmd.args(&argv[1..]);
            cmd.args(args);
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
        "“Open in terminal” uses $TERMINAL when it is set, else the desktop's default terminal \
         (xdg-terminal-exec, then x-terminal-emulator), else the first common terminal installed."
    }

    fn file_manager_name(&self) -> &'static str {
        "Files"
    }
}

// MARK: Choosing a terminal

/// Terminals tried after `$TERMINAL` and the desktop's default, roughly by
/// how likely a desktop is to ship them.
pub const KNOWN_TERMINALS: [&str; 12] = [
    "gnome-terminal",
    "ptyxis",
    "kgx",
    "konsole",
    "xfce4-terminal",
    "mate-terminal",
    "tilix",
    "kitty",
    "alacritty",
    "foot",
    "wezterm",
    "xterm",
];

/// The desktop's own choice of terminal: the freedesktop proposal's
/// launcher, then Debian's alternative.
const DEFAULT_TERMINALS: [&str; 2] = ["xdg-terminal-exec", "x-terminal-emulator"];

/// What to try, in order, each as a program and its arguments: the user's
/// `$TERMINAL` (which may carry arguments of its own, quoted as the shell
/// would), the desktop's default, then the known ones. A `$TERMINAL` whose
/// quotes do not close is skipped rather than guessed at.
pub fn terminal_candidates(env_terminal: Option<&str>) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = Vec::new();
    if let Some(argv) = env_terminal.and_then(shell_words).filter(|w| !w.is_empty()) {
        out.push(argv);
    }
    for t in DEFAULT_TERMINALS.iter().chain(KNOWN_TERMINALS.iter()) {
        if !out.iter().any(|c| c.len() == 1 && c[0] == *t) {
            out.push(vec![t.to_string()]);
        }
    }
    out
}

/// `s` split into words as a POSIX shell would, quotes and backslashes
/// included, without expanding anything. `None` when a quote is left open.
pub fn shell_words(s: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut word = String::new();
    // A word can be empty (`''`), so whether one is under way is kept apart
    // from what it holds.
    let mut in_word = false;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' | '\n' => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            '\'' => {
                in_word = true;
                loop {
                    match chars.next()? {
                        '\'' => break,
                        c => word.push(c),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next()? {
                        '"' => break,
                        // Inside double quotes a backslash escapes only
                        // these; before anything else it stands for itself.
                        '\\' => match chars.next()? {
                            c @ ('"' | '\\' | '$' | '`') => word.push(c),
                            '\n' => {}
                            c => {
                                word.push('\\');
                                word.push(c);
                            }
                        },
                        c => word.push(c),
                    }
                }
            }
            '\\' => {
                in_word = true;
                match chars.next() {
                    Some('\n') => {}
                    Some(c) => word.push(c),
                    None => word.push('\\'),
                }
            }
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    if in_word {
        words.push(word);
    }
    Some(words)
}

/// Whether an `xdg-terminal-exec` (usually a shell script) knows `--dir`.
fn mentions_dir_option(program: &[u8]) -> bool {
    program.windows(5).any(|w| w == b"--dir")
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
        Some("xdg-terminal-exec") => flag_eq("--dir="),
        Some("gnome-terminal" | "kgx" | "xfce4-terminal" | "mate-terminal" | "foot") => {
            flag_eq("--working-directory=")
        }
        // Without `--new-window` a running Ptyxis adds a tab to a window
        // that may be on another workspace.
        Some("ptyxis") => {
            let mut args = vec![OsString::from("--new-window")];
            args.extend(flag_eq("--working-directory="));
            args
        }
        Some("konsole") => flag_sep("--workdir"),
        Some("tilix") => flag_sep("-w"),
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
    DEFAULT_TERMINALS[..1]
        .iter()
        .chain(KNOWN_TERMINALS.iter())
        .copied()
        .find(|t| *t == base)
}

/// The program a candidate names found on `PATH`, with its file name.
fn candidate_program(first: &str) -> Option<(PathBuf, String)> {
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
        assert_eq!(args("xdg-terminal-exec"), ["--dir=/w/my repo"]);
        assert_eq!(
            args("ptyxis"),
            ["--new-window", "--working-directory=/w/my repo"]
        );
        assert_eq!(args("kgx"), ["--working-directory=/w/my repo"]);
        assert_eq!(args("tilix"), ["-w", "/w/my repo"]);
        assert_eq!(args("mate-terminal"), ["--working-directory=/w/my repo"]);
        // No flag: started in the folder instead.
        assert!(args("xterm").is_empty());
        assert!(args("st").is_empty());
        // Unresolved, Debian's alternative is a terminal of unknown flags.
        assert!(args("x-terminal-emulator").is_empty());
    }

    #[test]
    fn an_old_xdg_terminal_exec_gets_no_dir_option() {
        assert!(mentions_dir_option(
            b"#!/bin/sh\n  --dir=*) dir=${1#--dir=} ;;"
        ));
        assert!(!mentions_dir_option(b"#!/bin/sh\nexec \"$@\""));
    }

    fn joined(c: &[Vec<String>]) -> Vec<String> {
        c.iter().map(|w| w.join(" ")).collect()
    }

    #[test]
    fn terminal_env_comes_first_then_the_default_and_nothing_twice() {
        let all = DEFAULT_TERMINALS.len() + KNOWN_TERMINALS.len();
        let c = joined(&terminal_candidates(Some("kitty")));
        assert_eq!(
            c[..3],
            ["kitty", "xdg-terminal-exec", "x-terminal-emulator"]
        );
        assert_eq!(c.iter().filter(|t| *t == "kitty").count(), 1);
        assert_eq!(c.len(), all);
        let c = joined(&terminal_candidates(Some("  ")));
        assert_eq!(c[0], "xdg-terminal-exec");
        assert_eq!(terminal_candidates(None).len(), all);
        // Arguments of its own, quoted as in a shell.
        let c = terminal_candidates(Some(r#"'/opt/My Term/term' --class "dev box""#));
        assert_eq!(c[0], ["/opt/My Term/term", "--class", "dev box"]);
        assert_eq!(c.len(), all + 1);
        // A quote left open: skipped, not guessed at.
        let c = joined(&terminal_candidates(Some("'kitty")));
        assert_eq!(c[0], "xdg-terminal-exec");
    }

    #[test]
    fn terminal_words_are_split_as_the_shell_does() {
        let w = |s: &str| shell_words(s);
        assert_eq!(w("a  b\tc").unwrap(), ["a", "b", "c"]);
        assert_eq!(w(r#"a\ b 'c d' "e f""#).unwrap(), ["a b", "c d", "e f"]);
        assert_eq!(w(r#"x'y'"z""#).unwrap(), ["xyz"]);
        assert_eq!(w("'' a").unwrap(), ["", "a"]);
        assert_eq!(w(r#"'\n' "\"\$\q""#).unwrap(), [r"\n", r#""$\q"#]);
        assert!(w("").unwrap().is_empty());
        assert_eq!(w("\"open"), None);
        assert_eq!(w("'open"), None);
    }

    /// The session id in a `/proc/<pid>/stat` line (after the command name,
    /// which may hold spaces).
    fn session_of(stat: &str) -> String {
        let rest = &stat[stat.rfind(')').unwrap() + 2..];
        rest.split(' ').nth(3).unwrap().to_string()
    }

    #[test]
    fn launched_programs_get_a_session_of_their_own() {
        let out = std::env::temp_dir().join(format!("wtm-gtk-sid-{}", std::process::id()));
        let _ = std::fs::remove_file(&out);
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "cat /proc/$$/stat > \"$0\"", out.to_str().unwrap()]);
        spawn(cmd).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let child = loop {
            match std::fs::read_to_string(&out) {
                Ok(s) if !s.is_empty() => break s,
                _ if std::time::Instant::now() > deadline => panic!("the child never ran"),
                _ => std::thread::sleep(std::time::Duration::from_millis(20)),
            }
        };
        let _ = std::fs::remove_file(&out);
        let ours = std::fs::read_to_string("/proc/self/stat").unwrap();
        let pid = child.split(' ').next().unwrap().to_string();
        assert_ne!(session_of(&child), session_of(&ours));
        assert_eq!(session_of(&child), pid, "the child leads its session");
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
