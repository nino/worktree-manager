//! When a worktree was last worked in, so stale ones can be picked out.
//!
//! It is the newest of two times, both cheap to read and neither needing a
//! git process of its own:
//!
//! - the last entry in the worktree's own HEAD reflog (each linked worktree
//!   has one), which every commit, checkout, reset, rebase, merge and pull
//!   there appends to, including the `git worktree add` that made it;
//! - the modification times of the files `git status` already lists as
//!   changed or untracked, which catch work not yet committed.
//!
//! Commit dates alone would not do: a branch made from a week-old commit
//! would look a week stale on the day it was made, and a rebase keeps the
//! old dates. The index's mtime is no good either, because `git status`
//! (which the app runs on every refresh) may rewrite it. Ignored files
//! (build output) are left out on purpose: a build is not work.
//!
//! Separately, [`ClaudeIndex`] says what Claude Code sessions have the
//! worktree as their working folder:
//!
//! - when one was last written to: Claude Code keeps each as `<id>.jsonl` in
//!   `~/.claude/projects/<folder>`, `<folder>` being the working folder with
//!   every character but a letter or digit turned into `-`;
//! - whether one is running: `~/.claude/sessions/<pid>.json` exists while it
//!   does, naming its `cwd`;
//! - whether they are archived: the Claude desktop app keeps a record of
//!   each Code session it ran, with `cwd`, `worktreePath` and `isArchived`,
//!   under `Claude/claude-code-sessions` in the application data folder.
//!   Sessions only claude.ai has are not on disk, so not known.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::types::ClaudeSessions;

/// How much of the end of the reflog is read: comfortably more than one
/// entry, however long the file has grown.
const REFLOG_TAIL: u64 = 8 * 1024;

/// Changed files looked at, at most. A worktree with more than this is
/// plainly in use, and a stat each is not free on a network drive.
const MAX_PATHS: usize = 2000;

/// The time of the last entry of a reflog, in Unix seconds. An entry is
/// `<old> <new> <name> <<email>> <seconds> <zone>\t<message>`.
pub fn last_reflog_time(contents: &str) -> Option<u64> {
    let line = contents.lines().rev().find(|l| !l.trim().is_empty())?;
    let head = line.split('\t').next()?;
    let mut fields = head.split_whitespace().rev();
    let _zone = fields.next()?;
    fields.next()?.parse().ok()
}

/// The git directory of a worktree: `.git` itself in the primary tree, or
/// where the `gitdir:` line of a linked worktree's `.git` file points.
fn git_dir(worktree: &Path) -> Option<PathBuf> {
    let dot_git = worktree.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git);
    }
    let text = fs::read_to_string(&dot_git).ok()?;
    let target = text.lines().find_map(|l| l.strip_prefix("gitdir:"))?.trim();
    Some(worktree.join(target))
}

fn reflog_time(worktree: &Path) -> Option<u64> {
    let mut file = fs::File::open(git_dir(worktree)?.join("logs").join("HEAD")).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(REFLOG_TAIL)))
        .ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    // The cut may land inside a line or a character; only the last line is
    // read, and it is whole.
    last_reflog_time(&String::from_utf8_lossy(&bytes))
}

fn mtime(path: &Path) -> Option<u64> {
    unix(fs::symlink_metadata(path).ok()?.modified().ok()?)
}

/// When the worktree at `worktree` was last worked in, in Unix seconds,
/// given the paths (relative to it) that `git status` lists. `None` when
/// neither time can be read. Blocking: it reads the disk.
pub fn last_activity(worktree: &Path, changed: &[String]) -> Option<u64> {
    let mut budget = MAX_PATHS;
    let files = changed
        .iter()
        .filter_map(|p| newest_within(&worktree.join(p), &mut budget));
    reflog_time(worktree).into_iter().chain(files).max()
}

/// The newest mtime of `path` and, for a folder, of what is in it, looking
/// at no more than `budget` entries in all. `git status` names a new folder
/// as `dir/` rather than its files, and a folder's own mtime does not change
/// when a file in it is edited.
fn newest_within(path: &Path, budget: &mut usize) -> Option<u64> {
    let mut newest = None;
    let mut stack = vec![path.to_path_buf()];
    while let Some(p) = stack.pop() {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        let Ok(meta) = fs::symlink_metadata(&p) else {
            continue;
        };
        newest = newest.max(meta.modified().ok().and_then(unix));
        if meta.is_dir() {
            if let Ok(entries) = fs::read_dir(&p) {
                stack.extend(entries.filter_map(Result::ok).map(|e| e.path()));
            }
        }
    }
    newest
}

