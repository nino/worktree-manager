//! The UI's view of the world: one immutable snapshot at a time.
//!
//! The [`App`](crate::App) replaces the snapshot wholesale on every change and
//! notifies listeners; a UI diffs the new snapshot against the one it last
//! rendered and touches only the rows that differ. Keeping the model a plain
//! value (no interior mutability, cheap to clone) is what makes that diffing
//! trivial and thread-safe.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::types::{AppConfig, RepoConfig, WorktreeInfo};

/// What an in-flight operation is doing to a worktree, for row spinners and
/// for disabling controls that would conflict with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Busy {
    Pushing,
    Pulling,
    Merging,
    Switching,
    Deleting,
}

impl Busy {
    pub fn label(self) -> &'static str {
        match self {
            Busy::Pushing => "Pushing…",
            Busy::Pulling => "Pulling…",
            Busy::Merging => "Merging…",
            Busy::Switching => "Switching…",
            Busy::Deleting => "Deleting…",
        }
    }
}

/// A worktree creation that has been requested but not yet listed by git,
/// shown as a placeholder row at its final sort position; a failure stays in
/// the list (with `error`) until dismissed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingCreation {
    pub id: u64,
    pub repo_id: String,
    pub branch: String,
    pub error: Option<String>,
}

/// A repository together with everything the UI shows for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoNode {
    pub repo: RepoConfig,
    pub worktrees: Vec<WorktreeInfo>,
    /// Preferred base ref when creating a new branch (`origin/<trunk>` when
    /// fetched, else the local trunk); also what statuses compare against.
    pub default_base_ref: String,
    /// Local branches, for the switch popup.
    pub branches: Vec<String>,
    /// Local + remote-tracking branches, for the base-ref picker.
    pub base_ref_candidates: Vec<String>,
    /// Populated if listing worktrees failed.
    pub error: Option<String>,
    /// False until the first listing after launch has completed (a cached
    /// snapshot may still be on screen).
    #[serde(skip)]
    pub loaded: bool,
}

impl RepoNode {
    pub fn placeholder(repo: RepoConfig) -> Self {
        Self {
            default_base_ref: repo.main_branch.clone(),
            repo,
            worktrees: Vec::new(),
            branches: Vec::new(),
            base_ref_candidates: Vec::new(),
            error: None,
            loaded: false,
        }
    }

    pub fn worktree(&self, path: &str) -> Option<&WorktreeInfo> {
        self.worktrees.iter().find(|w| w.path == path)
    }
}

/// Tone of the one-line notice under the toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Info,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub tone: Tone,
    pub text: String,
    /// Monotonic id so a UI can tell a re-issued identical notice from a stale one.
    pub id: u64,
}

/// The complete UI state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    pub config: AppConfig,
    pub repos: Vec<RepoNode>,
    /// In-flight operations keyed by worktree path.
    pub busy: BTreeMap<String, Busy>,
    pub pending: Vec<PendingCreation>,
    pub notice: Option<Notice>,
    /// A full refresh is running.
    pub refreshing: bool,
    /// The background `git fetch` cycle is running.
    pub fetching: bool,
    /// The user's home directory, for `~` abbreviation.
    pub home: String,
}

impl Model {
    pub fn repo(&self, repo_id: &str) -> Option<&RepoNode> {
        self.repos.iter().find(|r| r.repo.id == repo_id)
    }

    pub fn repo_mut(&mut self, repo_id: &str) -> Option<&mut RepoNode> {
        self.repos.iter_mut().find(|r| r.repo.id == repo_id)
    }

    pub fn pending_for<'a>(
        &'a self,
        repo_id: &'a str,
    ) -> impl Iterator<Item = &'a PendingCreation> + 'a {
        self.pending.iter().filter(move |p| p.repo_id == repo_id)
    }

    pub fn busy_for(&self, path: &str) -> Option<Busy> {
        self.busy.get(path).copied()
    }

    /// Bring the repo nodes in line with the configuration: same order as the
    /// config, placeholders for new repos, removed repos dropped, edited repo
    /// settings applied while keeping listed worktrees.
    pub fn sync_repos_with_config(&mut self) {
        let old = std::mem::take(&mut self.repos);
        let mut old: BTreeMap<String, RepoNode> =
            old.into_iter().map(|n| (n.repo.id.clone(), n)).collect();
        self.repos = self
            .config
            .repos
            .iter()
            .map(|repo| match old.remove(&repo.id) {
                Some(mut node) => {
                    node.repo = repo.clone();
                    node
                }
                None => RepoNode::placeholder(repo.clone()),
            })
            .collect();
    }
}
