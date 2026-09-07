//! Pure path helpers.

use std::path::{Path, PathBuf};

/// Abbreviate the user's home directory to `~` for display purposes.
pub fn tildify(path: &str, home: &str) -> String {
    if home.is_empty() {
        return path.to_string();
    }
    if path == home {
        return "~".to_string();
    }
    if let Some(rest) = path.strip_prefix(home) {
        if rest.starts_with('/') {
            return format!("~{rest}");
        }
    }
    path.to_string()
}

/// Turn a branch name into a filesystem-safe directory segment.
pub fn slugify_branch(branch: &str) -> String {
    let mut out = String::with_capacity(branch.len());
    let mut last_dash = false;
    for ch in branch.chars() {
        let mapped = match ch {
            '/' | '\\' => '-',
            c if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' => c,
            _ => '-',
        };
        if mapped == '-' {
            if last_dash {
                continue;
            }
            last_dash = true;
        } else {
            last_dash = false;
        }
        out.push(mapped);
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "worktree".to_string()
    } else {
        trimmed.to_string()
    }
}

/// The on-disk path for a new worktree: `<root>/<repo name>/<branch slug>`.
pub fn worktree_path_for(worktrees_root: &str, repo_name: &str, branch: &str) -> PathBuf {
    Path::new(worktrees_root)
        .join(repo_name)
        .join(slugify_branch(branch))
}

/// Repo display names double as a directory segment under the worktrees root,
/// so they must never contain path separators or traversal sequences.
pub fn sanitize_repo_name(name: &str) -> String {
    let cleaned: String = name
        .replace(['/', '\\'], "-")
        .replace("..", "-")
        .trim()
        .trim_start_matches('.')
        .to_string();
    if cleaned.is_empty() {
        "repo".to_string()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tildify_abbreviates_home() {
        assert_eq!(tildify("/Users/me/x", "/Users/me"), "~/x");
        assert_eq!(tildify("/Users/me", "/Users/me"), "~");
        assert_eq!(tildify("/Users/meow/x", "/Users/me"), "/Users/meow/x");
        assert_eq!(tildify("/tmp", ""), "/tmp");
    }

    #[test]
    fn slug_replaces_slashes_with_dashes() {
        assert_eq!(slugify_branch("feature/foo"), "feature-foo");
        assert_eq!(slugify_branch("release/v1.2.3"), "release-v1.2.3");
    }

    #[test]
    fn slug_strips_unusual_characters_and_collapses_dashes() {
        assert_eq!(slugify_branch("feat/@weird  name!"), "feat-weird-name");
    }

    #[test]
    fn slug_falls_back_for_empty_results() {
        assert_eq!(slugify_branch("///"), "worktree");
    }

    #[test]
    fn worktree_path_nests_under_root_repo_slug() {
        assert_eq!(
            worktree_path_for("/home/me/.claude-worktrees", "myrepo", "feature/x"),
            PathBuf::from("/home/me/.claude-worktrees/myrepo/feature-x")
        );
    }

    #[test]
    fn sanitize_repo_name_rejects_traversal() {
        assert_eq!(sanitize_repo_name("../etc"), "--etc");
        assert_eq!(sanitize_repo_name("a/b"), "a-b");
        assert_eq!(sanitize_repo_name("  "), "repo");
    }
}
