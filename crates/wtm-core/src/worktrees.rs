//! Worktree mutations: create, the delete safety ladder, push/pull/merge/switch.
//! Every function takes the already-resolved [`RepoConfig`] and returns a
//! result value; the caller ([`App`](crate::App)) owns locking and the model.

use std::path::Path;

use log::warn;

use crate::branch_name;
use crate::git::{
    add_tracking_worktree, add_worktree, assert_valid_ref, branch_exists, fetch_branch, has_remote,
    list_worktrees_raw, ref_exists, resolve_trunk_ref, run_git, GitError,
};
use crate::paths::worktree_path_for;
use crate::types::{
    BranchSource, CreateWorktreeParams, DeleteRefusal, DeleteWorktreeParams, DeleteWorktreeResult,
    GitOpResult, RepoConfig,
};

/// Create a worktree; resolves with its on-disk path.
pub async fn create_worktree(
    repo: &RepoConfig,
    worktrees_root: &str,
    params: &CreateWorktreeParams,
) -> Result<String, String> {
    let branch = params.branch.trim();
    if branch.is_empty() {
        return Err("A branch name is required.".into());
    }
    // Before the name: git cannot start in a folder that is not there, and
    // the check below would blame the name for that.
    if tokio::fs::metadata(&repo.path).await.is_err() {
        return Err(format!("The repository folder is missing: {}", repo.path));
    }
    // The reason the dialog gives while the name is typed; git's own check
    // below stays the authority.
    if let Some(problem) = branch_name::problem(branch) {
        return Err(problem);
    }
    assert_valid_ref(&repo.path, branch)
        .await
        .map_err(|_| format!("Not a valid branch name: {branch}"))?;
    let base_ref = match &params.source {
        BranchSource::New { base_ref } => base_ref.as_deref().map(str::trim),
        _ => None,
    }
    .filter(|b| !b.is_empty());
    if let Some(b) = base_ref {
        assert_valid_ref(&repo.path, b)
            .await
            .map_err(|_| format!("Not a valid base ref: {b}"))?;
    }
    let exists = branch_exists(&repo.path, branch).await;
    match (&params.source, exists) {
        (BranchSource::New { .. } | BranchSource::Remote(_), true) => {
            return Err(format!("Branch \"{branch}\" already exists."));
        }
        (BranchSource::Existing, false) => {
            return Err(format!("Branch \"{branch}\" does not exist."));
        }
        _ => {}
    }

    let target = worktree_path_for(worktrees_root, &repo.name, branch);
    let target_str = target.to_string_lossy().into_owned();
    if tokio::fs::metadata(&target).await.is_ok() {
        return Err(format!("Target path already exists: {target_str}"));
    }
    tokio::fs::create_dir_all(Path::new(worktrees_root).join(&repo.name))
        .await
        .map_err(|e| format!("Cannot create {}: {e}", worktrees_root))?;

    let added = match &params.source {
        BranchSource::New { .. } => {
            let base = match base_ref {
                Some(b) => b.to_string(),
                None => resolve_trunk_ref(&repo.path, &repo.main_branch).await,
            };
            add_worktree(&repo.path, &target_str, branch, true, Some(&base)).await
        }
        BranchSource::Existing => add_worktree(&repo.path, &target_str, branch, false, None).await,
        BranchSource::Remote(remote) => {
            let upstream = fetch_upstream(repo, remote, branch).await?;
            add_tracking_worktree(&repo.path, &target_str, branch, &upstream).await
        }
    };
    added.map_err(|e| e.detail())?;
    Ok(target_str)
}

/// Fetch `branch` from `remote` and return its remote-tracking ref, so the
/// new worktree starts where the remote is now rather than where it was at
/// the last fetch. Offline, the last fetch is what there is.
async fn fetch_upstream(repo: &RepoConfig, remote: &str, branch: &str) -> Result<String, String> {
    if let Err(e) = fetch_branch(&repo.path, remote, branch).await {
        warn!("fetching {branch} from {remote}: {}", e.detail());
    }
    let upstream = format!("refs/remotes/{remote}/{branch}");
    if ref_exists(&repo.path, &upstream).await {
        Ok(upstream)
    } else {
        Err(format!("{remote} has no branch \"{branch}\"."))
    }
}

