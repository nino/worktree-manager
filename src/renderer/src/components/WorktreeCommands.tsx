import { useState } from "react";
import { Square } from "lucide-react";
import { INIT_COMMAND_ID, type RepoConfig, type WorktreeInfo } from "@shared/types";
import { useRuns } from "../runs";

const ICON = { size: 13, strokeWidth: 1.75 } as const;

interface WorktreeCommandsProps {
  repo: RepoConfig;
  worktree: WorktreeInfo;
  /** Folder deleted outside the app — running commands makes no sense. */
  disabled?: boolean;
}

/**
 * Per-worktree command control: a native popup to start any configured command
 * (or re-view a running one) plus an inline Stop button per running command.
 * A native <select> is used for the launcher so its popup is never clipped by
 * the repo panel's `overflow: hidden`.
 *
 * A repo with no configured commands renders nothing at all — a permanently
 * disabled "No commands" plate is dead weight in every row of every such repo.
 * Runs already in flight (a command deleted from settings while it ran) keep
 * their Stop button, so nothing can be left running with no way to stop it.
 */
export function WorktreeCommands({ repo, worktree, disabled }: WorktreeCommandsProps) {
  const runs = useRuns();
  const [error, setError] = useState<string | null>(null);
  // The init run has its own "Initialising" badge (viewable/stoppable from the
  // terminal drawer); it isn't a configured command, so keep it out of here.
  const runningHere = runs.runningFor(worktree.path).filter((r) => r.commandId !== INIT_COMMAND_ID);
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

  // Nothing to offer and nothing running: stay out of the row entirely.
  if (!hasCommands && runningHere.length === 0 && error === null) return null;

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
      {hasCommands && (
        <select
          className="branch-select cmd-select"
          // Controlled to "" so it always snaps back to the placeholder label.
          value=""
          title="Run a command"
          aria-label="Run a command"
          disabled={disabled}
          onChange={(e) => void onPick(e.target.value)}
        >
          <option value="" disabled hidden>
            Run…
          </option>
          {repo.commands.map((c) => (
            <option key={c.id} value={c.id}>
              {runs.isRunning(worktree.path, c.id) ? `View ${c.name}` : `Run ${c.name}`}
            </option>
          ))}
        </select>
      )}
      {error && <span className="error row-error">{error}</span>}
    </span>
  );
}
