//! `.worktreeinclude`, a Claude Code convention rather than a git one: a file
//! at the root of a repository, in `.gitignore` syntax, naming the files a
//! fresh worktree should be given from the main checkout (`.env` and the
//! like, which a checkout cannot bring along). A file is copied only if it
//! matches a pattern *and* git ignores it, so nothing tracked is ever
//! duplicated: <https://code.claude.com/docs/en/worktrees>.
//!
//! Git names the ignored files, and stops at a folder it ignores as a whole
//! (`node_modules/`, `target/`) rather than listing what is inside. Such a
//! folder is only opened when a pattern can reach into it, by Claude Code's
//! rule: the folder itself matches, the pattern names a path (`vendor/**/x`),
//! or the first name after a leading `**/` is one of the folder's names. So
//! `.env` copies `app/.env` but not `node_modules/pkg/.env`, and a large
//! ignored tree that no pattern reaches is never walked.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use log::warn;

use crate::git::run_git_bytes;

pub const FILE_NAME: &str = ".worktreeinclude";

/// Copy what the repo's `.worktreeinclude` asks for from `repo_path` into
/// the new worktree at `worktree_path`, never replacing anything already
/// there. Returns how many files were copied, or what went wrong: a file
/// that could not be copied does not stop the rest.
pub async fn apply(repo_path: &str, worktree_path: &str) -> Result<usize, String> {
    let include = Path::new(repo_path).join(FILE_NAME);
    if tokio::fs::metadata(&include).await.is_err() {
        return Ok(0);
    }
    // `--directory` stops at a folder ignored as a whole; it also names the
    // untracked folders it went into, which the check below tells apart.
    let listed = run_git_bytes(
        repo_path,
        &[
            "ls-files",
            "-z",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
        ],
        None,
        &[],
    )
    .await
    .map_err(|e| format!("Could not list the files git ignores: {}", e.detail()))?;
    let (files, dirs) = split_entries(&listed);
    let ignored_dirs = if dirs.is_empty() {
        Vec::new()
    } else {
        let mut input = Vec::new();
        for dir in &dirs {
            input.extend_from_slice(dir.as_bytes());
            input.push(0);
        }
        let out = run_git_bytes(
            repo_path,
            &["check-ignore", "-z", "--stdin"],
            Some(input),
            &[1],
        )
        .await
        .map_err(|e| format!("Could not ask git which folders it ignores: {}", e.detail()))?;
        split_entries(&out).1
    };
    let (from, to) = (PathBuf::from(repo_path), PathBuf::from(worktree_path));
    tokio::task::spawn_blocking(move || copy_matching(&from, &to, &include, &files, &ignored_dirs))
        .await
        .map_err(|e| format!("Copying the {FILE_NAME} files stopped: {e}"))?
}

// MARK: Matching

/// The include patterns, parsed by git's rules, plus the raw lines for
/// deciding whether a folder is worth opening.
struct Patterns {
    matcher: Gitignore,
    lines: Vec<String>,
}

impl Patterns {
    fn load(root: &Path, include: &Path) -> (Self, Option<String>) {
        let mut builder = GitignoreBuilder::new(root);
        let problem = builder
            .add(include)
            .map(|e| format!("Some of {FILE_NAME} could not be read: {e}"));
        let matcher = builder.build().unwrap_or_else(|_| Gitignore::empty());
        let lines = std::fs::read_to_string(include)
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('!'))
            .map(|l| l.strip_prefix('\\').unwrap_or(l).to_string())
            .collect();
        (Patterns { matcher, lines }, problem)
    }

    /// The pattern that takes `path`, unless none does or a `!` line lets
    /// it go again.
    fn pattern_for(&self, path: &Path, is_dir: bool) -> Option<&str> {
        match self.matcher.matched_path_or_any_parents(path, is_dir) {
            ignore::Match::Ignore(glob) => Some(glob.original()),
            _ => None,
        }
    }

    /// Whether any pattern could take a file inside the ignored folder `dir`.
    fn may_reach(&self, dir: &str) -> bool {
        self.pattern_for(Path::new(dir), true).is_some()
            || self.lines.iter().any(|p| could_reach(p, dir))
    }
}