fn unix(t: SystemTime) -> Option<u64> {
    Some(t.duration_since(UNIX_EPOCH).ok()?.as_secs())
}

// MARK: Claude Code sessions

/// Claude Code's name for the folder that holds the sessions run in `cwd`.
/// Lossy (`a.b` and `a/b` share one), which only ever errs towards saying a
/// worktree has a session. Claude Code shortens a name over 200 characters
/// and adds a hash, and a session started in a subfolder is filed under
/// that subfolder: neither is found.
pub fn claude_project_name(cwd: &str) -> String {
    cwd.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Claude Code's own folder: `$CLAUDE_CONFIG_DIR`, else `~/.claude`.
fn claude_config_dir() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| directories::BaseDirs::new().map(|d| d.home_dir().join(".claude")))
}

/// Where the Claude desktop app keeps a record of each Code session it ran,
/// archived or not: `claude-code-sessions` in its folder under the system's
/// application data (`~/Library/Application Support/Claude` on macOS).
fn desktop_sessions_dir() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|d| d.config_dir().join("Claude").join("claude-code-sessions"))
}

/// The newest modification time of a session file in `projects` for the
/// sessions run in `cwd`, in Unix seconds; `None` when there is none.
/// Blocking.
pub fn last_claude_session_in(projects: &Path, cwd: &str) -> Option<u64> {
    fs::read_dir(projects.join(claude_project_name(cwd)))
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
        .filter_map(|e| mtime(&e.path()))
        .max()
}

/// A running session's `~/.claude/sessions/<pid>.json`, which Claude Code
/// writes at start and deletes at exit: its process and working folder.
pub fn parse_running_session(json: &str) -> Option<(u32, String)> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let pid = u32::try_from(v.get("pid")?.as_u64()?).ok()?;
    Some((pid, v.get("cwd")?.as_str()?.to_string()))
}

/// One of the desktop app's session records: the folders it names (where it
/// ran, and the worktree it made, if any) and whether it is archived.
pub fn parse_desktop_record(json: &str) -> Option<(Vec<String>, bool)> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let folders = ["cwd", "worktreePath"]
        .iter()
        .filter_map(|k| v.get(*k)?.as_str())
        .filter(|s| !s.is_empty())
        .map(trim_path)
        .collect::<Vec<_>>();
    let archived = v.get("isArchived")?.as_bool()?;
    (!folders.is_empty()).then_some((folders, archived))
}

/// A path as compared: forward slashes, no trailing separator.
fn trim_path(p: &str) -> String {
    let p = crate::paths::normalise(p);
    match p.trim_end_matches('/') {
        "" => "/".to_string(),
        t => t.to_string(),
    }
}

/// Whether a process is still there, for a session file a crash left
/// behind. `ps -p` on macOS and Linux; where there is no `ps` (Windows)
/// the file is believed.
fn process_alive(pid: u32) -> bool {
    match std::process::Command::new("ps")
        .args(["-p", &pid.to_string()])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        Ok(status) => status.success(),
        Err(_) => true,
    }
}

/// What is known about the Claude Code sessions of every folder, read once
/// per listing rather than once per worktree.
#[derive(Debug, Default)]
pub struct ClaudeIndex {
    projects: Option<PathBuf>,
    running: HashSet<String>,
    /// Folder → whether each desktop record naming it is archived.
    desktop: HashMap<String, Vec<bool>>,
}

impl ClaudeIndex {
    /// Read Claude Code's folder and the desktop app's records. Blocking.
    pub fn load() -> Self {
        let config = claude_config_dir();
        let running = config
            .as_ref()
            .map(|c| json_files(&c.join("sessions"), 1, |_| true))
            .unwrap_or_default();
        let desktop = desktop_sessions_dir()
            .map(|d| json_files(&d, 3, |name| name.starts_with("local_")))
            .unwrap_or_default();
        Self::from_parts(
            config.map(|c| c.join("projects")),
            running.iter().map(String::as_str),
            desktop.iter().map(String::as_str),
            process_alive,
        )
    }

