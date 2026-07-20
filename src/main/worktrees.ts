import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { join } from "node:path";
import { mkdir, stat } from "node:fs/promises";
import type {
  CreateWorktreeParams,
  CreateWorktreeResult,
  DeleteWorktreeParams,
  DeleteWorktreeResult,
  GitOpResult,
  InitCommandResult,
  RepoWithWorktrees,
  WorktreeInfo,
} from "@shared/types";
import * as store from "./store";
import {
  GitError,
  addWorktree,
  assertValidRef,
  branchExists,
  getWorktreeStatus,
  hasRemote,
  listWorktrees,
  parseWorktreePorcelain,
  runGit,
} from "./git";

const execFileAsync = promisify(execFile);

// MARK: Per-worktree serialization

const opQueues = new Map<string, Promise<unknown>>();

/**
 * Serialize mutating operations per worktree path, so e.g. a delete can never
 * race a push that is still running on the same worktree.
 */
function withWorktreeLock<T>(key: string, fn: () => Promise<T>): Promise<T> {
  const prev = opQueues.get(key) ?? Promise.resolve();
  const next = prev.then(fn, fn);
  opQueues.set(
    key,
    next.catch(() => {}),
  );
  return next;
}

/** Turn a branch name into a filesystem-safe directory segment. */
export function slugifyBranch(branch: string): string {
  return (
    branch
      .replace(/[/\\]/g, "-")
      .replace(/[^a-zA-Z0-9._-]/g, "-")
      .replace(/-+/g, "-")
      .replace(/^-|-$/g, "") || "worktree"
  );
}

/**
 * Compute the on-disk path for a new worktree:
 * `<worktreesRoot>/<repoName>/<branch-slug>`.
 */
export function worktreePathFor(worktreesRoot: string, repoName: string, branch: string): string {
  return join(worktreesRoot, repoName, slugifyBranch(branch));
}

/** List worktrees for a single repo (existing worktrees included automatically). */
export async function listReposWorktrees(repoId: string): Promise<RepoWithWorktrees> {
  const repo = store.getRepo(repoId);
  try {
    const worktrees = await listWorktrees(repo.path, repo.mainBranch);
    return { repo, worktrees };
  } catch (err) {
    return { repo, worktrees: [], error: (err as Error).message };
  }
}

/** List every configured repo with its worktrees. */
export async function listAllRepos(): Promise<RepoWithWorktrees[]> {
  const { repos } = store.getConfig();
  return Promise.all(repos.map((r) => listReposWorktrees(r.id)));
}

/**
 * Run a repo's configured init command inside a freshly created worktree.
 * Uses the user's login shell (so GUI launches still see nvm/pnpm on PATH)
 * and a hard timeout so a long-lived command can't hang the IPC call forever.
 */
async function runInitCommand(command: string, cwd: string): Promise<InitCommandResult> {
  const trimmed = command.trim();
  if (!trimmed) {
    return { ran: false, exitCode: 0, stdout: "", stderr: "" };
  }
  const loginShell = process.env.SHELL || "/bin/zsh";
  try {
    const { stdout, stderr } = await execFileAsync(loginShell, ["-lc", trimmed], {
      cwd,
      maxBuffer: 32 * 1024 * 1024,
      windowsHide: true,
      timeout: 15 * 60_000,
      killSignal: "SIGTERM",
    });
    return { ran: true, exitCode: 0, stdout, stderr };
  } catch (err) {
    const e = err as {
      code?: number;
      killed?: boolean;
      stdout?: string;
      stderr?: string;
      message?: string;
    };
    return {
      ran: true,
      exitCode: typeof e.code === "number" ? e.code : 1,
      stdout: e.stdout ?? "",
      stderr: e.killed
        ? `Init command timed out after 15 minutes and was terminated.\n${e.stderr ?? ""}`
        : (e.stderr ?? e.message ?? ""),
    };
  }
}

