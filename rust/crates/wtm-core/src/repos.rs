//! Adding repositories by path.

use std::path::{Path, PathBuf};

use crate::git::{detect_main_branch, resolve_repo_root, GitError};

/// Turn whatever `resolve_repo_root`/`detect_main_branch` failed with into one
/// short line for the UI.
pub fn describe_add_failure(err: &GitError) -> String {
    let raw = &err.message;
    let lower = raw.to_ascii_lowercase();
    if lower.contains("not a git repository") {
        return "Not a git repository".to_string();
    }
    if raw.contains("ENOENT") || raw.contains("ENOTDIR") || lower.contains("no such file") {
        return "Folder not found".to_string();
    }
    let without_prefix = match raw.find(" failed: ") {
        Some(i) if raw.starts_with("git ") => &raw[i + " failed: ".len()..],
        _ => raw.as_str(),
    };
    let first = without_prefix.lines().next().unwrap_or("").trim();
    let first = first.strip_prefix("fatal: ").unwrap_or(first);
    if first.is_empty() {
        "Could not add this folder".to_string()
    } else {
        first.to_string()
    }
}

/// git needs a directory to run in, and a drop can hand us a file. Fall back
/// to the containing folder; an unreadable path is passed through so git
/// reports it.
pub async fn containing_directory(path: &Path) -> PathBuf {
    match tokio::fs::metadata(path).await {
        Ok(m) if !m.is_dir() => path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| path.to_path_buf()),
        _ => path.to_path_buf(),
    }
}

/// Resolve a picked path to `(repo root, main branch)`.
pub async fn inspect_repo(path: &Path) -> Result<(String, String), GitError> {
    let dir = containing_directory(path).await;
    let root = resolve_repo_root(&dir).await?;
    let main = detect_main_branch(&root).await;
    Ok((root, main))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_error(stderr: &str) -> GitError {
        GitError {
            message: format!("git rev-parse --show-toplevel failed: {stderr}"),
            cwd: "/tmp".into(),
            args: vec![],
            stderr: stderr.into(),
        }
    }

    #[test]
    fn recognises_non_repository() {
        let e = git_error("fatal: not a git repository (or any of the parent directories): .git");
        assert_eq!(describe_add_failure(&e), "Not a git repository");
    }

    #[test]
    fn recognises_missing_folder() {
        let e = GitError {
            message: "spawn git ENOENT".into(),
            cwd: "".into(),
            args: vec![],
            stderr: "".into(),
        };
        assert_eq!(describe_add_failure(&e), "Folder not found");
    }

    #[test]
    fn keeps_gits_wording_without_prefix() {
        let e = git_error(
            "fatal: detected dubious ownership in repository at '/srv/app'\nTo add an\nexception",
        );
        assert_eq!(
            describe_add_failure(&e),
            "detected dubious ownership in repository at '/srv/app'"
        );
    }

    #[test]
    fn falls_back_for_empty() {
        let e = GitError {
            message: "".into(),
            cwd: "".into(),
            args: vec![],
            stderr: "".into(),
        };
        assert_eq!(describe_add_failure(&e), "Could not add this folder");
    }
}