    /// The index from the files' contents, for tests.
    pub fn from_parts<'a>(
        projects: Option<PathBuf>,
        running: impl IntoIterator<Item = &'a str>,
        desktop: impl IntoIterator<Item = &'a str>,
        alive: impl Fn(u32) -> bool,
    ) -> Self {
        let running = running
            .into_iter()
            .filter_map(parse_running_session)
            .filter(|(pid, _)| alive(*pid))
            .map(|(_, cwd)| trim_path(&cwd))
            .collect();
        let mut by_folder: HashMap<String, Vec<bool>> = HashMap::new();
        for (folders, archived) in desktop.into_iter().filter_map(parse_desktop_record) {
            for folder in folders {
                by_folder.entry(folder).or_default().push(archived);
            }
        }
        Self {
            projects,
            running,
            desktop: by_folder,
        }
    }

    /// The sessions run in `cwd`, or `None` when there are none. Blocking:
    /// it reads the folder of `cwd`'s transcripts.
    pub fn lookup(&self, cwd: &str) -> Option<ClaudeSessions> {
        let key = trim_path(cwd);
        let last_written = self
            .projects
            .as_deref()
            .and_then(|p| last_claude_session_in(p, cwd));
        let running = self.running.contains(&key);
        // Archived only when every record says so: one open session in the
        // desktop app is a reason to keep the worktree.
        let archived = self
            .desktop
            .get(&key)
            .map(|records| records.iter().all(|a| *a));
        (last_written.is_some() || running || archived.is_some()).then_some(ClaudeSessions {
            last_written,
            running,
            archived,
        })
    }
}

/// The contents of the `.json` files under `dir`, `depth` levels down at
/// most, whose names pass `keep`. Unreadable ones are skipped.
fn json_files(dir: &Path, depth: usize, keep: impl Fn(&str) -> bool + Copy) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() && depth > 1 {
            out.extend(json_files(&path, depth - 1, keep));
        } else if kind.is_file()
            && path.extension().is_some_and(|x| x == "json")
            && path.file_name().and_then(|n| n.to_str()).is_some_and(keep)
        {
            out.extend(fs::read_to_string(&path).ok());
        }
    }
    out
}

