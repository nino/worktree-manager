/** Types shared across the main and renderer processes. */

/**
 * A named command a repo can run inside a worktree (e.g. `pnpm dev`). Repos hold
 * an array of these so more than one command per repo is supported.
 */
export interface RepoCommand {
  /** Stable unique id (generated when the command is first added). */
  id: string;
  /** Display name shown in the UI (e.g. "Dev server"). */
  name: string;
  /** The shell command line to run (e.g. `pnpm dev`). */
  command: string;
}

/** Per-repository configuration, persisted in app preferences. */
export interface RepoConfig {
  /** Stable unique id. */
  id: string;
  /** Display name (defaults to the repo directory basename). */
  name: string;
  /** Absolute path to the repository root (the primary working tree). */
  path: string;
  /** Branch that worktrees are compared against for ahead/behind. */
  mainBranch: string;
  /** Command run inside a new worktree after it is created (e.g. `pnpm i`). */
  initCommand: string;
  /** Configurable commands runnable per worktree (e.g. `pnpm dev`). */
  commands: RepoCommand[];
}

/** Global, app-wide configuration, persisted in app preferences. */
export interface AppConfig {
  /** Root directory that all worktrees are created under. */
  worktreesRoot: string;
  /** Editor command used by "Open in editor" (e.g. `code`, or an absolute path). */
  editorCommand: string;
  /** Configured repositories. */
  repos: RepoConfig[];
}

/** The app-wide settings editable in the Settings dialog. */
export type AppSettings = Pick<AppConfig, "worktreesRoot" | "editorCommand">;

/** Git status of a single worktree. */
export interface WorktreeStatus {
  /** Working-tree changes that are not staged. */
  hasUnstaged: boolean;
  /** Changes staged but not yet committed. */
  hasStaged: boolean;
  /** Untracked files present. */
  hasUntracked: boolean;
  /** Commits ahead of the repo's configured main branch; null if unknown. */
  aheadOfMain: number | null;
  /** Commits behind the repo's configured main branch; null if unknown. */
  behindMain: number | null;
  /** True if the branch has an upstream and local commits are unpushed. */
  unpushed: boolean;
  /** Number of commits not pushed to upstream (0 if no upstream). */
  unpushedCount: number;
  /** True if the branch tracks an upstream remote. */
  hasUpstream: boolean;
}

/** A single worktree belonging to a repository. */
export interface WorktreeInfo {
  /** Absolute path to the worktree. */
  path: string;
  /** Checked-out branch name, or null if detached. */
  branch: string | null;
  /** Short HEAD sha. */
  head: string;
  /** True for the repository's primary working tree. */
  isMain: boolean;
  /** True if the worktree is locked. */
  locked: boolean;
  /** True if git reports the folder as missing (deleted outside the app). */
  prunable: boolean;
  /** Git status; null if it could not be computed. */
  status: WorktreeStatus | null;
}

/** A repository together with its current worktrees (view model). */
export interface RepoWithWorktrees {
  repo: RepoConfig;
  worktrees: WorktreeInfo[];
  /**
   * Preferred base ref when creating a new branch: `origin/<trunk>` when that
   * remote-tracking branch exists locally (so new branches start from the
   * latest fetched remote state), otherwise the local trunk branch name.
   */
  defaultBaseRef: string;
  /** Populated if listing worktrees failed. */
  error?: string;
}

/** Parameters for creating a new worktree. */
export interface CreateWorktreeParams {
  repoId: string;
  /** Branch name to create/check out in the new worktree. */
  branch: string;
  /** If true, create a new branch; otherwise check out an existing one. */
  newBranch: boolean;
  /** Optional base ref for a new branch (defaults to the repo's main branch). */
  baseRef?: string;
}

/** Result of running the init command in a new worktree. */
export interface InitCommandResult {
  ran: boolean;
  exitCode: number | null;
  stdout: string;
  stderr: string;
}

/** Result of creating a worktree. */
export interface CreateWorktreeResult {
  worktree: WorktreeInfo;
  init: InitCommandResult;
}

/**
 * Outcome of a git operation triggered from the UI (push/pull/switch…).
 * Never thrown across IPC — failures carry git's own message.
 */
export interface GitOpResult {
  ok: boolean;
  /** git's output (stdout on success, stderr on failure). */
  message: string;
}

/** Parameters for deleting a worktree. */
export interface DeleteWorktreeParams {
  repoId: string;
  worktreePath: string;
  /**
   * Branch the UI showed when the user confirmed. Deletion is refused if the
   * worktree has since switched branches (stale-row protection).
   */
  expectedBranch: string | null;
  /** Destroy uncommitted changes too (requires an explicit second confirmation). */
  force: boolean;
}

