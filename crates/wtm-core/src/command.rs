//! Pure helpers for building shell command lines.

/// Quote a string for safe use inside a POSIX shell command.
pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Combine a configured command with a target path.
///
/// If the command contains `{path}` placeholders, each is replaced with the
/// quoted path (e.g. `open -na Ghostty --args --working-directory={path}`).
/// Otherwise the quoted path is appended as a final argument (e.g. `code`).
pub fn build_command(command: &str, target_path: &str) -> String {
    let quoted = shell_quote(target_path);
    if command.contains("{path}") {
        command.replace("{path}", &quoted)
    } else {
        format!("{command} {quoted}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_single_quotes() {
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn appends_path_when_no_placeholder() {
        assert_eq!(build_command("code", "/a b"), "code '/a b'");
    }

    #[test]
    fn substitutes_every_placeholder() {
        assert_eq!(
            build_command("open -a X {path} && echo {path}", "/p"),
            "open -a X '/p' && echo '/p'"
        );
    }
}
