// Keyboard navigation through the list: the rows a key can land on, in the
// order they are drawn, and where each arrow key goes from a row. A row is
// named by what it shows (a repo id, a worktree path), never by position,
// so a refresh that reorders rows never moves the selection elsewhere.

import { children, showsRepo } from "./search";
import type { Repo } from "./wire/Repo";

export type RowKey = `repo:${string}` | `wt:${string}`;

export interface Row {
  key: RowKey;
  repoId: string;
  /// The worktree's path; absent for a card's header.
  path?: string;
}

export const repoKey = (id: string): RowKey => `repo:${id}`;
export const worktreeKey = (path: string): RowKey => `wt:${path}`;

/// Every row a key can land on: each card's header, then its worktrees
/// while it is open. Creations in progress are not rows.
export function navRows(repos: readonly Repo[], query: string, isOpen: (id: string) => boolean): Row[] {
  const rows: Row[] = [];
  for (const repo of repos) {
    if (!showsRepo(repo, query)) continue;
    rows.push({ key: repoKey(repo.id), repoId: repo.id });
    if (!isOpen(repo.id)) continue;
    for (const c of children(repo, query)) {
      if (c.kind === "worktree") {
        rows.push({ key: worktreeKey(c.worktree.path), repoId: repo.id, path: c.worktree.path });
      }
    }
  }
  return rows;
}

export type NavKey = "up" | "down" | "left" | "right" | "home" | "end";

/// What an arrow key does from `selected`: select another row, or open or
/// close the selected card (as an outline view does).
export type NavResult = { select: RowKey } | { open: string } | { close: string } | null;

export function navigate(
  rows: readonly Row[],
  selected: RowKey | null,
  key: NavKey,
  isOpen: (id: string) => boolean,
): NavResult {
  if (!rows.length) return null;
  const at = selected === null ? -1 : rows.findIndex((r) => r.key === selected);
  const select = (i: number) => ({ select: rows[Math.min(Math.max(i, 0), rows.length - 1)].key });
  switch (key) {
    case "home":
      return select(0);
    case "end":
      return select(rows.length - 1);
    case "down":
      return select(at + 1);
    case "up":
      return at < 0 ? select(rows.length - 1) : select(at - 1);
  }
  if (at < 0) return select(0);
  const row = rows[at];
  if (key === "left") {
    if (row.path !== undefined) return { select: repoKey(row.repoId) };
    return isOpen(row.repoId) ? { close: row.repoId } : null;
  }
  if (row.path !== undefined) return null;
  if (!isOpen(row.repoId)) return { open: row.repoId };
  const next = rows[at + 1];
  return next && next.repoId === row.repoId ? { select: next.key } : null;
}