/// Delete a worktree.
///
/// Safety ladder:
/// 1. The path must be one of the repo's worktrees, verbatim per git's list.
/// 2. The primary working tree is never deletable.
/// 3. The worktree must still be on the branch the UI showed.
/// 4. `git worktree remove` runs WITHOUT `--force` first, so git's own
///    dirty-tree protection applies; only `force` escalates.
/// 5. Prunable worktrees are cleaned with `git worktree prune`, refused when
///    other prunable worktrees exist (prune is repo-wide).
pub async fn delete_worktree(
    repo: &RepoConfig,
    params: &DeleteWorktreeParams,
) -> DeleteWorktreeResult {
    let all = match list_worktrees_raw(&repo.path).await {
        Ok(a) => a,
        Err(e) => return DeleteWorktreeResult::refused(DeleteRefusal::Error, e.detail()),
    };
    let Some(entry) = all.iter().find(|w| w.path == params.worktree_path) else {
        return DeleteWorktreeResult::refused(
            DeleteRefusal::Changed,
            format!(
                "{} is not (or no longer) a worktree of {}. Refresh and try again.",
                params.worktree_path, repo.name
            ),
        );
    };
    let primary = all.first().filter(|w| !w.bare).map(|w| w.path.as_str());
    if entry.bare || Some(entry.path.as_str()) == primary {
        return DeleteWorktreeResult::refused(
            DeleteRefusal::Error,
            "Refusing to delete the repository's primary working tree.",
        );
    }
    if entry.branch != params.expected_branch {
        let show = |b: &Option<String>| b.clone().unwrap_or_else(|| "(detached)".into());
        return DeleteWorktreeResult::refused(
            DeleteRefusal::Changed,
            format!(
                "This worktree is now on “{}”, not “{}”. Refresh and re-confirm.",
                show(&entry.branch),
                show(&params.expected_branch)
            ),
        );
    }
    if entry.prunable {
        let others: Vec<String> = all
            .iter()
            .filter(|w| w.prunable && w.path != entry.path)
            .map(|w| w.branch.clone().unwrap_or_else(|| w.path.clone()))
            .collect();
        if !others.is_empty() {
            return DeleteWorktreeResult::refused(
                DeleteRefusal::Error,
                format!(
                    "Other worktrees also have missing folders ({}). git can only prune them all together — if any live on an unmounted drive, remount it first, then delete these rows individually.",
                    others.join(", ")
                ),
            );
        }
        return match run_git(&repo.path, &["worktree", "prune"]).await {
            Ok(_) => {
                DeleteWorktreeResult::success("Cleaned up git bookkeeping for the missing folder.")
            }
            Err(e) => DeleteWorktreeResult::refused(DeleteRefusal::Error, e.detail()),
        };
    }
    // Without --force: git refuses dirty trees, which is exactly what we want.
    match run_git(&repo.path, &["worktree", "remove", &params.worktree_path]).await {
        Ok(_) => return DeleteWorktreeResult::success(""),
        Err(e) => {
            let message = e.detail();
            let lower = message.to_ascii_lowercase();
            let dirty = lower.contains("contains modified or untracked files")
                || lower.contains("use --force");
            if !dirty {
                return DeleteWorktreeResult::refused(DeleteRefusal::Error, message);
            }
            if !params.force {
                return DeleteWorktreeResult::refused(DeleteRefusal::Dirty, message);
            }
        }
    }
    match run_git(
        &repo.path,
        &["worktree", "remove", "--force", &params.worktree_path],
    )
    .await
    {
        Ok(_) => DeleteWorktreeResult::success(""),
        Err(e) => DeleteWorktreeResult::refused(DeleteRefusal::Error, e.detail()),
    }
}

// MARK: Per-worktree git operations

