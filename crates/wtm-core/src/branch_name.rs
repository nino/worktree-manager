//! git's rules for a branch name, checked without running git, so the New
//! Worktree dialog can say what is wrong with a name while it is typed. They
//! are the rules `git check-ref-format --branch` applies, and the property
//! tests run git to hold the two together. git still has the last word when
//! the branch is created: a newer git may be stricter, and inside a repo it
//! also expands `@{-1}` (the previous branch), which this refuses.

/// Why git would refuse `name` as a branch name, or `None` if it would take
/// it. The name is checked as given; the dialog and the core trim it first.
pub fn problem(name: &str) -> Option<String> {
    if name.is_empty() {
        return Some("A branch name is required.".into());
    }
    // git's own refusals for a branch, on top of those for any ref: a
    // leading dash would read as an option, and HEAD is taken.
    if name.starts_with('-') {
        return Some("A branch name cannot start with “-”.".into());
    }
    if name == "HEAD" {
        return Some("“HEAD” cannot be a branch name.".into());
    }
    for c in name.chars() {
        if c == ' ' {
            return Some("A branch name cannot contain spaces.".into());
        }
        // git works on bytes: below 0x20 and DEL. Every byte of a multi-byte
        // character is 0x80 or above, so Unicode is fine.
        if c.is_ascii_control() {
            return Some("A branch name cannot contain control characters.".into());
        }
        if "~^:?*[\\".contains(c) {
            return Some(format!("A branch name cannot contain “{c}”."));
        }
    }
    for sequence in ["..", "@{", "//"] {
        if name.contains(sequence) {
            return Some(format!("A branch name cannot contain “{sequence}”."));
        }
    }
    if name.starts_with('/') {
        return Some("A branch name cannot start with “/”.".into());
    }
    if name.ends_with('/') {
        return Some("A branch name cannot end with “/”.".into());
    }
    if name.ends_with('.') {
        return Some("A branch name cannot end with “.”.".into());
    }
    for part in name.split('/') {
        if part.starts_with('.') {
            return Some("No part of a branch name can start with “.”.".into());
        }
        if part.ends_with(".lock") {
            return Some("No part of a branch name can end with “.lock”.".into());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_what_git_takes() {
        for name in [
            "main",
            "feature/login",
            "claude/fix-the-crash",
            "fix/ünïcödé-İstanbul",
            "😀",
            "@",
            "a@b",
            "a{b",
            "a]b",
            "x-",
            "x/HEAD",
            "HEAD/x",
            "head",
            "a.lockx",
            "a.lock.b",
            "a/-b",
            "refs/heads/x",
            "e5x7or9",
        ] {
            assert_eq!(problem(name), None, "{name:?}");
        }
    }

    #[test]
    fn refuses_what_git_refuses() {
        for name in [
            "", "-x", "-", "HEAD", "a b", "a\tb", "a\u{1}b", "a\u{7f}b", "a~b", "a^b", "a:b",
            "a?b", "a*b", "a[b", "a\\b", "a..b", "..", "@{-1}", "a@{b", "a//b", "/a", "a/", "a.",
            "a/b.", ".", ".a", "a/.b", "a/./b", ".lock", "a.lock", "a.lock/b",
        ] {
            assert!(problem(name).is_some(), "{name:?}");
        }
    }

    #[test]
    fn says_which_rule_a_name_breaks() {
        assert_eq!(
            problem("z@İ6 3lyru#0lt").as_deref(),
            Some("A branch name cannot contain spaces.")
        );
        assert_eq!(
            problem("fix~1").as_deref(),
            Some("A branch name cannot contain “~”.")
        );
        assert_eq!(
            problem("a/b.lock").as_deref(),
            Some("No part of a branch name can end with “.lock”.")
        );
    }
}