/** Outcome of a delete request. Never thrown across IPC. */
export interface DeleteWorktreeResult {
  ok: boolean;
  /**
   * Why the delete did not happen:
   * - "dirty": worktree has uncommitted changes; retry with force after re-confirming.
   * - "changed": the worktree no longer matches what the UI showed; re-check.
   * - "error": anything else (message has details).
   */
  reason?: "dirty" | "changed" | "error";
  message: string;
}

/**
 * A command process currently running for a specific worktree. Keyed uniquely by
 * `(worktreePath, commandId)` so the same command can run on several worktrees
 * at once. `name`/`command` are snapshots taken when the process was started.
 */
export interface RunningCommand {
  /** Owning repo id. */
  repoId: string;
  /** Worktree the process runs in. */
  worktreePath: string;
  /** Id of the RepoCommand that was started. */
  commandId: string;
  /** Command display name at spawn time. */
  name: string;
  /** Command line at spawn time. */
  command: string;
  /** Epoch milliseconds when the process started. */
  startedAt: number;
}

/** A chunk of merged stdout/stderr streamed from a running command. */
export interface CommandOutputEvent {
  worktreePath: string;
  commandId: string;
  /** Raw output text (may contain partial lines and ANSI escapes). */
  chunk: string;
}

/** Emitted once when a running command exits (or fails to start). */
export interface CommandExitEvent {
  worktreePath: string;
  commandId: string;
  /** Process exit code, or null if it was killed by a signal / failed to spawn. */
  exitCode: number | null;
  /** Signal that terminated the process, if any. */
  signal: string | null;
}

/** Shape of the API exposed to the renderer via the preload bridge. */
export interface WorktreeApi {
  /** The user's home directory (for `~` display abbreviation). */
  home: string;
  getConfig(): Promise<AppConfig>;
  setAppSettings(settings: AppSettings): Promise<AppConfig>;
  addRepo(repoPath: string): Promise<AppConfig>;
  updateRepo(repo: RepoConfig): Promise<AppConfig>;
  removeRepo(repoId: string): Promise<AppConfig>;
  listRepos(): Promise<RepoWithWorktrees[]>;
  listWorktrees(repoId: string): Promise<RepoWithWorktrees>;
  createWorktree(params: CreateWorktreeParams): Promise<CreateWorktreeResult>;
  deleteWorktree(params: DeleteWorktreeParams): Promise<DeleteWorktreeResult>;
  listBranches(repoId: string): Promise<string[]>;
  pushWorktree(repoId: string, worktreePath: string): Promise<GitOpResult>;
  pullWorktree(repoId: string, worktreePath: string): Promise<GitOpResult>;
  pullMainIntoWorktree(repoId: string, worktreePath: string): Promise<GitOpResult>;
  switchBranch(repoId: string, worktreePath: string, branch: string): Promise<GitOpResult>;
  /** Start a configured command for a worktree; resolves with the run's metadata. */
  startCommand(repoId: string, worktreePath: string, commandId: string): Promise<RunningCommand>;
  /** Stop a running command (SIGTERM, then SIGKILL after a grace period). */
  stopCommand(worktreePath: string, commandId: string): Promise<void>;
  /** All commands currently running across every worktree. */
  listRunningCommands(): Promise<RunningCommand[]>;
  /** Existing scrollback for a running command (empty once it has exited). */
  getCommandBuffer(worktreePath: string, commandId: string): Promise<string>;
  /** Subscribe to live command output; returns an unsubscribe fn. */
  onCommandOutput(listener: (event: CommandOutputEvent) => void): () => void;
  /** Subscribe to command-exit notifications; returns an unsubscribe fn. */
  onCommandExit(listener: (event: CommandExitEvent) => void): () => void;
  openInEditor(targetPath: string): Promise<void>;
  openInTerminal(targetPath: string): Promise<void>;
  revealInFinder(targetPath: string): Promise<void>;
  /** Open a native directory picker; returns the chosen path or null. */
  pickDirectory(title?: string): Promise<string | null>;
  /** Collapse the window to the Dock (classic collapse box). */
  minimizeWindow(): Promise<void>;
  /** Toggle the classic "zoom" between maximized and restored (zoom box). */
  zoomWindow(): Promise<void>;
  /** Close the window (close box). */
  closeWindow(): Promise<void>;
  /** Resize the window to the given logical size (drives the grow box). */
  setWindowSize(width: number, height: number): Promise<void>;
  /**
   * Subscribe to window activation changes (frontmost vs. background). The
   * listener fires with the current focus state; returns an unsubscribe fn.
   */
  onWindowFocusChange(listener: (focused: boolean) => void): () => void;
  /**
   * Subscribe to "repos may have changed" pushes from the periodic background
   * fetch, so the renderer can refetch its trees. Returns an unsubscribe fn.
   */
  onReposChanged(listener: () => void): () => void;
}
