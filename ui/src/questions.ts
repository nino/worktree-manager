// The delete ladder's questions, worded as `wtm-ui`'s `dialogs` words them.

import type { DeleteRequest } from "./wire/DeleteRequest";
import type { Repo } from "./wire/Repo";

export interface Question {
  title: string;
  message: string;
  /// The red button's caption.
  confirm: string;
  /// Shown with a warning sign: the second, destructive rung.
  critical: boolean;
  request: DeleteRequest;
}

const shown = (branch: string | null) => branch ?? "(detached)";

/// The first rung, for a worktree as its row shows it; `null` when it is
/// not listed any more.
export function confirmDelete(repo: Repo, path: string): Question | null {
  const w = repo.worktrees.find((w) => w.path === path);
  if (!w || w.isMain) return null;
  let message = "";
  if (w.prunable) message += "The folder is already gone — this cleans up git's bookkeeping. ";
  if (w.status && (w.status.hasStaged || w.status.hasUnstaged || w.status.hasUntracked)) {
    message += "It has uncommitted changes. ";
  } else if (!w.status && !w.prunable) {
    message += "Its status could not be determined. ";
  }
  message += w.displayPath;
  return {
    title: `Delete worktree “${shown(w.branch)}”?`,
    message,
    confirm: "Delete",
    critical: false,
    request: { repoId: repo.id, path, expectedBranch: w.branch, force: false },
  };
}

/// The second rung, after git refused a tree with uncommitted changes.
export function forceDelete(request: DeleteRequest): Question {
  return {
    title: `“${shown(request.expectedBranch)}” has uncommitted changes`,
    message: "They will be permanently lost. Delete anyway?",
    confirm: "Force Delete — Discard Changes",
    critical: true,
    request: { ...request, force: true },
  };
}
