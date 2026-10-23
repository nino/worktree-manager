//! Worktree mutations: create, the delete safety ladder, push/pull/merge/switch.
//! Every function takes the already-resolved [`RepoConfig`] and returns a
//! result value; the caller ([`App`](crate::App)) owns locking and the model.

use std::path::Path;

use crate::branch_name;
use crate::git::{
    add_worktree, assert_valid_ref, branch_exists, fetch_branch, has_remote, list_worktrees_raw,
    lock_refs, ref_exists, resolve_trunk_ref, run_git, AddBranch, GitError,
};
use crate::paths::worktree_path_for;
use crate::types::{
    BranchSource, CreateWorktreeParams, DeleteRefusal, DeleteWorktreeParams, DeleteWorktreeResult,
    GitOpResult, RepoConfig,
};

/// Create a worktree; resolves with its on-disk path, and with a warning
/// when a branch from a remote could not be fetched first and starts where
/// the last fetch left it.
pub async fn create_worktree(
    repo: &RepoConfig,
    worktrees_root: &str,
    params: &CreateWorktreeParams,
) -> Result<(String, Option<String>), String> {
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
    // The name reaches `git fetch`, which would also take a URL or a path.
    if let BranchSource::Remote(remote) = &params.source {
        if !has_remote(&repo.path, remote).await {
            return Err(format!("There is no remote called \"{remote}\"."));
        }
    }

    // git resolves a relative path against the repo it runs in, and this
    // process against its own working folder, so the two would make the
    // worktree and its parent folder in different places.
    if !Path::new(worktrees_root).is_absolute() {
        return Err(format!(
            "The worktrees root must be a full path, not \"{worktrees_root}\". Change it in Settings."
        ));
    }
    let target = worktree_path_for(worktrees_root, &repo.name, branch);
    let target_str = target.to_string_lossy().into_owned();
    if tokio::fs::metadata(&target).await.is_ok() {
        return Err(format!("Target path already exists: {target_str}"));
    }
    tokio::fs::create_dir_all(Path::new(worktrees_root).join(&repo.name))
        .await
        .map_err(|e| format!("Cannot create {}: {e}", worktrees_root))?;

    let mut warning = None;
    let added = match &params.source {
        BranchSource::New { .. } => {
            let base = match base_ref {
                Some(b) => b.to_string(),
                None => resolve_trunk_ref(&repo.path, &repo.main_branch).await,
            };
            add_worktree(
                &repo.path,
                &target_str,
                branch,
                AddBranch::New { base: &base },
            )
            .await
        }
        BranchSource::Existing => {
            add_worktree(&repo.path, &target_str, branch, AddBranch::Existing).await
        }
        BranchSource::Remote(remote) => {
            // Held from the fetch to the checkout, so no other fetch of the
            // repo fails on the ref's lock or moves it in between.
            let _refs = lock_refs(&repo.path).await;
            let upstream;
            (upstream, warning) = fetch_upstream(repo, remote, branch).await?;
            let how = AddBranch::Tracking {
                upstream: &upstream,
            };
            add_worktree(&repo.path, &target_str, branch, how).await
        }
    };
    added.map_err(|e| e.detail())?;
    Ok((target_str, warning))
}