/// Claude Code's rule for a pattern reaching into a folder git ignores as a
/// whole: a pattern naming a path does; a bare name, or a leading `**/`,
/// only when the name after it is one of the folder's own.
fn reaches(pattern: &str, dir: &str) -> bool {
    let pattern = pattern.trim_end_matches('/');
    let first = match pattern.strip_prefix("**/") {
        Some(rest) => rest.split('/').next().unwrap_or(rest),
        None if pattern.contains('/') => return true,
        None => pattern,
    };
    dir.trim_end_matches('/')
        .split('/')
        .any(|name| name == first)
}

/// [`reaches`], also ruling out a path-naming pattern that leads somewhere
/// else (`config/x` cannot reach into `vendor/`), so an unrelated ignored
/// folder is not walked.
fn could_reach(pattern: &str, dir: &str) -> bool {
    if !reaches(pattern, dir) {
        return false;
    }
    let anchored = pattern.trim_end_matches('/');
    if anchored.starts_with("**/") || !anchored.contains('/') {
        return true;
    }
    let literal = anchored
        .trim_start_matches('/')
        .split('/')
        .take_while(|c| !c.contains(['*', '?', '[', '\\']));
    literal
        .zip(dir.trim_end_matches('/').split('/'))
        .all(|(a, b)| a == b)
}

// MARK: Copying

fn copy_matching(
    from: &Path,
    to: &Path,
    include: &Path,
    files: &[String],
    ignored_dirs: &[String],
) -> Result<usize, String> {
    let (patterns, problem) = Patterns::load(from, include);
    let mut copier = Copier {
        from,
        to,
        made: HashSet::new(),
        copied: 0,
        failed: problem.into_iter().collect(),
    };
    for file in files {
        if patterns.pattern_for(Path::new(file), false).is_some() {
            copier.copy(Path::new(file));
        }
    }
    for dir in ignored_dirs {
        let rel = Path::new(dir.trim_end_matches('/'));
        if is_repository(&from.join(rel)) || !patterns.may_reach(dir) {
            continue;
        }
        let matched = patterns.pattern_for(rel, true).is_some();
        walk(&patterns, &mut copier, dir, rel, matched);
    }
    let Copier { copied, failed, .. } = copier;
    match failed.first() {
        None => Ok(copied),
        Some(first) if failed.len() == 1 => Err(first.clone()),
        Some(first) => Err(format!(
            "{} files from {FILE_NAME} were not copied. The first: {first}",
            failed.len()
        )),
    }
}

/// Go through `rel`, inside the ignored folder `top`, copying what a
/// pattern takes. `matched` says a folder on the way matched a pattern
/// itself, which takes everything below it.
fn walk(patterns: &Patterns, copier: &mut Copier, top: &str, rel: &Path, matched: bool) {
    let entries = match std::fs::read_dir(copier.from.join(rel)) {
        Ok(entries) => entries,
        Err(e) => {
            copier
                .failed
                .push(format!("Could not read {}: {e}", rel.display()));
            return;
        }
    };
    for entry in entries.flatten() {
        let path = rel.join(entry.file_name());
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            // Another repository or worktree (Claude Code keeps its own in
            // `.claude/worktrees/`) is not this one's to copy.
            if is_repository(&entry.path()) {
                continue;
            }
            let matched = matched || patterns.pattern_for(&path, true).is_some();
            walk(patterns, copier, top, &path, matched);
        } else if let Some(pattern) = patterns.pattern_for(&path, false) {
            if matched || reaches(pattern, top) {
                copier.copy(&path);
            }
        }
    }
}

fn is_repository(dir: &Path) -> bool {
    std::fs::symlink_metadata(dir.join(".git")).is_ok()
}

struct Copier<'a> {
    from: &'a Path,
    to: &'a Path,
    /// Folders already made in the worktree.
    made: HashSet<PathBuf>,
    copied: usize,
    failed: Vec<String>,
}

