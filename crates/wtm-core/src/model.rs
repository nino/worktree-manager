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
    /// The repo's remotes, in `git remote` order. Neither this nor
    /// `remote_branches` is in the snapshot: only the New Worktree sheet reads
    /// them, and the first listing after launch fills them in.
    #[serde(skip)]
    pub remotes: Vec<String>,
    /// Remote-tracking branches as `<remote>/<branch>`, for finding a branch
    /// that only a remote has.
    #[serde(skip)]
    pub remote_branches: Vec<String>,
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
            remotes: Vec::new(),
            remote_branches: Vec::new(),
            error: None,
            loaded: false,
        }
    }

    pub fn worktree(&self, path: &str) -> Option<&WorktreeInfo> {
        self.worktrees.iter().find(|w| w.path == path)
    }

    /// Where the last listing saw a branch called `branch`. Each remote is
    /// asked for `<remote>/<branch>`, as git maps a remote's branches, rather
    /// than each ref being split at a slash: a remote's name can have one.
    pub fn locate_branch(&self, branch: &str) -> BranchLocation {
        if let Some(w) = self
            .worktrees
            .iter()
            .find(|w| w.branch.as_deref() == Some(branch))
        {
            return BranchLocation::CheckedOut {
                missing: w.prunable,
            };
        }
        if self.branches.iter().any(|b| b == branch) {
            return BranchLocation::Local;
        }
        let mut on: Vec<String> = self
            .remotes
            .iter()
            .filter(|remote| {
                let tracking = format!("{remote}/{branch}");
                self.remote_branches.iter().any(|r| *r == tracking)
            })
            .cloned()
            .collect();
        match on.len() {
            // Only a listing that ran and worked can say a branch is absent;
            // a cached snapshot has no remote branches, a failed one nothing.
            0 if !self.loaded || self.error.is_some() => BranchLocation::Unknown,
            0 => BranchLocation::Nowhere,
            1 => BranchLocation::Remote(on.remove(0)),
            _ => BranchLocation::Remotes(on),
        }
    }
}

/// Where a branch named in the New Worktree sheet is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchLocation {
    /// A worktree of the repo already has that branch checked out;
    /// `missing` when git says its folder is gone.
    CheckedOut { missing: bool },
    /// There is a local branch of that name, not checked out anywhere.
    Local,
    /// Not a local branch, and this remote is the only one that has it.
    Remote(String),
    /// Not a local branch, and each of these remotes has one of that name,
    /// so which is meant cannot be told.
    Remotes(Vec<String>),
    /// No branch of that name in a completed listing.
    Nowhere,
    /// Not found, but the repo has not been listed yet, or its listing
    /// failed, so it may still exist.
    Unknown,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn node(branches: &[&str], remotes: &[&str], remote_branches: &[&str]) -> RepoNode {
        let strings = |s: &[&str]| s.iter().map(|s| s.to_string()).collect();
        RepoNode {
            branches: strings(branches),
            remotes: strings(remotes),
            remote_branches: strings(remote_branches),
            loaded: true,
            ..RepoNode::placeholder(RepoConfig {
                id: "r".into(),
                name: "r".into(),
                path: "/r".into(),
                main_branch: "main".into(),
                init_command: String::new(),
                commands: Vec::new(),
            })
        }
    }

    #[test]
    fn a_local_branch_is_local_whatever_the_remotes_have() {
        let n = node(
            &["main", "fix"],
            &["origin"],
            &["origin/main", "origin/fix"],
        );
        assert_eq!(n.locate_branch("fix"), BranchLocation::Local);
    }

    #[test]
    fn a_branch_a_worktree_has_is_checked_out() {
        let mut n = node(&["main", "fix"], &[], &[]);
        n.worktrees.push(WorktreeInfo {
            path: "/wt/fix".into(),
            branch: Some("fix".into()),
            head: "abc".into(),
            is_main: false,
            locked: false,
            prunable: false,
            status: None,
        });
        assert_eq!(
            n.locate_branch("fix"),
            BranchLocation::CheckedOut { missing: false }
        );
        assert_eq!(n.locate_branch("main"), BranchLocation::Local);
        n.worktrees[0].prunable = true;
        assert_eq!(
            n.locate_branch("fix"),
            BranchLocation::CheckedOut { missing: true }
        );
    }

    #[test]
    fn absence_is_unknown_until_a_listing_has_worked() {
        let mut n = node(&["main"], &[], &[]);
        n.loaded = false;
        assert_eq!(n.locate_branch("x"), BranchLocation::Unknown);
        assert_eq!(n.locate_branch("main"), BranchLocation::Local);
        n.loaded = true;
        n.error = Some("git failed".into());
        assert_eq!(n.locate_branch("x"), BranchLocation::Unknown);
    }

    #[test]
    fn a_branch_one_remote_has_is_on_that_remote() {
        let n = node(
            &["main"],
            &["origin", "fork"],
            &["origin/main", "fork/review/x"],
        );
        assert_eq!(
            n.locate_branch("review/x"),
            BranchLocation::Remote("fork".into())
        );
    }

    #[test]
    fn a_branch_several_remotes_have_names_them_all_in_order() {
        let n = node(
            &["main"],
            &["origin", "upstream", "fork"],
            &["upstream/x", "origin/x", "fork/y"],
        );
        assert_eq!(
            n.locate_branch("x"),
            BranchLocation::Remotes(vec!["origin".into(), "upstream".into()])
        );
    }

    #[test]
    fn a_remote_with_a_slash_in_its_name_is_matched_whole() {
        let n = node(&[], &["team/alice"], &["team/alice/fix"]);
        assert_eq!(
            n.locate_branch("fix"),
            BranchLocation::Remote("team/alice".into())
        );
        assert_eq!(n.locate_branch("alice/fix"), BranchLocation::Nowhere);
    }

    #[test]
    fn nothing_is_found_where_nothing_is() {
        let n = node(&["main"], &["origin"], &["origin/main"]);
        assert_eq!(n.locate_branch("new-thing"), BranchLocation::Nowhere);
        assert_eq!(n.locate_branch(""), BranchLocation::Nowhere);
        // A remote-tracking ref of a remote that is gone from `git remote`.
        let n = node(&[], &[], &["old/x"]);
        assert_eq!(n.locate_branch("x"), BranchLocation::Nowhere);
    }
}