/** Create a new worktree for a repo and run its init command. */
export async function createWorktree(params: CreateWorktreeParams): Promise<CreateWorktreeResult> {
  const repo = store.getRepo(params.repoId);
  const { worktreesRoot } = store.getConfig();
  const branch = params.branch.trim();
  if (!branch) throw new Error("A branch name is required.");
  await assertValidRef(repo.path, branch).catch(() => {
    throw new Error(`Not a valid branch name: ${branch}`);
  });
  if (params.baseRef) {
    await assertValidRef(repo.path, params.baseRef).catch(() => {
      throw new Error(`Not a valid base ref: ${params.baseRef}`);
    });
  }

  if (params.newBranch && (await branchExists(repo.path, branch))) {
    throw new Error(`Branch "${branch}" already exists.`);
  }
  if (!params.newBranch && !(await branchExists(repo.path, branch))) {
    throw new Error(`Branch "${branch}" does not exist.`);
  }

  const targetPath = worktreePathFor(worktreesRoot, repo.name, branch);
  const init = await withWorktreeLock(targetPath, async () => {
    if (await pathExists(targetPath)) {
      throw new Error(`Target path already exists: ${targetPath}`);
    }

    // Ensure the repo's parent directory under the worktrees root exists.
    await mkdir(join(worktreesRoot, repo.name), { recursive: true });

    await addWorktree(
      repo.path,
      targetPath,
      branch,
      params.newBranch,
      params.baseRef ?? (params.newBranch ? repo.mainBranch : undefined),
    );

    return runInitCommand(repo.initCommand, targetPath);
  });

  const status = await getWorktreeStatus(targetPath, repo.mainBranch).catch(() => null);
  const worktree: WorktreeInfo = {
    path: targetPath,
    branch,
    head: "",
    isMain: false,
    locked: false,
    prunable: false,
    status,
  };
  return { worktree, init };
}

/**
 * Delete a worktree.
 *
 * Safety ladder:
 * 1. The path must be one of the repo's worktrees, verbatim per git's own list.
 * 2. The primary working tree is never deletable.
 * 3. The worktree must still be on the branch the UI showed (`expectedBranch`);
 *    otherwise the row was stale and we refuse with reason "changed".
 * 4. `git worktree remove` runs WITHOUT --force first, so git's own dirty-tree
 *    protection applies. A dirty tree returns reason "dirty" and only an
 *    explicit `force: true` (second confirmation in the UI) escalates.
 * 5. Worktrees whose folder is already gone ("prunable") are cleaned up with
 *    `git worktree prune` — but only when no OTHER worktree is also prunable,
 *    since prune is repo-wide and must not eat e.g. a temporarily unmounted
 *    volume's worktree.
 *
 * No `rm -rf` fallback: `git worktree remove` deletes the directory itself.
 */
export function deleteWorktree(params: DeleteWorktreeParams): Promise<DeleteWorktreeResult> {
  return withWorktreeLock(params.worktreePath, () => deleteWorktreeUnlocked(params));
}

async function deleteWorktreeUnlocked(params: DeleteWorktreeParams): Promise<DeleteWorktreeResult> {
  const repo = store.getRepo(params.repoId);
  const out = await runGit(repo.path, ["worktree", "list", "--porcelain"]);
  const all = parseWorktreePorcelain(out);

  const entry = all.find((w) => w.path === params.worktreePath);
  if (!entry) {
    return {
      ok: false,
      reason: "changed",
      message: `${params.worktreePath} is not (or no longer) a worktree of ${repo.name}. Refresh and try again.`,
    };
  }

  const primaryPath = all.length > 0 && !all[0].bare ? all[0].path : null;
  if (entry.bare || entry.path === primaryPath) {
    return {
      ok: false,
      reason: "error",
      message: "Refusing to delete the repository's primary working tree.",
    };
  }

  const currentBranch = entry.branch;
  if (currentBranch !== params.expectedBranch) {
    return {
      ok: false,
      reason: "changed",
      message: `This worktree is now on “${currentBranch ?? "(detached)"}”, not “${params.expectedBranch ?? "(detached)"}”. Refresh and re-confirm.`,
    };
  }

  // Folder already deleted outside the app: targeted bookkeeping cleanup.
  if (entry.prunable) {
    const otherPrunable = all.filter((w) => w.prunable && w.path !== entry.path);
    if (otherPrunable.length > 0) {
      return {
        ok: false,
        reason: "error",
        message:
          `Other worktrees also have missing folders (${otherPrunable
            .map((w) => w.branch ?? w.path)
            .join(", ")}). ` +
          `git can only prune them all together — if any live on an unmounted drive, remount it first, then delete these rows individually.`,
      };
    }
    await runGit(repo.path, ["worktree", "prune"]);
    return { ok: true, message: "Cleaned up git bookkeeping for the missing folder." };
  }

  // Without --force: git refuses dirty trees, which is exactly what we want.
  try {
    await runGit(repo.path, ["worktree", "remove", params.worktreePath]);
    return { ok: true, message: "" };
  } catch (err) {
    const message = err instanceof GitError ? err.stderr.trim() || err.message : String(err);
    const dirty = /contains modified or untracked files|use --force/i.test(message);
    if (!dirty) return { ok: false, reason: "error", message };
    if (!params.force) return { ok: false, reason: "dirty", message };
  }

  await runGit(repo.path, ["worktree", "remove", "--force", params.worktreePath]);
  return { ok: true, message: "" };
}

