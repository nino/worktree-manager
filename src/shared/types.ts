/** Types shared across the main and renderer processes. */

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
}
