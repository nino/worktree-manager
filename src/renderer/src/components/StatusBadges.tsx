import type { ReactElement } from "react";
import type { WorktreeStatus } from "@shared/types";

interface StatusBadgesProps {
  status: WorktreeStatus | null;
  mainBranch: string;
}

/** Compact row of git-status indicators for a worktree. */
export function StatusBadges({ status, mainBranch }: StatusBadgesProps) {
  if (!status) return <span className="badge badge-muted">no status</span>;

  // The ref the counts were measured against — usually `origin/<main>`. The
  // repo's configured branch name is only a fallback for older/partial status.
  const trunkRef = status.trunkRef || mainBranch;
  const dirty = status.hasStaged || status.hasUnstaged || status.hasUntracked;
  const badges: ReactElement[] = [];

  if (status.hasStaged)
    badges.push(
      <span key="staged" className="badge badge-staged" title="Staged, uncommitted changes">
        staged
      </span>,
    );
  if (status.hasUnstaged)
    badges.push(
      <span key="unstaged" className="badge badge-unstaged" title="Unstaged changes">
        unstaged
      </span>,
    );
  if (status.hasUntracked)
    badges.push(
      <span key="untracked" className="badge badge-untracked" title="Untracked files">
        untracked
      </span>,
    );
  if (status.aheadOfMain === null || status.behindMain === null)
    badges.push(
      <span
        key="vs-main-unknown"
        className="badge badge-muted"
        title={`Couldn't compare with ${trunkRef} — check the repo's main-branch setting`}
      >
        ? {trunkRef}
      </span>,
    );
  if (status.aheadOfMain !== null && status.aheadOfMain > 0)
    badges.push(
      <span
        key="ahead"
        className="badge badge-ahead"
        title={`${status.aheadOfMain} commit(s) ahead of ${trunkRef}`}
      >
        ↑{status.aheadOfMain} {trunkRef}
      </span>,
    );
  if (status.behindMain !== null && status.behindMain > 0)
    badges.push(
      <span
        key="behind"
        className="badge badge-behind"
        title={`${status.behindMain} commit(s) behind ${trunkRef}`}
      >
        ↓{status.behindMain} {trunkRef}
      </span>,
    );
  if (status.unpushed)
    badges.push(
      <span
        key="unpushed"
        className="badge badge-unpushed"
        title={`${status.unpushedCount} unpushed commit(s)`}
      >
        ⇡{status.unpushedCount} unpushed
      </span>,
    );
  else if (!status.hasUpstream)
    badges.push(
      <span key="noupstream" className="badge badge-muted" title="No upstream branch configured">
        no upstream
      </span>,
    );

  if (!dirty)
    badges.unshift(
      <span
        key="clean"
        className="badge badge-clean"
        title="Clean working tree — no staged, unstaged, or untracked changes"
      >
        ✓
      </span>,
    );

  return <span className="badges">{badges}</span>;
}
