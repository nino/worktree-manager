//! Branches created by a coding agent carry a recognisable `<tool>/` prefix.
//! The UI swaps that prefix for the agent's mark, so the part of the name that
//! identifies the work is not pushed off the end of a narrow row.

/// Coding agents whose branches carry a recognisable `<tool>/` prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchTool {
    Claude,
    Cursor,
}

impl BranchTool {
    pub fn prefix(self) -> &'static str {
        match self {
            BranchTool::Claude => "claude/",
            BranchTool::Cursor => "cursor/",
        }
    }
}

/// Split a `claude/…` or `cursor/…` branch into the tool that owns it and the
/// remainder. Every other name (a bare `claude`, or a prefix with nothing
/// after it) is `None` and renders as plain text.
pub fn split_tool_prefix(branch: &str) -> Option<(BranchTool, &str)> {
    for tool in [BranchTool::Claude, BranchTool::Cursor] {
        if let Some(rest) = branch.strip_prefix(tool.prefix()) {
            if !rest.is_empty() {
                return Some((tool, rest));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_agent_prefixes_only() {
        assert_eq!(
            split_tool_prefix("claude/rust-rewrite"),
            Some((BranchTool::Claude, "rust-rewrite"))
        );
        assert_eq!(
            split_tool_prefix("cursor/a/b"),
            Some((BranchTool::Cursor, "a/b"))
        );
        assert_eq!(split_tool_prefix("claude/"), None);
        assert_eq!(split_tool_prefix("claude"), None);
        assert_eq!(split_tool_prefix("Claude/x"), None);
        assert_eq!(split_tool_prefix("feature/claude/x"), None);
        assert_eq!(split_tool_prefix("main"), None);
    }
}
