/** Coding agents whose branches carry a recognisable `<tool>/` prefix. */
export type BranchTool = "claude" | "cursor";

const TOOL_PREFIX = /^(claude|cursor)\/(.+)$/;

/**
 * Split a `claude/…` or `cursor/…` branch name into the tool that owns it
 * and the remainder, so the UI can swap the prefix for the tool's icon.
 * Returns `null` for every other name (including a bare `claude` with no
 * slash and an empty remainder) — those render as plain text.
 */
export function splitToolPrefix(branch: string): { tool: BranchTool; rest: string } | null {
  const m = TOOL_PREFIX.exec(branch);
  if (!m) return null;
  return { tool: m[1] as BranchTool, rest: m[2] };
}
