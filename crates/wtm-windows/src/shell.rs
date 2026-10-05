//! The pure half of `WindowsPlatform`: quoting for `cmd.exe` and for the
//! C runtime's argument parser, the command lines the platform runs, PATH
//! lookup, and where the app keeps its files. Nothing here calls Windows, so
//! it is tested on every OS (`tests/shell.rs` includes this file).

use std::path::{Path, PathBuf};

use wtm_platform::AppDirs;

/// Quote `value` as one argument of a `cmd /C` command line: wrapped in
/// double quotes, each inner quote doubled. Backslashes that end the value,
/// or come before a quote, are doubled too: the program at the other end
/// splits its command line by the C runtime's rules, where `\"` is a
/// literal quote and would swallow the closing one (`"C:\dir\"`).
///
/// `%NAME%` is still expanded by `cmd` inside quotes; there is no quoting
/// that stops it on a `/C` line.
pub fn quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    let mut backslashes = 0;
    for c in value.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.extend(std::iter::repeat_n('\\', backslashes * 2));
                backslashes = 0;
                out.push_str("\"\"");
                continue;
            }
            _ => {
                out.extend(std::iter::repeat_n('\\', backslashes));
                backslashes = 0;
            }
        }
        if c != '\\' {
            out.push(c);
        }
    }
    out.extend(std::iter::repeat_n('\\', backslashes * 2));
    out.push('"');
    out
}

/// The arguments after `cmd.exe` that run `command_line` as typed, in
/// `unc_dir` when that is a UNC folder (see [`unc_dir`]). `/S` makes `cmd`
/// strip exactly the outer pair of quotes and keep the rest verbatim,
/// whatever quotes the line itself holds; without it, a line that starts
/// with a quoted program name loses its first and last quote. `/V:OFF`:
/// delayed expansion, if the user's registry turns it on, would read a
/// `!` in a path as the start of a variable.
///
/// `cmd` cannot run in a UNC folder: started in one it warns and runs in
/// the Windows folder instead. `pushd` maps a drive letter to the folder
/// and goes there, and `popd` gives the letter back afterwards, whatever
/// the command did.
pub fn cmd_args(command_line: &str, unc_dir: Option<&str>) -> String {
    match unc_dir {
        Some(dir) => format!("/D /V:OFF /S /C \"pushd \"{dir}\" && {command_line} & popd\""),
        None => format!("/D /V:OFF /S /C \"{command_line}\""),
    }
}

/// `path` as a UNC folder (`\\server\share\…`), or `None` for a folder on
/// a drive, which `cmd` can start in.
pub fn unc_dir(path: &Path) -> Option<String> {
    let p = native_path(path);
    (p.starts_with("\\\\") && !p.starts_with("\\\\?\\")).then_some(p)
}

/// A path as Windows programs expect it: git prints `C:/x/y`, and Explorer
/// takes a forward slash for a switch.
pub fn native_path(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\")
}

/// Explorer's arguments to open the folder holding `path` with `path`
/// selected, as Finder's reveal does. The path is quoted (a comma would
/// otherwise end it), and given raw: Explorer reads its own command line,
/// not the C runtime's way.
pub fn explorer_args(path: &Path) -> String {
    format!("/select,\"{}\"", native_path(path))
}

/// Arguments for Windows Terminal to open a tab in `path`. Its command line
/// takes `;` as "and another tab", so a folder name holding one has it
/// escaped.
pub fn wt_args(path: &Path) -> Vec<String> {
    vec!["-d".into(), native_path(path).replace(';', "\\;")]
}

/// The first `name` on `path_var` (a `;`-separated PATH), trying each of
/// `exts` (from PATHEXT) when `name` has no extension of its own.
pub fn find_on_path(
    name: &str,
    path_var: &str,
    exts: &str,
    exists: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let has_ext = Path::new(name).extension().is_some();
    let exts: Vec<&str> = if has_ext {
        vec![""]
    } else {
        exts.split(';').filter(|e| !e.is_empty()).collect()
    };
    path_var
        .split(';')
        .map(|d| d.trim().trim_matches('"'))
        .filter(|d| !d.is_empty())
        .flat_map(|dir| {
            exts.iter()
                .map(move |ext| Path::new(dir).join(format!("{name}{}", ext.to_lowercase())))
        })
        .find(|p| exists(p))
}

/// `WTM_NO_LAUNCH` set to anything but empty or `0`.
pub fn launching_disabled(value: Option<&str>) -> bool {
    value.is_some_and(|v| !v.is_empty() && v != "0")
}