async function pathExists(p: string): Promise<boolean> {
  try {
    await stat(p);
    return true;
  } catch {
    return false;
  }
}

// MARK: Worktree git operations (push / pull / switch)

/** Assert that `worktreePath` is one of the repo's worktrees; returns the repo. */
async function requireWorktree(repoId: string, worktreePath: string) {
  const repo = store.getRepo(repoId);
  const worktrees = await listWorktreesRaw(repo.path);
  if (!worktrees.includes(worktreePath)) {
    throw new Error(`Not a worktree of ${repo.name}: ${worktreePath}`);
  }
  return repo;
}

async function listWorktreesRaw(repoPath: string): Promise<string[]> {
  const out = await runGit(repoPath, ["worktree", "list", "--porcelain"]);
  return out
    .split("\n")
    .filter((l) => l.startsWith("worktree "))
    .map((l) => l.slice("worktree ".length));
}

/** Convert a git success/failure into a GitOpResult (never throws). */
function opResult(fn: () => Promise<string>): Promise<GitOpResult> {
  return fn().then(
    (output) => ({ ok: true, message: output.trim() }),
    (err: unknown) => ({
      ok: false,
      message:
        err instanceof GitError && err.stderr.trim()
          ? err.stderr.trim()
          : ((err as Error).message ?? String(err)),
    }),
  );
}

/** Push the worktree's branch, setting upstream on first push. */
export function pushWorktree(repoId: string, worktreePath: string): Promise<GitOpResult> {
  return withWorktreeLock(worktreePath, async () => {
    await requireWorktree(repoId, worktreePath);
    const first = await opResult(() => runGit(worktreePath, ["push"]));
    if (
      !first.ok &&
      /no upstream|set-upstream|no configured push destination/i.test(first.message)
    ) {
      return opResult(() => runGit(worktreePath, ["push", "-u", "origin", "HEAD"]));
    }
    return first;
  });
}

/** Fast-forward pull of the worktree's branch (never creates surprise merges). */
export function pullWorktree(repoId: string, worktreePath: string): Promise<GitOpResult> {
  return withWorktreeLock(worktreePath, async () => {
    await requireWorktree(repoId, worktreePath);
    return opResult(() => runGit(worktreePath, ["pull", "--ff-only"]));
  });
}

/**
 * Pull the repo's primary branch into this worktree's branch.
 * Explicit --no-rebase so a user's `pull.rebase` config can never rewrite the
 * branch's history from this button; --no-edit avoids editor prompts.
 */
export function pullMainIntoWorktree(repoId: string, worktreePath: string): Promise<GitOpResult> {
  return withWorktreeLock(worktreePath, async () => {
    const repo = await requireWorktree(repoId, worktreePath);
    try {
      await assertValidRef(repo.path, repo.mainBranch);
    } catch {
      return {
        ok: false,
        message: `Configured main branch is not a valid ref: ${repo.mainBranch}`,
      };
    }
    if (await hasRemote(repo.path)) {
      return opResult(() =>
        runGit(worktreePath, ["pull", "--no-rebase", "--no-edit", "origin", repo.mainBranch]),
      );
    }
    return opResult(() => runGit(worktreePath, ["merge", "--no-edit", repo.mainBranch]));
  });
}

/**
 * Switch the worktree to another branch. Plain `git switch`: git itself
 * refuses when local changes would be overwritten or the branch is already
 * checked out in another worktree, so this cannot lose work.
 */
export function switchWorktreeBranch(
  repoId: string,
  worktreePath: string,
  branch: string,
): Promise<GitOpResult> {
  return withWorktreeLock(worktreePath, async () => {
    await requireWorktree(repoId, worktreePath);
    return opResult(() => runGit(worktreePath, ["switch", branch]));
  });
}
