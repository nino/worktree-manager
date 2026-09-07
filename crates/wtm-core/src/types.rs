//! Data types shared between the core and every UI backend. Field names are
//! serialised in camelCase so the file format matches the Electron app's
//! `worktree-manager.json` byte for byte in meaning, allowing a one-time import.

use serde::{Deserialize, Serialize};

/// A named command a repo can run inside a worktree (e.g. `pnpm dev`). Kept for
/// config round-tripping; the command runner itself is not part of this build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoCommand {
    pub id: String,
    pub name: String,
    pub command: String,
}

/// Per-repository configuration, persisted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoConfig {
    /// Stable unique id.
    pub id: String,
    /// Display name (defaults to the repo directory basename). Doubles as a
    /// directory segment under the worktrees root.
    pub name: String,
    /// Absolute path to the repository root (the primary working tree).
    pub path: String,
    /// Trunk branch. Worktrees compare against `origin/<main_branch>` when
    /// that exists, else the local branch.
    pub main_branch: String,
    /// Command run inside a new worktree after it is created (e.g. `pnpm i`).
    #[serde(default)]
    pub init_command: String,
    /// Configurable per-worktree commands (preserved, not executed here).
    #[serde(default)]
    pub commands: Vec<RepoCommand>,
}

/// Global, app-wide configuration, persisted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    /// Root directory that all worktrees are created under.
    pub worktrees_root: String,
    /// Editor command used by "Open in editor" (e.g. `code`, or an absolute path).
    pub editor_command: String,
    /// Configured repositories.
    #[serde(default)]
    pub repos: Vec<RepoConfig>,
}

/// The app-wide settings editable in the Settings dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSettings {
    pub worktrees_root: String,
    pub editor_command: String,
}

/// A path that could not be added as a repository, with the reason why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddRepoFailure {
    pub path: String,
    pub message: String,
}

/// Outcome of adding one or more repositories. Adding is per-path best effort,
/// so both lists can be non-empty.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AddReposResult {
    /// Display names of the repos that were added, in order.
    pub added: Vec<String>,
    /// Paths that were rejected.
    pub failed: Vec<AddRepoFailure>,
}

/// Git status of a single worktree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeStatus {
    pub has_unstaged: bool,
    pub has_staged: bool,
    pub has_untracked: bool,
    /// The ref the ahead/behind counts are measured against.
    pub trunk_ref: String,
    /// Commits ahead of `trunk_ref`; `None` if unknown.
    pub ahead_of_main: Option<u32>,
    /// Commits behind `trunk_ref`; `None` if unknown.
    pub behind_main: Option<u32>,
    /// True if the branch has an upstream and local commits are unpushed.
    pub unpushed: bool,
    /// Number of commits not pushed to upstream (0 if no upstream).
    pub unpushed_count: u32,
    /// True if the branch tracks an upstream remote.
    pub has_upstream: bool,
}

impl WorktreeStatus {
    /// Any staged, unstaged or untracked change.
    pub fn is_dirty(&self) -> bool {
        self.has_staged || self.has_unstaged || self.has_untracked
    }
}

/// A single worktree belonging to a repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeInfo {
    /// Absolute path to the worktree.
    pub path: String,
    /// Checked-out branch name, or `None` if detached.
    pub branch: Option<String>,
    /// Short HEAD sha.
    pub head: String,
    /// True for the repository's primary working tree.
    pub is_main: bool,
    /// True if the worktree is locked.
    pub locked: bool,
    /// True if git reports the folder as missing (deleted outside the app).
    pub prunable: bool,
    /// Git status; `None` if it could not be computed.
    pub status: Option<WorktreeStatus>,
}

/// Parameters for creating a new worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateWorktreeParams {
    pub repo_id: String,
    /// Branch name to create/check out in the new worktree.
    pub branch: String,
    /// If true, create a new branch; otherwise check out an existing one.
    pub new_branch: bool,
    /// Optional base ref for a new branch (defaults to the repo's trunk ref).
    pub base_ref: Option<String>,
}

/// Outcome of a git operation triggered from the UI (push/pull/switch…).
/// Never an error type: failures carry git's own message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitOpResult {
    pub ok: bool,
    /// git's output (stdout on success, stderr on failure).
    pub message: String,
}

/// Parameters for deleting a worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteWorktreeParams {
    pub repo_id: String,
    pub worktree_path: String,
    /// Branch the UI showed when the user confirmed. Deletion is refused if
    /// the worktree has since switched branches (stale-row protection).
    pub expected_branch: Option<String>,
    /// Destroy uncommitted changes too (requires a second confirmation).
    pub force: bool,
}

/// Why a delete did not happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteRefusal {
    /// Worktree has uncommitted changes; retry with `force` after re-confirming.
    Dirty,
    /// The worktree no longer matches what the UI showed; re-check.
    Changed,
    /// Anything else (the message has details).
    Error,
}

/// Outcome of a delete request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteWorktreeResult {
    pub ok: bool,
    pub reason: Option<DeleteRefusal>,
    pub message: String,
}

impl DeleteWorktreeResult {
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            ok: true,
            reason: None,
            message: message.into(),
        }
    }

    pub fn refused(reason: DeleteRefusal, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            reason: Some(reason),
            message: message.into(),
        }
    }
}
