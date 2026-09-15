//! Git command runner plus pure porcelain parsers.
//!
//! Every git invocation is a child process run on the tokio runtime, never on
//! the UI thread. A global semaphore bounds concurrency so refreshing many
//! worktrees at once does not fork-storm the machine.

use std::fmt;
use std::path::Path;
use std::sync::OnceLock;

use tokio::process::Command;
use tokio::sync::Semaphore;

use crate::types::{WorktreeInfo, WorktreeStatus};

/// Maximum number of git processes in flight at once.
const MAX_CONCURRENT_GIT: usize = 12;

fn gate() -> &'static Semaphore {
    static GATE: OnceLock<Semaphore> = OnceLock::new();
    GATE.get_or_init(|| Semaphore::new(MAX_CONCURRENT_GIT))
}

/// Raised when a git command exits non-zero or cannot be spawned.
#[derive(Debug, Clone)]
pub struct GitError {
    pub message: String,
    pub cwd: String,
    pub args: Vec<String>,
    pub stderr: String,
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for GitError {}

impl GitError {
    /// git's stderr if it said anything, else the wrapped message.
    pub fn detail(&self) -> String {
        let stderr = self.stderr.trim();
        if stderr.is_empty() {
            self.message.clone()
        } else {
            stderr.to_string()
        }
    }
}

pub type GitResult<T> = Result<T, GitError>;

/// Run a git command in `cwd` and return its stdout.
pub async fn run_git(cwd: impl AsRef<Path>, args: &[&str]) -> GitResult<String> {
    let cwd = cwd.as_ref();
    let _permit = gate().acquire().await.expect("git gate never closes");
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        // Never let a git subprocess block on an interactive editor or prompt.
        .env("GIT_EDITOR", "true")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .kill_on_drop(true)
        .output()
        .await;
    let mk = |message: String, stderr: String| GitError {
        message,
        cwd: cwd.to_string_lossy().into_owned(),
        args: args.iter().map(|s| s.to_string()).collect(),
        stderr,
    };
    match output {
        Ok(out) if out.status.success() => Ok(String::from_utf8_lossy(&out.stdout).into_owned()),
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
            let why = if stderr.trim().is_empty() {
                out.status.to_string()
            } else {
                stderr.trim().to_string()
            };
            Err(mk(format!("git {} failed: {why}", args.join(" ")), stderr))
        }
        Err(err) => {
            let why = if err.kind() == std::io::ErrorKind::NotFound {
                // Either git itself or the working directory is missing;
                // mirror the ENOENT wording callers pattern-match on.
                format!("spawn git ENOENT: {err}")
            } else {
                err.to_string()
            };
            Err(mk(
                format!("git {} failed: {why}", args.join(" ")),
                String::new(),
            ))
        }
    }
}

// MARK: Pure parsers

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedWorktree {
    pub path: String,
    pub head: String,
    pub branch: Option<String>,
    pub detached: bool,
    pub locked: bool,
    pub bare: bool,
    /// Set when git reports the worktree directory as missing.
    pub prunable: bool,
}

/// Parse the output of `git worktree list --porcelain`.
pub fn parse_worktree_porcelain(output: &str) -> Vec<ParsedWorktree> {
    let mut entries = Vec::new();
    let mut cur: Option<ParsedWorktree> = None;
    let flush = |cur: &mut Option<ParsedWorktree>, entries: &mut Vec<ParsedWorktree>| {
        if let Some(w) = cur.take() {
            if !w.path.is_empty() {
                entries.push(w);
            }
        }
    };
    for line in output.lines() {
        if line.trim().is_empty() {
            flush(&mut cur, &mut entries);
            continue;
        }
        let (key, val) = match line.split_once(' ') {
            Some((k, v)) => (k, v),
            None => (line, ""),
        };
        match key {
            "worktree" => {
                flush(&mut cur, &mut entries);
                cur = Some(ParsedWorktree {
                    path: val.to_string(),
                    ..Default::default()
                });
            }
            "HEAD" => {
                if let Some(c) = cur.as_mut() {
                    c.head = val.to_string();
                }
            }
            "branch" => {
                if let Some(c) = cur.as_mut() {
                    c.branch = Some(val.strip_prefix("refs/heads/").unwrap_or(val).to_string());
                }
            }
            "detached" => set(&mut cur, |c| c.detached = true),
            "locked" => set(&mut cur, |c| c.locked = true),
            "bare" => set(&mut cur, |c| c.bare = true),
            "prunable" => set(&mut cur, |c| c.prunable = true),
            _ => {}
        }
    }
    flush(&mut cur, &mut entries);
    entries
}