fn op_result(r: Result<String, GitError>) -> GitOpResult {
    match r {
        Ok(out) => GitOpResult {
            ok: true,
            message: out.trim().to_string(),
        },
        Err(e) => GitOpResult {
            ok: false,
            message: e.detail(),
        },
    }
}

/// Assert that `worktree_path` is one of the repo's worktrees.
async fn require_worktree(repo: &RepoConfig, worktree_path: &str) -> Result<(), GitOpResult> {
    let all = list_worktrees_raw(&repo.path)
        .await
        .map_err(|e| GitOpResult {
            ok: false,
            message: e.detail(),
        })?;
    if all.iter().any(|w| w.path == worktree_path) {
        Ok(())
    } else {
        Err(GitOpResult {
            ok: false,
            message: format!("Not a worktree of {}: {worktree_path}", repo.name),
        })
    }
}

/// Push the worktree's branch, setting upstream on first push.
pub async fn push(repo: &RepoConfig, worktree_path: &str) -> GitOpResult {
    if let Err(e) = require_worktree(repo, worktree_path).await {
        return e;
    }
    let first = op_result(run_git(worktree_path, &["push"]).await);
    let lower = first.message.to_ascii_lowercase();
    if !first.ok
        && (lower.contains("no upstream")
            || lower.contains("set-upstream")
            || lower.contains("no configured push destination"))
    {
        return op_result(run_git(worktree_path, &["push", "-u", "origin", "HEAD"]).await);
    }
    first
}

/// Fast-forward pull of the worktree's branch.
pub async fn pull(repo: &RepoConfig, worktree_path: &str) -> GitOpResult {
    if let Err(e) = require_worktree(repo, worktree_path).await {
        return e;
    }
    op_result(run_git(worktree_path, &["pull", "--ff-only"]).await)
}

/// Pull the repo's primary branch into this worktree's branch. Explicit
/// `--no-rebase` so a `pull.rebase` config can never rewrite history from a
/// button; `--no-edit` avoids editor prompts.
pub async fn pull_main(repo: &RepoConfig, worktree_path: &str) -> GitOpResult {
    if let Err(e) = require_worktree(repo, worktree_path).await {
        return e;
    }
    if assert_valid_ref(&repo.path, &repo.main_branch)
        .await
        .is_err()
    {
        return GitOpResult {
            ok: false,
            message: format!(
                "Configured main branch is not a valid ref: {}",
                repo.main_branch
            ),
        };
    }
    if has_remote(&repo.path, "origin").await {
        op_result(
            run_git(
                worktree_path,
                &[
                    "pull",
                    "--no-rebase",
                    "--no-edit",
                    "origin",
                    &repo.main_branch,
                ],
            )
            .await,
        )
    } else {
        op_result(run_git(worktree_path, &["merge", "--no-edit", &repo.main_branch]).await)
    }
}

/// Plain `git switch`: git refuses when local changes would be overwritten or
/// the branch is checked out elsewhere, so this cannot lose work.
pub async fn switch_branch(repo: &RepoConfig, worktree_path: &str, branch: &str) -> GitOpResult {
    if let Err(e) = require_worktree(repo, worktree_path).await {
        return e;
    }
    op_result(run_git(worktree_path, &["switch", branch]).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn creating_in_a_missing_repo_folder_says_so() {
        let repo = RepoConfig {
            id: "r".into(),
            name: "r".into(),
            path: "/nonexistent/wtm-missing-repo".into(),
            main_branch: "main".into(),
            init_command: String::new(),
            commands: Vec::new(),
        };
        let params = CreateWorktreeParams {
            repo_id: "r".into(),
            // A valid name, which this used to be refused as.
            branch: "e5x7or9".into(),
            source: BranchSource::New { base_ref: None },
        };
        let err = create_worktree(&repo, "/nonexistent/root", &params)
            .await
            .unwrap_err();
        assert_eq!(
            err,
            "The repository folder is missing: /nonexistent/wtm-missing-repo"
        );
    }
}
