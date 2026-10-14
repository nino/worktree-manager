//! `.worktreeinclude`, a Claude Code convention rather than a git one: a file
//! at the root of a repository, in `.gitignore` syntax, naming the files a
//! fresh worktree should be given from the main checkout (`.env` and the
//! like, which a checkout cannot bring along). A file is copied only if it
//! matches a pattern *and* git ignores it, so nothing tracked is ever
//! duplicated: <https://code.claude.com/docs/en/worktrees>.

use std::path::Path;

use crate::git::{run_git, run_git_with_input, GitError};

pub const FILE_NAME: &str = ".worktreeinclude";

/// Paths, relative to `repo_path`, of the files to copy: untracked files
/// that match a pattern in the repo's `.worktreeinclude`, kept only if git
/// ignores them. Empty when the repo has no such file.
pub async fn files_to_copy(repo_path: &str) -> Result<Vec<String>, GitError> {
    let include = Path::new(repo_path).join(FILE_NAME);
    if tokio::fs::metadata(&include).await.is_err() {
        return Ok(Vec::new());
    }
    // Git does the matching, so the patterns mean exactly what they would in
    // a .gitignore: `--exclude-from` takes them as the "ignore" rules for
    // this one listing, and `--others` leaves tracked files out.
    let exclude_from = format!("--exclude-from={}", include.to_string_lossy());
    let matched = run_git(
        repo_path,
        &["ls-files", "-z", "--others", "--ignored", &exclude_from],
    )
    .await?;
    if matched.is_empty() {
        return Ok(Vec::new());
    }
    // Then git's own ignore rules, separately: the listing above saw only
    // the include patterns.
    let ignored = match run_git_with_input(
        repo_path,
        &["check-ignore", "-z", "--stdin"],
        Some(matched.into_bytes()),
    )
    .await
    {
        Ok(out) => out,
        // Exit status 1 with nothing said: none of them is ignored.
        Err(e) if e.stderr.trim().is_empty() => String::new(),
        Err(e) => return Err(e),
    };
    Ok(split_nul(&ignored))
}

/// Copy `files` (relative paths) from `from` into `to`, never replacing
/// anything already there. Returns how many were copied, or what went wrong
/// with the first that could not be, after trying the rest.
pub async fn copy_files(from: &Path, to: &Path, files: &[String]) -> Result<usize, String> {
    let mut copied = 0;
    let mut failed = Vec::new();
    for rel in files {
        let dest = to.join(rel);
        if tokio::fs::symlink_metadata(&dest).await.is_ok() {
            continue;
        }
        let result = async {
            if let Some(dir) = dest.parent() {
                tokio::fs::create_dir_all(dir).await?;
            }
            tokio::fs::copy(from.join(rel), &dest).await
        }
        .await;
        match result {
            Ok(_) => copied += 1,
            Err(e) => failed.push(format!("{rel}: {e}")),
        }
    }
    match failed.first() {
        None => Ok(copied),
        Some(first) if failed.len() == 1 => Err(format!("Could not copy {first}")),
        Some(first) => Err(format!(
            "Could not copy {} files from {FILE_NAME}, among them {first}",
            failed.len()
        )),
    }
}

/// Copy what the repo's `.worktreeinclude` asks for into a new worktree.
pub async fn apply(repo_path: &str, worktree_path: &str) -> Result<usize, String> {
    let files = files_to_copy(repo_path)
        .await
        .map_err(|e| format!("Could not read {FILE_NAME}: {}", e.detail()))?;
    copy_files(Path::new(repo_path), Path::new(worktree_path), &files).await
}

fn split_nul(output: &str) -> Vec<String> {
    output
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_nul_separated_output() {
        assert_eq!(split_nul("a\0b c\0"), ["a", "b c"]);
        assert!(split_nul("").is_empty());
    }
}
