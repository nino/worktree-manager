import { spawn, type ChildProcess } from "node:child_process";
import { BrowserWindow } from "electron";
import type { CommandExitEvent, CommandOutputEvent, RunningCommand } from "@shared/types";
import * as store from "./store";
import { runGit } from "./git";
import { LineRingBuffer, runKey } from "./commandKit";

// Re-exported so the run key helper is reachable from the registry module too.
export { runKey } from "./commandKit";

/**
 * Event channel names for streamed command output/exit. These are `.send`/`.on`
 * events (not `invoke`), so they must be kept in sync with the `CH` map in
 * `ipc.ts` and `preload/index.ts`.
 */
export const CommandEvents = { output: "command:output", exit: "command:exit" } as const;

/** Keep the last N lines of output per run, so scrollback stays bounded. */
const MAX_LINES = 5000;
/** Grace period between SIGTERM and SIGKILL when stopping a command. */
const KILL_GRACE_MS = 2000;

// MARK: Registry

interface Run {
  child: ChildProcess;
  buffer: LineRingBuffer;
  meta: RunningCommand;
  /** Pending SIGKILL escalation timer, if a stop is in progress. */
  killTimer: ReturnType<typeof setTimeout> | null;
  /** Guards against emitting the exit event twice (error + close). */
  finished: boolean;
}

/** Keyed by `runKey(worktreePath, commandId)`. */
const runs = new Map<string, Run>();

// MARK: Renderer notifications

function broadcast(channel: string, payload: unknown): void {
  for (const win of BrowserWindow.getAllWindows()) {
    if (!win.isDestroyed()) win.webContents.send(channel, payload);
  }
}

// MARK: Worktree guard

/** Assert `worktreePath` is one of the repo's worktrees (mirrors requireWorktree). */
async function requireWorktree(repoPath: string, worktreePath: string): Promise<void> {
  const out = await runGit(repoPath, ["worktree", "list", "--porcelain"]);
  const paths = out
    .split("\n")
    .filter((l) => l.startsWith("worktree "))
    .map((l) => l.slice("worktree ".length));
  if (!paths.includes(worktreePath)) {
    throw new Error(`Not a worktree of this repository: ${worktreePath}`);
  }
}

// MARK: Lifecycle

/**
 * Start a repo command inside a worktree. Validates the command exists and the
 * worktree belongs to the repo, then spawns via a shell in its own process group
 * so the whole tree (shell + e.g. node) can be signalled on stop. Streams merged
 * stdout/stderr to the renderer and buffers a bounded scrollback.
 */
export async function startCommand(
  repoId: string,
  worktreePath: string,
  commandId: string,
): Promise<RunningCommand> {
  const repo = store.getRepo(repoId);
  const command = repo.commands.find((c) => c.id === commandId);
  if (!command) throw new Error(`Unknown command: ${commandId}`);
  if (!command.command.trim()) throw new Error(`Command "${command.name}" is empty.`);

  await requireWorktree(repo.path, worktreePath);

  const key = runKey(worktreePath, commandId);
  if (runs.has(key)) {
    throw new Error(`"${command.name}" is already running on this worktree.`);
  }

  // Run through the user's login shell (like the init command in worktrees.ts)
  // so GUI/Finder launches still pick up nvm/pnpm on PATH — a plain /bin/sh
  // wouldn't. detached puts it in its own process group so stop() can signal the
  // whole tree (shell + e.g. node), not just the shell.
  const loginShell = process.env.SHELL || "/bin/zsh";
  const child = spawn(loginShell, ["-lc", command.command], {
    cwd: worktreePath,
    detached: true,
    env: process.env,
    windowsHide: true,
  });

  const buffer = new LineRingBuffer(MAX_LINES);
  const meta: RunningCommand = {
    repoId,
    worktreePath,
    commandId,
    name: command.name,
    command: command.command,
    startedAt: Date.now(),
  };
  const run: Run = { child, buffer, meta, killTimer: null, finished: false };
  runs.set(key, run);

  const emit = (chunk: string): void => {
    buffer.push(chunk);
    broadcast(CommandEvents.output, {
      worktreePath,
      commandId,
      chunk,
    } satisfies CommandOutputEvent);
  };

  child.stdout?.setEncoding("utf8");
  child.stderr?.setEncoding("utf8");
  child.stdout?.on("data", (c: string) => emit(c));
  child.stderr?.on("data", (c: string) => emit(c));

  const finalize = (exitCode: number | null, signal: NodeJS.Signals | null): void => {
    if (run.finished) return;
    run.finished = true;
    if (run.killTimer) clearTimeout(run.killTimer);
    runs.delete(key);
    broadcast(CommandEvents.exit, {
      worktreePath,
      commandId,
      exitCode,
      signal,
    } satisfies CommandExitEvent);
  };

  child.on("error", (err) => {
    emit(`\n[failed to start: ${err.message}]\n`);
    finalize(null, null);
  });
  // "close" (not "exit") so all buffered output is flushed before we report exit.
  child.on("close", (code, signal) => finalize(code, signal));

  return meta;
}

/** Send a signal to a run's whole process group, falling back to the child. */
function signalRun(run: Run, signal: NodeJS.Signals): void {
  const pid = run.child.pid;
  if (pid === undefined) return;
  try {
    // Negative pid targets the process group created by detached: true.
    process.kill(-pid, signal);
  } catch {
    try {
      run.child.kill(signal);
    } catch {
      // Already gone.
    }
  }
}

/** Stop a running command: SIGTERM, then SIGKILL after a short grace period. */
export function stopCommand(worktreePath: string, commandId: string): void {
  const run = runs.get(runKey(worktreePath, commandId));
  if (!run || run.killTimer) return;
  signalRun(run, "SIGTERM");
  run.killTimer = setTimeout(() => {
    if (!run.finished) signalRun(run, "SIGKILL");
  }, KILL_GRACE_MS);
}

/** Metadata for every command currently running, across all worktrees. */
export function listRunningCommands(): RunningCommand[] {
  return [...runs.values()].map((r) => r.meta);
}

/** Existing scrollback for a run (empty string once it has exited/cleared). */
export function getCommandBuffer(worktreePath: string, commandId: string): string {
  return runs.get(runKey(worktreePath, commandId))?.buffer.toString() ?? "";
}

/** Force-kill every running command. Call on app quit to avoid orphaned processes. */
export function stopAll(): void {
  for (const run of runs.values()) {
    if (run.killTimer) clearTimeout(run.killTimer);
    signalRun(run, "SIGKILL");
  }
  runs.clear();
}
