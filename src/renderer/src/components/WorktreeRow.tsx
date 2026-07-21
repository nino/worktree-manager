import { useEffect, useState } from "react";
import {
  ArrowDownToLine,
  ArrowUpFromLine,
  Code,
  FolderOpen,
  GitMerge,
  Loader2,
  SquareTerminal,
  Trash2,
} from "lucide-react";
import type { RepoConfig, WorktreeInfo } from "@shared/types";
import { useBranches, useDeleteWorktree, useGitOp } from "../queries";
import { api } from "../api";
import { useRuns } from "../runs";
import { displayPath } from "../format";
import { BranchPicker } from "./BranchPicker";
import { StatusBadges } from "./StatusBadges";
import { WorktreeCommands } from "./WorktreeCommands";

interface Props {
  repo: RepoConfig;
  worktree: WorktreeInfo;
}

const ICON = { size: 13, strokeWidth: 1.75 } as const;

/** A single worktree row: branch selector, path, status, and actions. */
export function WorktreeRow({ repo, worktree }: Props) {
  const [confirmStage, setConfirmStage] = useState<"confirm" | "force" | null>(null);
  const [opError, setOpError] = useState<string | null>(null);
  const del = useDeleteWorktree();
  const gitOp = useGitOp();
  const branches = useBranches(repo.id);
  const runs = useRuns();
  const runningHere = runs.runningFor(worktree.path);

  // If the worktree changes under an open confirm (branch switched, new
  // commit), the confirmation no longer covers what the user saw — reset it.
  useEffect(() => {
    setConfirmStage(null);
  }, [worktree.branch, worktree.head]);

  const remove = async (force: boolean) => {
    setOpError(null);
    try {
      const result = await del.mutateAsync({
        repoId: repo.id,
        worktreePath: worktree.path,
        expectedBranch: worktree.branch,
        force,
      });
      if (result.ok) {
        setConfirmStage(null);
      } else if (result.reason === "dirty" && !force) {
        setConfirmStage("force");
      } else {
        setOpError(result.message);
        setConfirmStage(null);
      }
    } catch (err) {
      setOpError((err as Error).message);
      setConfirmStage(null);
    }
  };

  const runOp = async (op: "push" | "pull" | "pullMain" | "switch", branch?: string) => {
    setOpError(null);
    try {
      const result = await gitOp.mutateAsync({
        op,
        repoId: repo.id,
        worktreePath: worktree.path,
        branch,
      });
      if (!result.ok) setOpError(result.message);
    } catch (err) {
      setOpError((err as Error).message);
    }
  };

  const busy = gitOp.isPending || del.isPending;
  /** Folder was deleted outside the app; only delete (cleanup) makes sense. */
  const missing = worktree.prunable;
  const dirty =
    worktree.status !== null &&
    (worktree.status.hasStaged || worktree.status.hasUnstaged || worktree.status.hasUntracked);

  return (
    <div className={`wt-row${worktree.isMain ? " wt-main" : ""}`}>
      <div className="wt-info">
        <div className="wt-line1">
          {worktree.branch !== null && branches.data ? (
            <BranchPicker
              branches={branches.data}
              current={worktree.branch}
              disabled={busy || missing}
              onSelect={(branch) => runOp("switch", branch)}
            />
          ) : (
            <span className="wt-branch">{worktree.branch ?? "(detached)"}</span>
          )}
          {worktree.isMain && <span className="badge badge-main">primary</span>}
          {worktree.locked && <span className="badge badge-muted">locked</span>}
          {missing ? (
            <span
              className="badge badge-missing"
              title="Folder was deleted outside the app — git still tracks this worktree. Delete the row to clean up git's bookkeeping."
            >
              folder missing
            </span>
          ) : (
            <StatusBadges status={worktree.status} mainBranch={repo.mainBranch} />
          )}
          {runningHere.length > 0 && (
            <button
              className="badge badge-running"
              title={`Running: ${runningHere.map((r) => r.name).join(", ")} — click to view output`}
              onClick={() =>
                runs.view({ worktreePath: worktree.path, commandId: runningHere[0].commandId })
              }
            >
              <span className="run-dot" aria-hidden="true" />
              {runningHere.length === 1 ? runningHere[0].name : `${runningHere.length} running`}
            </button>
          )}
        </div>
        <div className="wt-path" title={worktree.path}>
          {displayPath(worktree.path)}
        </div>
      </div>

      <div className="wt-actions">
        <WorktreeCommands repo={repo} worktree={worktree} disabled={missing} />
        <span className="btn-group">
          <button
            className="btn btn-sm btn-icon"
            title="Push"
            aria-label="Push"
            disabled={busy || missing}
            onClick={() => runOp("push")}
          >
            <ArrowUpFromLine {...ICON} />
          </button>
          <button
            className="btn btn-sm btn-icon"
            title="Pull (fast-forward only)"
            aria-label="Pull"
            disabled={busy || missing}
            onClick={() => runOp("pull")}
          >
            <ArrowDownToLine {...ICON} />
          </button>
          <button
            className="btn btn-sm btn-icon"
            title={`Pull ${repo.mainBranch} into this branch`}
            aria-label={`Pull ${repo.mainBranch} into this branch`}
            disabled={busy || missing}
            onClick={() => runOp("pullMain")}
          >
            <GitMerge {...ICON} />
          </button>
        </span>
        <span className="btn-group">
          <button
            className="btn btn-sm btn-icon"
            title="Open in editor"
            aria-label="Open in editor"
            disabled={missing}
            onClick={() => api.openInEditor(worktree.path)}
          >
            <Code {...ICON} />
          </button>
          <button
            className="btn btn-sm btn-icon"
            title="Open in terminal"
            aria-label="Open in terminal"
            disabled={missing}
            onClick={() => api.openInTerminal(worktree.path)}
          >
            <SquareTerminal {...ICON} />
          </button>
          <button
            className="btn btn-sm btn-icon"
            title="Reveal in Finder"
            aria-label="Reveal in Finder"
            disabled={missing}
            onClick={() => api.revealInFinder(worktree.path)}
          >
            <FolderOpen {...ICON} />
          </button>
        </span>
        {!worktree.isMain && confirmStage === null && (
          <button
            className="btn btn-sm btn-icon btn-danger-ghost"
            title="Delete worktree"
            aria-label="Delete worktree"
            disabled={busy}
            onClick={() => setConfirmStage("confirm")}
          >
            <Trash2 {...ICON} />
          </button>
        )}
      </div>

      {confirmStage === "confirm" && (
        <div className="confirm row-error">
          <p>
            Delete worktree <strong>{worktree.branch ?? "(detached)"}</strong>
            {missing ? " (folder already gone — this cleans up git's bookkeeping)" : ""}?
            {dirty && " It has uncommitted changes."}
            {worktree.status === null && !missing && " Its status could not be determined."}
          </p>
          <div className="row">
            <button className="btn btn-sm" disabled={busy} onClick={() => setConfirmStage(null)}>
              Cancel
            </button>
            <button className="btn btn-sm btn-danger" disabled={busy} onClick={() => remove(false)}>
              {del.isPending ? "Deleting…" : "Delete"}
            </button>
          </div>
        </div>
      )}

      {confirmStage === "force" && (
        <div className="confirm row-error">
          <p>
            <strong>{worktree.branch ?? "(detached)"}</strong> has uncommitted changes that will be
            permanently lost. Delete anyway?
          </p>
          <div className="row">
            <button className="btn btn-sm" disabled={busy} onClick={() => setConfirmStage(null)}>
              Cancel
            </button>
            <button className="btn btn-sm btn-danger" disabled={busy} onClick={() => remove(true)}>
              {del.isPending ? "Deleting…" : "Force delete — discard changes"}
            </button>
          </div>
        </div>
      )}

      {gitOp.isPending && <p className="hint row-error">Running…</p>}
      {opError && <p className="error row-error">{opError}</p>}

      {/* While git removes the tree it transiently reports the vanishing files
          as changes; cover the row so that flicker never reaches the UI. */}
      {del.isPending && (
        <div className="wt-overlay">
          <Loader2 {...ICON} className="spin" />
          Deleting…
        </div>
      )}
    </div>
  );
}