impl Copier<'_> {
    fn copy(&mut self, rel: &Path) {
        let (src, dest) = (self.from.join(rel), self.to.join(rel));
        if std::fs::symlink_metadata(&dest).is_ok() {
            return;
        }
        // Through a link to the file it names, but nothing else: a link to a
        // folder, a dangling link or a pipe (some secret managers serve
        // `.env` as one, and opening it would wait forever) is left alone.
        match std::fs::metadata(&src) {
            Ok(meta) if meta.is_file() => {}
            _ => {
                warn!(
                    "{FILE_NAME}: skipping {}, not a regular file",
                    src.display()
                );
                return;
            }
        }
        let result = (|| {
            if let Some(dir) = dest.parent() {
                if !self.made.contains(dir) {
                    std::fs::create_dir_all(dir)?;
                    self.made.insert(dir.to_path_buf());
                }
            }
            std::fs::copy(&src, &dest)
        })();
        match result {
            Ok(_) => self.copied += 1,
            Err(e) => self.failed.push(format!(
                "Could not copy {} from {FILE_NAME}: {e}",
                rel.display()
            )),
        }
    }
}

/// Split git's NUL-separated paths into files and folders (`dir/`). A name
/// that is not UTF-8 is passed over: git on Windows never writes one, and
/// elsewhere it is no `.env` anybody listed.
fn split_entries(output: &[u8]) -> (Vec<String>, Vec<String>) {
    let mut files = Vec::new();
    let mut dirs = Vec::new();
    for raw in output.split(|b| *b == 0).filter(|s| !s.is_empty()) {
        match std::str::from_utf8(raw) {
            Ok(path) if path.ends_with('/') => dirs.push(path.to_string()),
            Ok(path) => files.push(path.to_string()),
            Err(_) => warn!(
                "{FILE_NAME}: skipping {}, not UTF-8",
                String::from_utf8_lossy(raw)
            ),
        }
    }
    (files, dirs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_replaces_a_file_already_in_the_worktree() {
        let base = std::env::temp_dir().join(format!("wtm-include-{}", std::process::id()));
        let (from, to) = (base.join("from"), base.join("to"));
        std::fs::create_dir_all(from.join("d")).unwrap();
        std::fs::create_dir_all(&to).unwrap();
        std::fs::write(from.join("a"), "new").unwrap();
        std::fs::write(from.join("d/b"), "new").unwrap();
        std::fs::write(to.join("a"), "mine").unwrap();
        let mut copier = Copier {
            from: &from,
            to: &to,
            made: HashSet::new(),
            copied: 0,
            failed: Vec::new(),
        };
        copier.copy(Path::new("a"));
        copier.copy(Path::new("d/b"));
        // A folder is not a file to copy, and is not a failure either.
        copier.copy(Path::new("d"));
        assert_eq!(std::fs::read_to_string(to.join("a")).unwrap(), "mine");
        assert_eq!(std::fs::read_to_string(to.join("d/b")).unwrap(), "new");
        assert_eq!((copier.copied, copier.failed.len()), (1, 0));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn splits_files_from_folders() {
        let (files, dirs) = split_entries(b"a\0b/\0c d\0\xff\0");
        assert_eq!(files, ["a", "c d"]);
        assert_eq!(dirs, ["b/"]);
    }

    #[test]
    fn reaches_into_an_ignored_folder_by_claude_codes_rule() {
        // A bare name is `**/name`: only a folder of that name.
        assert!(!reaches(".env", "node_modules/"));
        assert!(reaches("node_modules/", "node_modules/"));
        assert!(reaches("**/.claude/skills/*.md", ".claude/"));
        assert!(!reaches("**/config.json", "vendor/"));
        assert!(reaches("vendor/**/config.json", "vendor/"));
        assert!(reaches("/config/secrets.json", "config/"));
    }

    #[test]
    fn a_path_elsewhere_does_not_open_a_folder() {
        assert!(could_reach("vendor/**/config.json", "vendor/"));
        assert!(!could_reach("config/secrets.json", "vendor/"));
        assert!(could_reach("config/secrets.json", "config/"));
        assert!(could_reach("*/secrets.json", "config/"));
        assert!(!could_reach(".env", "target/"));
    }
}
