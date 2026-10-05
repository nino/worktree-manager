//! Pure helpers for building shell command lines.

/// Quote a string for safe use inside a POSIX shell command.
pub fn shell_quote(value: &str) -> String {
    wtm_platform::posix_quote(value)
}

/// Combine a configured command with a target path.
///
/// If the command contains `{path}` placeholders, each is replaced with the
/// quoted path (e.g. `open -na Ghostty --args --working-directory={path}`).
/// Otherwise the quoted path is appended as a final argument (e.g. `code`).
pub fn build_command(command: &str, target_path: &str) -> String {
    build_command_with(command, target_path, shell_quote)
}

/// [`build_command`], quoting with `quote` (the platform's shell's rules).
pub fn build_command_with(
    command: &str,
    target_path: &str,
    quote: impl Fn(&str) -> String,
) -> String {
    let quoted = quote(target_path);
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
