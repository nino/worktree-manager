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
//! Separately, [`last_claude_session`] says whether Claude Code has a
//! session whose working folder is the worktree, and when it last wrote to
//! one. Claude Code keeps each session as `<id>.jsonl` in
//! `~/.claude/projects/<folder>`, where `<folder>` is the session's working
//! folder with every character but a letter or digit turned into `-`.

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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
    let modified = fs::symlink_metadata(path).ok()?.modified().ok()?;
    Some(modified.duration_since(UNIX_EPOCH).ok()?.as_secs())
}

/// When the worktree at `worktree` was last worked in, in Unix seconds,
/// given the paths (relative to it) that `git status` lists. `None` when
/// neither time can be read. Blocking: it reads the disk.
pub fn last_activity(worktree: &Path, changed: &[String]) -> Option<u64> {
    let files = changed
        .iter()
        .take(MAX_PATHS)
        .filter_map(|p| mtime(&worktree.join(p)));
    reflog_time(worktree).into_iter().chain(files).max()
}

// MARK: Claude Code sessions

/// Claude Code's name for the folder that holds the sessions run in `cwd`.
/// Lossy (`a.b` and `a/b` share one), which only ever errs towards saying a
/// worktree has a session.
pub fn claude_project_name(cwd: &str) -> String {
    cwd.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Where Claude Code keeps its sessions: `$CLAUDE_CONFIG_DIR/projects`,
/// else `~/.claude/projects`. Looked up once.
fn claude_projects_dir() -> Option<&'static Path> {
    static DIR: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let config = std::env::var_os("CLAUDE_CONFIG_DIR")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| directories::BaseDirs::new().map(|d| d.home_dir().join(".claude")))?;
        Some(config.join("projects"))
    })
    .as_deref()
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

/// [`last_claude_session_in`] Claude Code's own folder. Blocking.
pub fn last_claude_session(cwd: &str) -> Option<u64> {
    last_claude_session_in(claude_projects_dir()?, cwd)
}

/// Now, in Unix seconds.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// How long ago `then` was, short enough for a badge: "now", "5m", "3h",
/// "4d", "3w", "5mo", "2y". A time in the future (a skewed clock) is "now".
pub fn ago(then: u64, now: u64) -> String {
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;
    let secs = now.saturating_sub(then);
    match secs {
        s if s < MINUTE => "now".to_string(),
        s if s < HOUR => format!("{}m", s / MINUTE),
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
        assert_eq!(ago(now, now), "now");
        assert_eq!(ago(now + 30, now), "now");
        assert_eq!(ago(now - 59, now), "now");
        assert_eq!(ago(now - 60, now), "1m");
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
}