/// Now, in Unix seconds.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// How long ago `then` was, short enough for a badge: "<1h", "3h", "4d",
/// "3w", "5mo", "2y". A time in the future (a skewed clock) is "<1h". No
/// minutes: the list is redrawn on model changes, at least at every
/// background fetch (minutes apart), not on a clock.
pub fn ago(then: u64, now: u64) -> String {
    const HOUR: u64 = 3600;
    const DAY: u64 = 24 * HOUR;
    let secs = now.saturating_sub(then);
    match secs {
        s if s < HOUR => "<1h".to_string(),
        s if s < DAY => format!("{}h", s / HOUR),
        s if s < 14 * DAY => format!("{}d", s / DAY),
        s if s < 60 * DAY => format!("{}w", s / (7 * DAY)),
        s if s < 365 * DAY => format!("{}mo", s / (30 * DAY)),
        s => format!("{}y", s / (365 * DAY)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENTRY: &str = "0000000000000000000000000000000000000000 \
        1111111111111111111111111111111111111111 Ada Lovelace <ada@example.com> \
        1700000000 +0100\treset: moving to HEAD";

    #[test]
    fn the_reflog_time_is_its_last_entry() {
        let later = ENTRY.replace("1700000000", "1700000500");
        assert_eq!(
            last_reflog_time(&format!("{ENTRY}\n{later}\n\n")),
            Some(1_700_000_500)
        );
        assert_eq!(last_reflog_time(""), None);
        assert_eq!(last_reflog_time("garbage"), None);
    }

    #[test]
    fn a_message_with_numbers_does_not_confuse_the_time() {
        let entry = ENTRY.replace("reset: moving to HEAD", "commit: bump to 1 2 3");
        assert_eq!(last_reflog_time(&entry), Some(1_700_000_000));
    }

    #[test]
    fn ago_is_short_and_rounds_down() {
        let now = 1_000_000_000;
        assert_eq!(ago(now, now), "<1h");
        assert_eq!(ago(now + 30, now), "<1h");
        assert_eq!(ago(now - 3599, now), "<1h");
        assert_eq!(ago(now - 3 * 3600 - 59, now), "3h");
        assert_eq!(ago(now - 86_400, now), "1d");
        assert_eq!(ago(now - 13 * 86_400, now), "13d");
        assert_eq!(ago(now - 14 * 86_400, now), "2w");
        assert_eq!(ago(now - 59 * 86_400, now), "8w");
        assert_eq!(ago(now - 60 * 86_400, now), "2mo");
        assert_eq!(ago(now - 400 * 86_400, now), "1y");
    }

    #[test]
    fn activity_is_the_newest_of_the_reflog_and_the_changed_files() {
        let dir = std::env::temp_dir().join(format!("wtm-activity-{}", std::process::id()));
        let wt = dir.join("wt");
        let gitdir = dir.join("repo/.git/worktrees/wt");
        fs::create_dir_all(gitdir.join("logs")).unwrap();
        fs::create_dir_all(&wt).unwrap();
        fs::write(wt.join(".git"), "gitdir: ../repo/.git/worktrees/wt\n").unwrap();
        fs::write(gitdir.join("logs/HEAD"), format!("{ENTRY}\n")).unwrap();

        // Only the reflog: its time.
        assert_eq!(last_activity(&wt, &[]), Some(1_700_000_000));
        // A file changed since: that file's time, which is now.
        fs::write(wt.join("edited.txt"), "x").unwrap();
        let t = last_activity(&wt, &["edited.txt".into(), "deleted.txt".into()]).unwrap();
        assert!(t >= now() - 60, "{t}");

        // A new folder is dated by the newest file in it, however deep.
        fs::create_dir_all(wt.join("notes/deep")).unwrap();
        fs::write(wt.join("notes/deep/plan.md"), "x").unwrap();
        // Dated later than any folder could be, so only reading the file
        // gives this time. (Windows cannot open a folder to date it.)
        let later = now() + 1000;
        fs::OpenOptions::new()
            .write(true)
            .open(wt.join("notes/deep/plan.md"))
            .unwrap()
            .set_modified(UNIX_EPOCH + std::time::Duration::from_secs(later))
            .unwrap();
        fs::remove_file(wt.join("edited.txt")).unwrap();
        assert_eq!(last_activity(&wt, &["notes/".into()]), Some(later));

        // No git dir to be found and no files: nothing.
        assert_eq!(last_activity(&dir.join("nowhere"), &[]), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn claude_names_a_session_folder_after_its_working_folder() {
        assert_eq!(
            claude_project_name("/Users/ada/wt/app/fix_login.v2"),
            "-Users-ada-wt-app-fix-login-v2"
        );
        assert_eq!(claude_project_name("C:\\src\\app"), "C--src-app");
    }

    #[test]
    fn a_session_is_found_by_its_working_folder() {
        let dir = std::env::temp_dir().join(format!("wtm-claude-{}", std::process::id()));
        let project = dir.join("-w-app-fix");
        fs::create_dir_all(&project).unwrap();
        assert_eq!(last_claude_session_in(&dir, "/w/app/fix"), None);
        fs::write(project.join("memory.md"), "").unwrap();
        assert_eq!(last_claude_session_in(&dir, "/w/app/fix"), None);
        fs::write(project.join("0b5c.jsonl"), "{}").unwrap();
        assert!(last_claude_session_in(&dir, "/w/app/fix").unwrap() >= now() - 60);
        // A sibling whose name starts the same is not this worktree.
        assert_eq!(last_claude_session_in(&dir, "/w/app/fi"), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn running_sessions_and_desktop_records_are_read_by_folder() {
        let running = [
            r#"{"pid": 10, "sessionId": "a", "cwd": "/w/app/fix", "status": "idle"}"#,
            // Its process is gone: a file a crash left behind.
            r#"{"pid": 11, "cwd": "/w/app/old"}"#,
            "not json",
        ];
        let desktop = [
            r#"{"sessionId": "local_1", "cwd": "/w/app/old/", "isArchived": true}"#,
            r#"{"sessionId": "local_2", "cwd": "/w/app", "worktreePath": "/w/app/new", "isArchived": true}"#,
            r#"{"sessionId": "local_3", "cwd": "/w/app/new", "isArchived": false}"#,
            r#"{"sessionId": "local_4", "worktreePath": null, "isArchived": false}"#,
        ];
        let index = ClaudeIndex::from_parts(None, running, desktop, |pid| pid == 10);

        let fix = index.lookup("/w/app/fix").unwrap();
        assert!(fix.running && fix.archived.is_none());

        let old = index.lookup("/w/app/old").unwrap();
        assert!(!old.running);
        assert_eq!(old.archived, Some(true));

        // One record of two still open: not archived.
        assert_eq!(index.lookup("/w/app/new").unwrap().archived, Some(false));
        assert_eq!(index.lookup("/w/app").unwrap().archived, Some(true));
        assert_eq!(index.lookup("/w/elsewhere"), None);
    }

    #[test]
    fn a_record_without_a_folder_or_flag_is_skipped() {
        assert_eq!(parse_desktop_record(r#"{"isArchived": true}"#), None);
        assert_eq!(parse_desktop_record(r#"{"cwd": "/a"}"#), None);
        assert_eq!(parse_running_session(r#"{"cwd": "/a"}"#), None);
    }
}
