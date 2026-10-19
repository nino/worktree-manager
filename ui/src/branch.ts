// A coding agent's branch prefix, drawn as its mark so the part of the name
// that says what the work is stays visible. Mirrors `wtm_core::branch_tool`.

export type Agent = "claude" | "cursor";

export function splitAgentPrefix(branch: string): [Agent, string] | null {
  for (const agent of ["claude", "cursor"] as const) {
    const prefix = `${agent}/`;
    if (branch.startsWith(prefix) && branch.length > prefix.length) {
      return [agent, branch.slice(prefix.length)];
    }
  }
  return null;
}