/// Fetch `branch` from `remote` and return its remote-tracking ref, so the
/// new worktree starts where the remote is now rather than where it was at
/// the last fetch. Offline, the last fetch is what there is, and the second
/// value says so.
async fn fetch_upstream(
    repo: &RepoConfig,
    remote: &str,
    branch: &str,
) -> Result<(String, Option<String>), String> {
    let warning = fetch_branch(&repo.path, remote, branch)
        .await
        .err()
        .map(|e| {
            format!(
                "Could not fetch \"{branch}\" from {remote}, so it starts where the last fetch left it: {}",
                e.detail()
            )
        });
    let upstream = format!("refs/remotes/{remote}/{branch}");
    if ref_exists(&repo.path, &upstream).await {
        Ok((upstream, warning))
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
    match remove_worktree(repo, &params.worktree_path, false).await {
        Ok(()) => return DeleteWorktreeResult::success(""),
        Err(message) => {
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
    match remove_worktree(repo, &params.worktree_path, true).await {
        Ok(()) => DeleteWorktreeResult::success(""),
        Err(message) => DeleteWorktreeResult::refused(DeleteRefusal::Error, message),
    }
}

/// `git worktree remove`, finishing the job when git stops halfway.
///
/// git checks the tree, deletes the folder, then drops its own bookkeeping
/// whether or not the folder went. When something writes into the folder
/// while it is being deleted (a dev server, a build watcher, an editor, a
/// Claude session), the last `rmdir` fails with "Directory not empty", the
/// worktree is no longer registered, and a stray folder is left that the
/// list no longer shows. Deleting what is left is what the user asked for,
/// and git's dirty-tree check has already passed (or was overridden), so it
/// is removed here. git's list decides, not the wording of its error, which
/// changes with the locale: a refusal (a dirty tree, a lock) leaves the
/// worktree registered and is passed on untouched.
async fn remove_worktree(
    repo: &RepoConfig,
    worktree_path: &str,
    force: bool,
) -> Result<(), String> {
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(worktree_path);
    let message = match run_git(&repo.path, &args).await {
        Ok(_) => return Ok(()),
        Err(e) => e.detail(),
    };
    match list_worktrees_raw(&repo.path).await {
        Ok(all) if !all.iter().any(|w| w.path == worktree_path) => {}
        _ => return Err(message),
    }
    remove_leftover(Path::new(worktree_path))
        .await
        .map_err(|e| leftover_message(worktree_path, &e))
}

/// Delete a folder git has already unregistered, retrying for a moment in
/// case whatever was writing there is still finishing.
async fn remove_leftover(path: &Path) -> std::io::Result<()> {
    let attempt = || async {
        match tokio::fs::remove_dir_all(path).await {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    };
    for delay_ms in [250, 1000] {
        if attempt().await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
    }
    attempt().await
}

fn leftover_message(worktree_path: &str, error: &std::io::Error) -> String {
    let cause = if error.kind() == std::io::ErrorKind::DirectoryNotEmpty {
        "Something is probably still writing into it, such as a dev server, a build \
         watcher, an editor or a Claude session. Stop it, then delete the folder"
    } else {
        "Delete the folder by hand"
    };
    format!(
        "The worktree was removed from git, but its folder could not be deleted ({error}). \
         {cause}: {worktree_path}"
    )
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

    /// Something writing into the worktree while git deletes it makes git
    /// fail with "Directory not empty" after it has already unregistered
    /// the worktree. The delete still succeeds and no folder is left.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_folder_written_to_during_delete_is_still_removed() {
        let dir = std::env::temp_dir().join(format!("wtm-busy-delete-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let main = dir.join("main");
        std::fs::create_dir_all(&main).unwrap();
        let git = |args: &[&str]| {
            let ok = std::process::Command::new("git")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .args([
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@t",
                    "-c",
                    "init.defaultBranch=main",
                ])
                .args(args)
                .current_dir(&main)
                .status()
                .unwrap()
                .success();
            assert!(ok, "git {args:?}");
        };
        git(&["init", "--quiet"]);
        std::fs::write(main.join(".gitignore"), "cache/\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "--quiet", "-m", "init"]);
        let wt = dir.join("wt");
        let wt_path = wt.to_string_lossy().into_owned();
        git(&["worktree", "add", "--quiet", "-b", "feat", &wt_path]);
        // Ignored files: git deletes them without counting the tree dirty,
        // and enough of them that the delete takes a while.
        for i in 0..2000 {
            let d = wt.join("cache").join(format!("d{i}"));
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("f"), "x").unwrap();
        }

        // A writer like a dev server's: keeps writing into a cache folder
        // for a moment, then stops. `create_dir`, not `create_dir_all`, so
        // it cannot bring the worktree back once the delete has finished.
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let writer = {
            let stop = stop.clone();
            let busy = wt.join("cache").join("busy");
            std::thread::spawn(move || {
                let started = std::time::Instant::now();
                let mut i = 0;
                while !stop.load(std::sync::atomic::Ordering::Relaxed)
                    && started.elapsed() < std::time::Duration::from_millis(400)
                {
                    let _ = std::fs::create_dir(&busy);
                    let _ = std::fs::write(busy.join(format!("n{i}")), "x");
                    i += 1;
                }
            })
        };
        std::thread::sleep(std::time::Duration::from_millis(50));

        let repo = RepoConfig {
            id: "r".into(),
            name: "r".into(),
            path: main.to_string_lossy().into_owned(),
            main_branch: "main".into(),
            init_command: String::new(),
            commands: Vec::new(),
        };
        let params = DeleteWorktreeParams {
            repo_id: "r".into(),
            worktree_path: wt_path.clone(),
            expected_branch: Some("feat".into()),
            force: false,
        };
        let result = delete_worktree(&repo, &params).await;
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        writer.join().unwrap();
        assert!(result.ok, "{}", result.message);
        assert!(!wt.exists(), "no folder is left behind");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_that_will_not_go_says_what_to_do() {
        let e = std::io::Error::from_raw_os_error(39);
        let message = leftover_message("/w/feat", &e);
        assert!(
            message.starts_with("The worktree was removed from git"),
            "{message}"
        );
        assert!(message.contains("dev server"), "{message}");
        let other = leftover_message("/w/feat", &std::io::Error::from_raw_os_error(13));
        assert!(!other.contains("dev server"), "{other}");
        assert!(message.ends_with("/w/feat"), "{message}");
    }

    #[tokio::test]
    async fn a_relative_worktrees_root_is_refused() {
        // A real repository: the refusal comes after the checks that need one.
        let dir = std::env::temp_dir().join(format!("wtm-relative-root-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        run_git(&dir, &["init", "--quiet"]).await.unwrap();
        let repo = RepoConfig {
            id: "r".into(),
            name: "r".into(),
            path: dir.to_string_lossy().into_owned(),
            main_branch: "main".into(),
            init_command: String::new(),
            commands: Vec::new(),
        };
        let params = CreateWorktreeParams {
            repo_id: "r".into(),
            branch: "x".into(),
            source: BranchSource::New { base_ref: None },
        };
        let err = create_worktree(&repo, "~/wt", &params).await.unwrap_err();
        assert!(
            err.starts_with("The worktrees root must be a full path"),
            "{err}"
        );
        assert!(!dir.join("~").exists(), "nothing is made inside the repo");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
