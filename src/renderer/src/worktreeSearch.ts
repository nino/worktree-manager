// MARK: Worktree search filtering

import type { RepoWithWorktrees, WorktreeInfo } from "@shared/types";

/** What `WorktreeRow` renders in place of a null (detached HEAD) branch. */
const DETACHED_LABEL = "(detached)";

/**
 * Case-insensitive substring test against a worktree's branch and path,
 * either of which matching is enough. A blank query matches everything. A
 * null branch (detached HEAD) is matched against the literal "detached" —
 * what the row actually displays for it. `path` is optional so the same
 * predicate covers in-flight creations, which have a branch but no path yet.
 */
export function matchesWorktreeSearch(
  query: string,
  branch: string | null,
  path?: string | null,
): boolean {
  const q = query.trim().toLowerCase();
  if (q.length === 0) return true;
  const branchText = (branch ?? DETACHED_LABEL).toLowerCase();
  if (branchText.includes(q)) return true;
  return path != null && path.toLowerCase().includes(q);
}

/** Filter `worktrees` to those matching `query`, preserving order. */
export function filterWorktrees(query: string, worktrees: WorktreeInfo[]): WorktreeInfo[] {
  if (query.trim().length === 0) return worktrees;
  return worktrees.filter((w) => matchesWorktreeSearch(query, w.branch, w.path));
}

/** True if any worktree in `node` matches `query`. */
export function repoHasMatch(query: string, node: RepoWithWorktrees): boolean {
  return node.worktrees.some((w) => matchesWorktreeSearch(query, w.branch, w.path));
}
