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

/// Move the element of `list` whose key is `id` to just before the one whose
/// key is `before`, or to the end for `None`. Returns false, leaving the list
/// as it was, if either is missing, `before` is `id`, or it is already there.
pub fn move_before<T>(
    list: &mut Vec<T>,
    key: impl Fn(&T) -> &str,
    id: &str,
    before: Option<&str>,
) -> bool {
    let Some(from) = list.iter().position(|x| key(x) == id) else {
        return false;
    };
    if before.is_some_and(|b| b == id || !list.iter().any(|x| key(x) == b)) {
        return false;
    }
    let moved = list.remove(from);
    let to = match before {
        Some(b) => list.iter().position(|x| key(x) == b).unwrap_or(list.len()),
        None => list.len(),
    };
    list.insert(to, moved);
    to != from
}

/// Where a repo dragged among the `shown` repos goes when it is dropped in
/// the gap before `shown[gap]` (`gap == shown.len()` for after the last):
/// the repo of `all` it goes before, `None` for the end of the list. The
/// outer `None` is a drop that changes nothing — either gap beside the
/// dragged repo, or a repo or gap that is not there.
///
/// During a search `shown` holds only some of `all`. A drop beside the
/// dragged repo must not move it past a hidden one, and a drop after the
/// last repo shown puts it right after that one, not after every hidden
/// repo too.
pub fn drop_target<'a>(
    all: &[&'a str],
    shown: &[&'a str],
    dragged: &str,
    gap: usize,
) -> Option<Option<&'a str>> {
    let from = shown.iter().position(|s| *s == dragged)?;
    if gap == from || gap == from + 1 || gap > shown.len() {
        return None;
    }
    if let Some(next) = shown.get(gap) {
        return Some(Some(next));
    }
    let last = shown.last()?;
    let after = all.iter().position(|a| a == last)?;
    Some(all[after + 1..].iter().find(|a| **a != dragged).copied())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moved(all: &[&str], id: &str, before: Option<&str>) -> Vec<String> {
        let mut list: Vec<String> = all.iter().map(|s| s.to_string()).collect();
        move_before(&mut list, |s| s.as_str(), id, before);
        list
    }

    #[test]
    fn a_repo_moves_before_another_or_to_the_end() {
        let all = ["a", "b", "c"];
        assert_eq!(moved(&all, "c", Some("a")), ["c", "a", "b"]);
        assert_eq!(moved(&all, "a", None), ["b", "c", "a"]);
        assert_eq!(moved(&all, "a", Some("c")), ["b", "a", "c"]);
        let mut list = all.to_vec();
        for (id, before) in [
            ("a", Some("b")),
            ("c", None),
            ("a", Some("a")),
            ("a", Some("gone")),
            ("gone", None),
        ] {
            assert!(
                !move_before(&mut list, |s| s, id, before),
                "{id} {before:?}"
            );
        }
        assert_eq!(list, all);
    }

    #[test]
    fn a_drop_between_repos_lands_before_the_next() {
        let all = ["a", "b", "c", "d"];
        assert_eq!(drop_target(&all, &all, "d", 0), Some(Some("a")));
        assert_eq!(drop_target(&all, &all, "a", 2), Some(Some("c")));
        assert_eq!(drop_target(&all, &all, "a", 4), Some(None));
        // Either side of itself, or past the end.
        assert_eq!(drop_target(&all, &all, "b", 1), None);
        assert_eq!(drop_target(&all, &all, "b", 2), None);
        assert_eq!(drop_target(&all, &all, "b", 5), None);
        assert_eq!(drop_target(&all, &all, "x", 0), None);
    }

    #[test]
    fn a_drop_during_a_search_leaves_hidden_repos_where_they_were() {
        // A search shows a, c and e.
        let (all, shown) = (["a", "b", "c", "d", "e", "f"], ["a", "c", "e"]);
        // Beside itself: not past the hidden b.
        assert_eq!(drop_target(&all, &shown, "a", 1), None);
        // After the last one shown: right after e, still before the hidden f.
        assert_eq!(drop_target(&all, &shown, "a", 3), Some(Some("f")));
        assert_eq!(drop_target(&all, &shown, "c", 3), Some(Some("f")));
        let (all, shown) = (["a", "b", "c"], ["a", "b"]);
        assert_eq!(drop_target(&all, &shown, "a", 2), Some(Some("c")));
        let (all, shown) = (["a", "b", "c"], ["b", "c"]);
        assert_eq!(drop_target(&all, &shown, "b", 2), Some(None));
    }

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
