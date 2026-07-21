import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { useQueryClient } from "@tanstack/react-query";
import type { RunningCommand } from "@shared/types";
import { api } from "./api";
import { keys, useRunningCommands } from "./queries";

// MARK: Types

/** Identifies one running command instance in the UI. */
export interface RunSelection {
  worktreePath: string;
  commandId: string;
}

interface RunsContextValue {
  /** Every command currently running. */
  running: RunningCommand[];
  /** Running commands for a single worktree. */
  runningFor: (worktreePath: string) => RunningCommand[];
  /** Whether a specific command is running on a specific worktree. */
  isRunning: (worktreePath: string, commandId: string) => boolean;
  /** Bottom terminal drawer visibility. */
  drawerOpen: boolean;
  openDrawer: () => void;
  closeDrawer: () => void;
  /** The run whose output the drawer is showing. */
  selected: RunSelection | null;
  /** Show a run in the drawer (opens the drawer). */
  view: (selection: RunSelection) => void;
  /** Start a configured command on a worktree; opens the drawer on that run. */
  start: (repoId: string, worktreePath: string, commandId: string) => Promise<void>;
  /** Stop a running command. */
  stop: (worktreePath: string, commandId: string) => Promise<void>;
}

const RunsContext = createContext<RunsContextValue | null>(null);

function sameRun(a: RunSelection | null, b: RunSelection): boolean {
  return a !== null && a.worktreePath === b.worktreePath && a.commandId === b.commandId;
}

// MARK: Provider

/**
 * Owns the renderer's view of running commands: the running list (a TanStack
 * query kept fresh by start/stop/exit events), the terminal drawer's open state,
 * and which run the drawer is showing.
 */
export function RunsProvider({ children }: { children: ReactNode }) {
  const qc = useQueryClient();
  const query = useRunningCommands();
  const running = useMemo(() => query.data ?? [], [query.data]);

  const [drawerOpen, setDrawerOpen] = useState(false);
  const [selected, setSelected] = useState<RunSelection | null>(null);

  const invalidate = useCallback(() => {
    void qc.invalidateQueries({ queryKey: keys.runningCommands });
  }, [qc]);

  // A process exiting removes it from the running list — refresh badges/pickers.
  useEffect(() => api.onCommandExit(() => invalidate()), [invalidate]);

  const start = useCallback(
    async (repoId: string, worktreePath: string, commandId: string) => {
      await api.startCommand(repoId, worktreePath, commandId);
      setSelected({ worktreePath, commandId });
      setDrawerOpen(true);
      invalidate();
    },
    [invalidate],
  );

  const stop = useCallback(
    async (worktreePath: string, commandId: string) => {
      await api.stopCommand(worktreePath, commandId);
      // The exit event will refresh too; invalidate now for a snappier UI.
      invalidate();
    },
    [invalidate],
  );

  const view = useCallback((selection: RunSelection) => {
    setSelected(selection);
    setDrawerOpen(true);
  }, []);

  const value = useMemo<RunsContextValue>(
    () => ({
      running,
      runningFor: (worktreePath) => running.filter((r) => r.worktreePath === worktreePath),
      isRunning: (worktreePath, commandId) =>
        running.some((r) => r.worktreePath === worktreePath && r.commandId === commandId),
      drawerOpen,
      openDrawer: () => setDrawerOpen(true),
      closeDrawer: () => setDrawerOpen(false),
      selected,
      view,
      start,
      stop,
    }),
    [running, drawerOpen, selected, view, start, stop],
  );

  return <RunsContext.Provider value={value}>{children}</RunsContext.Provider>;
}

export function useRuns() {
  const ctx = useContext(RunsContext);
  if (!ctx) throw new Error("useRuns must be used within a RunsProvider");
  return ctx;
}

export { sameRun };