/// Where the app keeps its files, from the environment `var` reads:
/// `%APPDATA%\Worktree Manager`, or `WTM_USER_DATA` for a sandboxed run.
/// A sandboxed run looks for the Electron app's file inside the sandbox
/// only: reading the real one would fill an empty sandbox with the real
/// repos, whose worktrees a test run can then delete.
pub fn app_dirs_from(var: impl Fn(&str) -> Option<String>) -> AppDirs {
    let home = var("USERPROFILE")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\"));
    let appdata = var("APPDATA")
        .filter(|a| !a.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("AppData").join("Roaming"));
    let sandbox = var("WTM_USER_DATA")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from);
    let legacy = match &sandbox {
        Some(dir) => dir.join("worktree-manager.json"),
        None => appdata
            .join("worktree-manager")
            .join("worktree-manager.json"),
    };
    AppDirs {
        config_dir: sandbox.unwrap_or_else(|| appdata.join("Worktree Manager")),
        home,
        legacy_config_files: vec![legacy],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn quotes_wrap_and_double_inner_quotes() {
        assert_eq!(quote("plain"), "\"plain\"");
        assert_eq!(quote("two words"), "\"two words\"");
        assert_eq!(quote("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(quote(""), "\"\"");
        assert_eq!(quote("50% & <more>"), "\"50% & <more>\"");
    }

    #[test]
    fn backslashes_before_a_quote_are_doubled() {
        assert_eq!(quote("C:\\dir\\"), "\"C:\\dir\\\\\"");
        assert_eq!(quote("C:\\a b\\c"), "\"C:\\a b\\c\"");
        assert_eq!(quote("a\\\"b"), "\"a\\\\\"\"b\"");
    }

    #[test]
    fn cmd_keeps_the_line_verbatim_inside_its_outer_quotes() {
        assert_eq!(
            cmd_args("\"C:\\Program Files\\x.exe\" \"a b\"", None),
            "/D /V:OFF /S /C \"\"C:\\Program Files\\x.exe\" \"a b\"\""
        );
    }

    #[test]
    fn a_unc_folder_is_entered_with_pushd() {
        assert_eq!(
            cmd_args("code .", Some("\\\\srv\\share\\wt")),
            "/D /V:OFF /S /C \"pushd \"\\\\srv\\share\\wt\" && code . & popd\""
        );
        assert_eq!(
            unc_dir(Path::new("//srv/share/wt")).as_deref(),
            Some("\\\\srv\\share\\wt")
        );
        assert_eq!(unc_dir(Path::new("C:/src/app")), None);
        assert_eq!(unc_dir(Path::new("\\\\?\\C:\\src")), None);
    }

    #[test]
    fn explorer_selects_the_path_quoted() {
        assert_eq!(
            explorer_args(Path::new("C:/src/a,b")),
            "/select,\"C:\\src\\a,b\""
        );
    }

    #[test]
    fn paths_get_backslashes_and_terminal_tabs_escape_semicolons() {
        assert_eq!(native_path(Path::new("C:/src/app")), "C:\\src\\app");
        assert_eq!(
            wt_args(Path::new("C:/src/a;b")),
            vec!["-d".to_string(), "C:\\src\\a\\;b".to_string()]
        );
    }

    #[test]
    fn finds_programs_on_the_path_with_their_extension() {
        let there = |p: &Path| p == Path::new("C:\\Tools").join("wt.exe");
        assert_eq!(
            find_on_path("wt", "C:\\Windows;\"C:\\Tools\";", ".COM;.EXE", there),
            Some(Path::new("C:\\Tools").join("wt.exe"))
        );
        assert_eq!(find_on_path("wt", "C:\\Windows", ".EXE", there), None);
        let exact = |p: &Path| p == Path::new("C:\\Tools").join("wt.exe");
        assert!(find_on_path("wt.exe", "C:\\Tools", ".BAT", exact).is_some());
    }

    #[test]
    fn launching_is_disabled_by_any_value_but_zero() {
        assert!(!launching_disabled(None));
        assert!(!launching_disabled(Some("")));
        assert!(!launching_disabled(Some("0")));
        assert!(launching_disabled(Some("1")));
    }

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k| map.get(k).cloned()
    }

    #[test]
    fn app_data_holds_the_config_and_the_electron_file() {
        let d = app_dirs_from(env(&[
            ("APPDATA", "C:\\Users\\ada\\AppData\\Roaming"),
            ("USERPROFILE", "C:\\Users\\ada"),
        ]));
        let roaming = PathBuf::from("C:\\Users\\ada\\AppData\\Roaming");
        assert_eq!(d.config_dir, roaming.join("Worktree Manager"));
        assert_eq!(d.home, PathBuf::from("C:\\Users\\ada"));
        assert_eq!(
            d.legacy_config_files,
            vec![roaming
                .join("worktree-manager")
                .join("worktree-manager.json")]
        );
    }

    #[test]
    fn a_sandbox_never_reads_the_real_profile() {
        let d = app_dirs_from(env(&[
            ("APPDATA", "C:\\Users\\ada\\AppData\\Roaming"),
            ("USERPROFILE", "C:\\Users\\ada"),
            ("WTM_USER_DATA", "C:\\tmp\\sandbox"),
        ]));
        let sandbox = PathBuf::from("C:\\tmp\\sandbox");
        assert_eq!(d.config_dir, sandbox);
        assert_eq!(
            d.legacy_config_files,
            vec![sandbox.join("worktree-manager.json")]
        );
    }
}
