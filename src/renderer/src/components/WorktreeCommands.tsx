import { useState } from "react";
import { Square } from "lucide-react";
import type { RepoConfig, WorktreeInfo } from "@shared/types";
import { useRuns } from "../runs";

interface Props {
  repo: RepoConfig;
  worktree: WorktreeInfo;
  /** Folder deleted outside the app — running commands makes no sense. */
  disabled?: boolean;
}

const ICON = { size: 13, strokeWidth: 1.75 } as const;

/**
 * Per-worktree command control: a native popup to start any configured command
 * (or re-view a running one) plus an inline Stop button per running command.
 * A native <select> is used for the launcher so its popup is never clipped by
 * the repo panel's `overflow: hidden`.
 */
export function WorktreeCommands({ repo, worktree, disabled }: Props) {
  const runs = useRuns();
  const [error, setError] = useState<string | null>(null);
  const runningHere = runs.runningFor(worktree.path);
  const hasCommands = repo.commands.length > 0;

  const onPick = async (commandId: string) => {
    if (!commandId) return;
    setError(null);
    // Already running → just focus the drawer on it; otherwise start it.
    if (runs.isRunning(worktree.path, commandId)) {
      runs.view({ worktreePath: worktree.path, commandId });
      return;
    }
    try {
      await runs.start(repo.id, worktree.path, commandId);
    } catch (err) {
      setError((err as Error).message);
    }
  };

  const onStop = async (commandId: string) => {
    setError(null);
    try {
      await runs.stop(worktree.path, commandId);
    } catch (err) {
      setError((err as Error).message);
    }
  };

  return (
    <span className="cmd-runner">
      {runningHere.map((r) => (
        <button
          key={r.commandId}
          className="btn btn-sm btn-icon btn-danger-ghost"
          title={`Stop ${r.name}`}
          aria-label={`Stop ${r.name}`}
          onClick={() => onStop(r.commandId)}
        >
          <Square {...ICON} />
        </button>
      ))}
      <select
        className="branch-select cmd-select"
        // Controlled to "" so it always snaps back to the placeholder label.
        value=""
        title={hasCommands ? "Run a command" : "No commands configured — add one in repo settings"}
        aria-label="Run a command"
        disabled={disabled || !hasCommands}
        onChange={(e) => void onPick(e.target.value)}
      >
        <option value="" disabled hidden>
          {hasCommands ? "Run…" : "No commands"}
        </option>
        {repo.commands.map((c) => (
          <option key={c.id} value={c.id}>
            {runs.isRunning(worktree.path, c.id) ? `View ${c.name}` : `Run ${c.name}`}
          </option>
        ))}
      </select>
      {error && <span className="error row-error">{error}</span>}
    </span>
  );
}
