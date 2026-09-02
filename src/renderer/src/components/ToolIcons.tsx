import type { BranchTool } from "../branchTool";

/**
 * Clawd, the Claude Code mascot: an orange block with two eyes and stubby
 * legs. Drawn on a 16-unit grid so it sits at text size beside a branch name.
 */
function ClaudeIcon() {
  return (
    <svg viewBox="0 0 16 16" width="1em" height="1em" aria-hidden="true" focusable="false">
      <path
        d="M2 4.5A2.5 2.5 0 0 1 4.5 2h7A2.5 2.5 0 0 1 14 4.5v5a2.5 2.5 0 0 1-2.5 2.5h-7A2.5 2.5 0 0 1 2 9.5z"
        fill="var(--claude)"
      />
      <rect x="4.5" y="5" width="2.2" height="3.6" rx="0.6" fill="var(--claude-ink)" />
      <rect x="9.3" y="5" width="2.2" height="3.6" rx="0.6" fill="var(--claude-ink)" />
      <rect x="4" y="12.5" width="2.4" height="2" rx="0.5" fill="var(--claude)" />
      <rect x="9.6" y="12.5" width="2.4" height="2" rx="0.5" fill="var(--claude)" />
    </svg>
  );
}

/** Cursor's isometric-cube mark, in the current text colour. */
function CursorIcon() {
  return (
    <svg viewBox="0 0 16 16" width="1em" height="1em" aria-hidden="true" focusable="false">
      {/* Top face */}
      <path d="M8 1.5 14 5 8 8.5 2 5z" fill="currentColor" opacity="0.9" />
      {/* Left face */}
      <path d="M2 5v6.5L8 15V8.5z" fill="currentColor" opacity="0.55" />
      {/* Right face */}
      <path d="M14 5v6.5L8 15V8.5z" fill="currentColor" opacity="0.35" />
      {/* Hidden-edge fold that makes the mark read as Cursor's, not a plain cube */}
      <path
        d="M2 5 14 11.5M8 8.5V15"
        fill="none"
        stroke="currentColor"
        strokeWidth="0.9"
        opacity="0.6"
      />
    </svg>
  );
}

interface ToolIconProps {
  tool: BranchTool;
}

/** The mark for the agent that owns a `<tool>/` branch. */
export function ToolIcon({ tool }: ToolIconProps) {
  return (
    <span className={`tool-icon tool-icon-${tool}`}>
      {tool === "claude" ? <ClaudeIcon /> : <CursorIcon />}
    </span>
  );
}
