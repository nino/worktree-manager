// The status badges on a worktree row. Ported from `wtm-ui`'s `badges_for`.

import type { Worktree } from "./wire/Worktree";

export type Hue =
  | "accent"
  | "gray"
  | "red"
  | "green"
  | "blue"
  | "orange"
  | "purple"
  | "teal"
  | "yellow";

/// How loudly a badge may speak where colour is rationed (the dark
/// appearance): only uncommitted work and a missing folder spend colour.
export type Emphasis = "quiet" | "notable" | "attention" | "alarm";

export interface Badge {
  text: string;
  hue: Hue;
  emphasis: Emphasis;
  tooltip: string;
}

const badge = (text: string, hue: Hue, emphasis: Emphasis, tooltip: string): Badge => ({
  text,
  hue,
  emphasis,
  tooltip,
});

/// The badge row for a worktree, in display order.
export function badgesFor(w: Worktree, mainBranch: string): Badge[] {
  const out: Badge[] = [];
  if (w.isMain) {
    out.push(badge("primary", "accent", "quiet", "The repository's primary working tree"));
  }
  if (w.locked) {
    out.push(badge("locked", "gray", "quiet", "This worktree is locked"));
  }
  if (w.prunable) {
    out.push(
      badge(
        "folder missing",
        "red",
        "alarm",
        "Folder was deleted outside the app — git still tracks this worktree. Delete the row to clean up git's bookkeeping.",
      ),
    );
    return out;
  }
  const s = w.status;
  if (!s) {
    out.push(badge("no status", "gray", "quiet", "Status could not be determined"));
    return out;
  }
  const trunk = s.trunkRef || mainBranch;
  if (!s.hasStaged && !s.hasUnstaged && !s.hasUntracked) {
    out.push(
      badge("✓", "green", "quiet", "Clean working tree — no staged, unstaged, or untracked changes"),
    );
  }
  if (s.hasStaged) out.push(badge("staged", "blue", "attention", "Staged, uncommitted changes"));
  if (s.hasUnstaged) out.push(badge("unstaged", "orange", "attention", "Unstaged changes"));
  if (s.hasUntracked) out.push(badge("untracked", "purple", "attention", "Untracked files"));
  if (s.ahead !== null && s.behind !== null) {
    if (s.ahead > 0) {
      out.push(badge(`↑${s.ahead} ${trunk}`, "teal", "quiet", `${s.ahead} commit(s) ahead of ${trunk}`));
    }
    if (s.behind > 0) {
      out.push(badge(`↓${s.behind} ${trunk}`, "red", "quiet", `${s.behind} commit(s) behind ${trunk}`));
    }
  } else {
    out.push(
      badge(
        `? ${trunk}`,
        "gray",
        "quiet",
        `Couldn't compare with ${trunk} — check the repo's main-branch setting`,
      ),
    );
  }
  if (s.unpushed > 0) {
    out.push(badge(`⇡${s.unpushed} unpushed`, "yellow", "notable", `${s.unpushed} unpushed commit(s)`));
  } else if (!s.hasUpstream) {
    out.push(badge("no upstream", "gray", "quiet", "No upstream branch configured"));
  }
  return out;
}
