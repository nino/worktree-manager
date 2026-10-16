// What a search shows. Ported from `wtm-ui`'s list module.

import type { Repo } from "./wire/Repo";
import type { Worktree } from "./wire/Worktree";
import type { Pending } from "./wire/Pending";

/// The query as matched: trimmed and lower-cased.
export const normalise = (query: string) => query.trim().toLowerCase();

/// Case-insensitive substring match on the branch or the path as the row
/// shows it. Not the full path: every worktree's starts with the home
/// folder, so a query like `users` would match them all.
export function matches(query: string, w: Worktree): boolean {
  return (
    (w.branch?.toLowerCase().includes(query) ?? false) ||
    w.displayPath.toLowerCase().includes(query)
  );
}

export type Child = { kind: "worktree"; worktree: Worktree } | { kind: "pending"; pending: Pending };

const folderName = (path: string) => path.slice(path.lastIndexOf("/") + 1);

/// One card's rows in the order git will list them: the primary worktree
/// first, then by folder name; a creation sits where its folder will.
export function children(repo: Repo, query: string): Child[] {
  const rows: [boolean, string, Child][] = [];
  for (const w of repo.worktrees) {
    if (query && !matches(query, w)) continue;
    rows.push([w.isMain, folderName(w.path), { kind: "worktree", worktree: w }]);
  }
  for (const p of repo.pending) {
    rows.push([false, p.folder, { kind: "pending", pending: p }]);
  }
  rows.sort((a, b) => Number(b[0]) - Number(a[0]) || (a[1] < b[1] ? -1 : a[1] > b[1] ? 1 : 0));
  return rows.map((r) => r[2]);
}

/// Whether a search shows `repo`: one with a matching row, or one whose
/// listing failed, so typing never hides the error.
export const showsRepo = (repo: Repo, query: string) =>
  !query || repo.error !== null || children(repo, query).length > 0;

/// "main · 3 worktrees", or "main · 1 of 3 worktrees" while searching.
export function repoMeta(repo: Repo, query: string): string {
  const total = repo.worktrees.length;
  const noun = total === 1 ? "worktree" : "worktrees";
  if (!query) return `${repo.mainBranch} · ${total} ${noun}`;
  const visible = children(repo, query).filter((c) => c.kind === "worktree").length;
  return `${repo.mainBranch} · ${visible} of ${total} ${noun}`;
}
