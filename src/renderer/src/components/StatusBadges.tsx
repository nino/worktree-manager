import type { ReactElement } from "react";
import type { WorktreeStatus } from "@shared/types";

interface Props {
  status: WorktreeStatus | null;
  mainBranch: string;
}

/** Compact row of git-status indicators for a worktree. */
export function StatusBadges({ status, mainBranch }: Props) {
  if (!status) return <span className="badge badge-muted">no status</span>;

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
        title={`Couldn't compare with ${mainBranch} — check the repo's main-branch setting`}
      >
        ? {mainBranch}
      </span>,
    );
  if (status.aheadOfMain !== null && status.aheadOfMain > 0)
    badges.push(
      <span
        key="ahead"
        className="badge badge-ahead"
        title={`${status.aheadOfMain} commit(s) ahead of ${mainBranch}`}
      >
        ↑{status.aheadOfMain} {mainBranch}
      </span>,
    );
  if (status.behindMain !== null && status.behindMain > 0)
    badges.push(
      <span
        key="behind"
        className="badge badge-behind"
        title={`${status.behindMain} commit(s) behind ${mainBranch}`}
      >
        ↓{status.behindMain} {mainBranch}
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