fn set(cur: &mut Option<ParsedWorktree>, f: impl FnOnce(&mut ParsedWorktree)) {
    if let Some(c) = cur.as_mut() {
        f(c);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedStatus {
    pub head: Option<String>,
    pub detached: bool,
    pub oid: String,
    pub upstream: Option<String>,
    pub ahead_upstream: u32,
    pub behind_upstream: u32,
    pub has_staged: bool,
    pub has_unstaged: bool,
    pub has_untracked: bool,
}

/// Parse the output of `git status --porcelain=v2 --branch`.
pub fn parse_status_porcelain_v2(output: &str) -> ParsedStatus {
    let mut r = ParsedStatus::default();
    for line in output.lines() {
        if line.is_empty() {
            continue;
        }
        if let Some(v) = line.strip_prefix("# branch.oid ") {
            r.oid = v.to_string();
        } else if let Some(v) = line.strip_prefix("# branch.head ") {
            if v == "(detached)" {
                r.detached = true;
                r.head = None;
            } else {
                r.head = Some(v.to_string());
            }
        } else if let Some(v) = line.strip_prefix("# branch.upstream ") {
            r.upstream = Some(v.to_string());
        } else if let Some(v) = line.strip_prefix("# branch.ab ") {
            // "+<ahead> -<behind>"
            let mut parts = v.split_whitespace();
            let ahead = parts
                .next()
                .and_then(|s| s.strip_prefix('+'))
                .and_then(|s| s.parse().ok());
            let behind = parts
                .next()
                .and_then(|s| s.strip_prefix('-'))
                .and_then(|s| s.parse().ok());
            if let (Some(a), Some(b)) = (ahead, behind) {
                r.ahead_upstream = a;
                r.behind_upstream = b;
            }
        } else if line.starts_with('#') {
            // other header lines: ignore
        } else {
            let bytes = line.as_bytes();
            match bytes[0] {
                b'1' | b'2' => {
                    let x = bytes.get(2).copied().unwrap_or(b'.');
                    let y = bytes.get(3).copied().unwrap_or(b'.');
                    if x != b'.' {
                        r.has_staged = true;
                    }
                    if y != b'.' {
                        r.has_unstaged = true;
                    }
                }
                b'u' => r.has_unstaged = true, // unmerged conflict needs resolution
                b'?' => r.has_untracked = true,
                _ => {}
            }
        }
    }
    r
}

/// Parse `git rev-list --left-right --count <trunk>...HEAD` → (behind, ahead).
pub fn parse_left_right_count(output: &str) -> (u32, u32) {
    let mut parts = output.split_whitespace();
    let behind = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let ahead = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (behind, ahead)
}

/// Parse `for-each-ref --format=%(refname:short)%09%(symref)`, dropping
/// symbolic refs (e.g. `origin/HEAD`, an alias rather than a real branch).
pub fn parse_ref_candidates(output: &str) -> Vec<String> {
    output
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| {
            let (name, symref) = l.split_once('\t').unwrap_or((l, ""));
            if !symref.is_empty() || name.is_empty() {
                None
            } else {
                Some(name.to_string())
            }
        })
        .collect()
}

// MARK: High-level operations

/// Resolve the repository's primary working tree for a given path, even when
/// the picked folder is a linked worktree (via the shared common dir), and
/// canonicalise symlinks so duplicate detection compares real paths.
pub async fn resolve_repo_root(some_path: &Path) -> GitResult<String> {
    let common = run_git(
        some_path,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .await?;
    let common = Path::new(common.trim());
    let root = if common.file_name().map(|n| n == ".git").unwrap_or(false) {
        common
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| common.to_path_buf())
    } else {
        // Bare repo (or unusual layout): fall back to the picked tree's toplevel.
        let top = run_git(some_path, &["rev-parse", "--show-toplevel"]).await?;
        Path::new(top.trim()).to_path_buf()
    };
    let real = tokio::fs::canonicalize(&root).await.unwrap_or(root);
    Ok(real.to_string_lossy().into_owned())
}

/// Best-effort detection of a repo's default/main branch.
pub async fn detect_main_branch(repo_path: &str) -> String {
    if let Ok(r) = run_git(
        repo_path,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    )
    .await
    {
        if let Some(name) = r.trim().strip_prefix("origin/") {
            return name.to_string();
        }
    }
    for name in ["main", "master"] {
        if branch_exists(repo_path, name).await {
            return name.to_string();
        }
    }
    if let Ok(cur) = run_git(repo_path, &["rev-parse", "--abbrev-ref", "HEAD"]).await {
        let cur = cur.trim();
        if !cur.is_empty() && cur != "HEAD" {
            return cur.to_string();
        }
    }
    "main".to_string()
}

/// The ref worktrees are compared against: `origin/<trunk>` when that
/// remote-tracking branch exists, else the local trunk name. Never fails.
pub async fn resolve_trunk_ref(repo_path: &str, main_branch: &str) -> String {
    let remote = format!("origin/{main_branch}");
    if ref_exists(repo_path, &remote).await {
        remote
    } else {
        main_branch.to_string()
    }
}

async fn ahead_behind_trunk(worktree_path: &str, trunk_ref: &str) -> (Option<u32>, Option<u32>) {
    let range = format!("{trunk_ref}...HEAD");
    match run_git(
        worktree_path,
        &["rev-list", "--left-right", "--count", &range],
    )
    .await
    {
        Ok(out) => {
            let (behind, ahead) = parse_left_right_count(&out);
            (Some(ahead), Some(behind))
        }
        Err(_) => (None, None),
    }
}

/// Compute the full git status for a single worktree.
pub async fn worktree_status(worktree_path: &str, trunk_ref: &str) -> GitResult<WorktreeStatus> {
    let status_out = run_git(worktree_path, &["status", "--porcelain=v2", "--branch"]).await?;
    let parsed = parse_status_porcelain_v2(&status_out);
    let (ahead, behind) = ahead_behind_trunk(worktree_path, trunk_ref).await;
    let has_upstream = parsed.upstream.is_some();
    Ok(WorktreeStatus {
        has_unstaged: parsed.has_unstaged,
        has_staged: parsed.has_staged,
        has_untracked: parsed.has_untracked,
        trunk_ref: trunk_ref.to_string(),
        ahead_of_main: ahead,
        behind_main: behind,
        has_upstream,
        unpushed_count: if has_upstream {
            parsed.ahead_upstream
        } else {
            0
        },
        unpushed: has_upstream && parsed.ahead_upstream > 0,
    })
}

/// Raw `git worktree list --porcelain` entries for a repo.
pub async fn list_worktrees_raw(repo_path: &str) -> GitResult<Vec<ParsedWorktree>> {
    let out = run_git(repo_path, &["worktree", "list", "--porcelain"]).await?;
    Ok(parse_worktree_porcelain(&out))
}

/// Convert one parsed entry into the UI model, computing status in the process.
pub async fn worktree_info(
    w: &ParsedWorktree,
    primary_path: Option<&str>,
    trunk_ref: &str,
) -> WorktreeInfo {
    let status = if w.prunable {
        None
    } else {
        worktree_status(&w.path, trunk_ref).await.ok()
    };
    WorktreeInfo {
        path: w.path.clone(),
        branch: w.branch.clone(),
        head: w.head.chars().take(12).collect(),
        is_main: primary_path == Some(w.path.as_str()),
        locked: w.locked,
        prunable: w.prunable,
        status,
    }
}

/// List all worktrees for a repo, with status, computed concurrently (one
/// task per worktree, bounded by the git gate).
pub async fn list_worktrees(repo_path: &str, trunk_ref: &str) -> GitResult<Vec<WorktreeInfo>> {
    let all = list_worktrees_raw(repo_path).await?;
    let primary = all.first().filter(|w| !w.bare).map(|w| w.path.clone());
    let mut set = tokio::task::JoinSet::new();
    let mut order = Vec::new();
    for (i, w) in all.into_iter().filter(|w| !w.bare).enumerate() {
        let primary = primary.clone();
        let trunk = trunk_ref.to_string();
        order.push(i);
        set.spawn(async move { (i, worktree_info(&w, primary.as_deref(), &trunk).await) });
    }
    let mut results: Vec<Option<WorktreeInfo>> = (0..order.len()).map(|_| None).collect();
    while let Some(joined) = set.join_next().await {
        if let Ok((i, info)) = joined {
            results[i] = Some(info);
        }
    }
    Ok(results.into_iter().flatten().collect())
}

/// List local branch names.
pub async fn list_branches(repo_path: &str) -> GitResult<Vec<String>> {
    let out = run_git(
        repo_path,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )
    .await?;
    Ok(out
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect())
}

/// List local branches and remote-tracking branches, for base-ref suggestions.
pub async fn list_base_ref_candidates(repo_path: &str) -> GitResult<Vec<String>> {
    let out = run_git(
        repo_path,
        &[
            "for-each-ref",
            "--format=%(refname:short)%09%(symref)",
            "refs/heads",
            "refs/remotes",
        ],
    )
    .await?;
    Ok(parse_ref_candidates(&out))
}

/// Validate a user-supplied branch/ref name (also rejects a leading `-`,
/// closing the option-injection hole for positional ref arguments).
pub async fn assert_valid_ref(repo_path: &str, name: &str) -> GitResult<()> {
    run_git(repo_path, &["check-ref-format", "--branch", name])
        .await
        .map(|_| ())
}

/// Whether the repo has a remote with the given name.
pub async fn has_remote(repo_path: &str, name: &str) -> bool {
    run_git(repo_path, &["remote", "get-url", name])
        .await
        .is_ok()
}

/// Whether a branch already exists locally.
pub async fn branch_exists(repo_path: &str, branch: &str) -> bool {
    let full = format!("refs/heads/{branch}");
    run_git(repo_path, &["show-ref", "--verify", "--quiet", &full])
        .await
        .is_ok()
}

/// Whether a ref resolves in the repo (branch, tag, remote-tracking…).
pub async fn ref_exists(repo_path: &str, r: &str) -> bool {
    run_git(repo_path, &["rev-parse", "--verify", "--quiet", r])
        .await
        .is_ok()
}

/// `git fetch --prune` for the repo's default remote.
pub async fn fetch_repo(repo_path: &str) -> GitResult<()> {
    run_git(repo_path, &["fetch", "--prune"]).await.map(|_| ())
}

/// Create a new worktree.
pub async fn add_worktree(
    repo_path: &str,
    worktree_path: &str,
    branch: &str,
    new_branch: bool,
    base_ref: Option<&str>,
) -> GitResult<()> {
    let mut args: Vec<&str> = vec!["worktree", "add"];
    if new_branch {
        // --no-track: even when basing off origin/main, the new branch must not
        // adopt it as upstream — "push sets upstream on first push" relies on
        // the branch having no upstream until pushed.
        args.extend(["--no-track", "-b", branch, worktree_path]);
        if let Some(b) = base_ref {
            args.push(b);
        }
    } else {
        args.extend([worktree_path, branch]);
    }
    run_git(repo_path, &args).await.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multiple_worktrees_with_branches_and_detached_heads() {
        let out = "worktree /repo/main\nHEAD 1111\nbranch refs/heads/main\n\nworktree /wt/feature\nHEAD 2222\nbranch refs/heads/feature/foo\n\nworktree /wt/detached\nHEAD 3333\ndetached\n\n";
        let r = parse_worktree_porcelain(out);
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].path, "/repo/main");
        assert_eq!(r[0].branch.as_deref(), Some("main"));
        assert!(!r[0].bare);
        assert_eq!(r[1].branch.as_deref(), Some("feature/foo"));
        assert_eq!(r[2].branch, None);
        assert!(r[2].detached);
    }

    #[test]
    fn captures_prunable_locked_and_bare() {
        let out = "worktree /bare\nHEAD 0000\nbare\n\nworktree /wt/locked\nHEAD 4444\nbranch refs/heads/x\nlocked reason here\n\nworktree /wt/gone\nHEAD 5555\nbranch refs/heads/gone\nprunable gitdir file points to non-existent location\n";
        let r = parse_worktree_porcelain(out);
        assert!(r[0].bare);
        assert!(r[1].locked);
        assert!(r[2].prunable);
    }

    #[test]
    fn status_reads_branch_headers() {
        let s = parse_status_porcelain_v2(
            "# branch.oid abc123\n# branch.head feature\n# branch.upstream origin/feature\n# branch.ab +2 -1\n",
        );
        assert_eq!(s.head.as_deref(), Some("feature"));
        assert!(!s.detached);
        assert_eq!(s.upstream.as_deref(), Some("origin/feature"));
        assert_eq!((s.ahead_upstream, s.behind_upstream), (2, 1));
    }

    #[test]
    fn status_detects_change_kinds() {
        let s = parse_status_porcelain_v2(
            "# branch.head main\n1 M. N... 100644 100644 100644 aaa bbb staged-only.txt\n1 .M N... 100644 100644 100644 ccc ddd unstaged-only.txt\n? untracked.txt\n",
        );
        assert!(s.has_staged && s.has_unstaged && s.has_untracked);
        let clean = parse_status_porcelain_v2("# branch.head main\n# branch.oid abc\n");
        assert!(!clean.has_staged && !clean.has_unstaged && !clean.has_untracked);
        assert!(clean.upstream.is_none());
        let conflict = parse_status_porcelain_v2(
            "# branch.head main\nu UU N... 1 1 1 1 h1 h2 h3 conflict.txt\n",
        );
        assert!(conflict.has_unstaged);
        let detached = parse_status_porcelain_v2("# branch.head (detached)");
        assert!(detached.detached && detached.head.is_none());
    }

    #[test]
    fn ref_candidates_drop_symrefs_and_blanks() {
        let out = "main\t\nfeature/foo\t\norigin/main\t\norigin/HEAD\trefs/remotes/origin/main\n\n";
        assert_eq!(
            parse_ref_candidates(out),
            vec!["main", "feature/foo", "origin/main"]
        );
        assert!(parse_ref_candidates("").is_empty());
    }

    #[test]
    fn left_right_count_maps_behind_then_ahead() {
        assert_eq!(parse_left_right_count("3\t5\n"), (3, 5));
        assert_eq!(parse_left_right_count("0 0"), (0, 0));
        assert_eq!(parse_left_right_count(""), (0, 0));
    }
}
