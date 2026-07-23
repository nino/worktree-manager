import { execFile } from "node:child_process";
import { realpath } from "node:fs/promises";
import { basename, dirname } from "node:path";
import { promisify } from "node:util";
import type { WorktreeInfo, WorktreeStatus } from "@shared/types";

const execFileAsync = promisify(execFile);

/** Raised when a git command exits non-zero. */
export class GitError extends Error {
  constructor(
    message: string,
    readonly cwd: string,
    readonly args: string[],
    readonly stderr: string,
  ) {
    super(message);
    this.name = "GitError";
  }
}

/** Run a git command in `cwd` and return its stdout (trimmed of trailing newline). */
export async function runGit(cwd: string, args: string[]): Promise<string> {
  try {
    const { stdout } = await execFileAsync("git", args, {
      cwd,
      maxBuffer: 32 * 1024 * 1024,
      windowsHide: true,
      env: {
        ...process.env,
        // Never let a git subprocess block on an interactive editor or prompt.
        GIT_EDITOR: "true",
        GIT_TERMINAL_PROMPT: "0",
      },
    });
    return stdout;
  } catch (err) {
    const e = err as { stderr?: string; message?: string };
    throw new GitError(
      `git ${args.join(" ")} failed: ${e.stderr?.trim() || e.message}`,
      cwd,
      args,
      e.stderr ?? "",
    );
  }
}

// MARK: Pure parsers (exported for unit testing)

export interface ParsedWorktree {
  path: string;
  head: string;
  branch: string | null;
  detached: boolean;
  locked: boolean;
  bare: boolean;
  /** Set when git reports the worktree directory as missing (deleted on disk). */
  prunable: boolean;
}

/** Parse the output of `git worktree list --porcelain`. */
export function parseWorktreePorcelain(output: string): ParsedWorktree[] {
  const entries: ParsedWorktree[] = [];
  let cur: ParsedWorktree | null = null;

  const flush = (): void => {
    if (cur && cur.path) entries.push(cur);
    cur = null;
  };

  for (const line of output.split("\n")) {
    if (line.trim() === "") {
      flush();
      continue;
    }
    const sp = line.indexOf(" ");
    const key = sp === -1 ? line : line.slice(0, sp);
    const val = sp === -1 ? "" : line.slice(sp + 1);
    switch (key) {
      case "worktree":
        cur = {
          path: val,
          head: "",
          branch: null,
          detached: false,
          locked: false,
          bare: false,
          prunable: false,
        };
        break;
      case "HEAD":
        if (cur) cur.head = val;
        break;
      case "branch":
        if (cur) cur.branch = val.replace(/^refs\/heads\//, "");
        break;
      case "detached":
        if (cur) cur.detached = true;
        break;
      case "locked":
        if (cur) cur.locked = true;
        break;
      case "bare":
        if (cur) cur.bare = true;
        break;
      case "prunable":
        if (cur) cur.prunable = true;
        break;
    }
  }
  flush();
  return entries;
}

export interface ParsedStatus {
  head: string | null;
  detached: boolean;
  oid: string;
  upstream: string | null;
  aheadUpstream: number;
  behindUpstream: number;
  hasStaged: boolean;
  hasUnstaged: boolean;
  hasUntracked: boolean;
}

/** Parse the output of `git status --porcelain=v2 --branch`. */
export function parseStatusPorcelainV2(output: string): ParsedStatus {
  const result: ParsedStatus = {
    head: null,
    detached: false,
    oid: "",
    upstream: null,
    aheadUpstream: 0,
    behindUpstream: 0,
    hasStaged: false,
    hasUnstaged: false,
    hasUntracked: false,
  };

  for (const line of output.split("\n")) {
    if (line === "") continue;
    if (line.startsWith("# branch.oid ")) {
      result.oid = line.slice("# branch.oid ".length);
    } else if (line.startsWith("# branch.head ")) {
      const v = line.slice("# branch.head ".length);
      if (v === "(detached)") {
        result.detached = true;
        result.head = null;
      } else {
        result.head = v;
      }
    } else if (line.startsWith("# branch.upstream ")) {
      result.upstream = line.slice("# branch.upstream ".length);
    } else if (line.startsWith("# branch.ab ")) {
      const m = line.slice("# branch.ab ".length).match(/\+(-?\d+)\s+-(-?\d+)/);
      if (m) {
        result.aheadUpstream = parseInt(m[1], 10);
        result.behindUpstream = parseInt(m[2], 10);
      }
    } else if (line.startsWith("#")) {
      // other header lines: ignore
    } else {
      const type = line[0];
      if (type === "1" || type === "2") {
        const x = line[2];
        const y = line[3];
        if (x !== ".") result.hasStaged = true;
        if (y !== ".") result.hasUnstaged = true;
      } else if (type === "u") {
        result.hasUnstaged = true; // unmerged conflict needs resolution
      } else if (type === "?") {
        result.hasUntracked = true;
      }
    }
  }
  return result;
}

/** Parse `git rev-list --left-right --count <main>...HEAD` → { behind, ahead }. */
export function parseLeftRightCount(output: string): { behind: number; ahead: number } {
  const parts = output.trim().split(/\s+/);
  const behind = parseInt(parts[0] ?? "0", 10);
  const ahead = parseInt(parts[1] ?? "0", 10);
  return { behind: Number.isNaN(behind) ? 0 : behind, ahead: Number.isNaN(ahead) ? 0 : ahead };
}

// MARK: High-level operations

/**
 * Resolve the repository's PRIMARY working tree for a given path, even when
 * the picked folder is a linked worktree (via the shared .git common dir),
 * and canonicalize symlinks so duplicate detection compares real paths.
 */
export async function resolveRepoRoot(somePath: string): Promise<string> {
  const commonDir = (
    await runGit(somePath, ["rev-parse", "--path-format=absolute", "--git-common-dir"])
  ).trim();
  let root: string;
  if (basename(commonDir) === ".git") {
    root = dirname(commonDir);
  } else {
    // Bare repo (or unusual layout): fall back to the picked tree's toplevel.
    root = (await runGit(somePath, ["rev-parse", "--show-toplevel"])).trim();
  }
  return realpath(root);
}

/** Best-effort detection of a repo's default/main branch. */
export async function detectMainBranch(repoPath: string): Promise<string> {
  // 1) origin's default branch, if configured.
  try {
    const ref = (
      await runGit(repoPath, ["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])
    ).trim();
    if (ref.startsWith("origin/")) return ref.slice("origin/".length);
  } catch {
    // no origin/HEAD; fall through
  }
  // 2) Common names that exist locally.
  for (const name of ["main", "master"]) {
    try {
      await runGit(repoPath, ["show-ref", "--verify", "--quiet", `refs/heads/${name}`]);
      return name;
    } catch {
      // not present; try next
    }
  }
  // 3) Whatever branch is currently checked out.
  try {
    const cur = (await runGit(repoPath, ["rev-parse", "--abbrev-ref", "HEAD"])).trim();
    if (cur && cur !== "HEAD") return cur;
  } catch {
    // ignore
  }
  return "main";
}

/**
 * Compute ahead/behind of a worktree's HEAD relative to `mainBranch`.
 * Returns nulls when the comparison fails (e.g. the branch doesn't exist) —
 * "unknown" must never masquerade as "0" in delete decisions.
 */
async function aheadBehindMain(
  worktreePath: string,
  mainBranch: string,
): Promise<{ ahead: number | null; behind: number | null }> {
  try {
    const out = await runGit(worktreePath, [
      "rev-list",
      "--left-right",
      "--count",
      `${mainBranch}...HEAD`,
    ]);
    return parseLeftRightCount(out);
  } catch {
    return { ahead: null, behind: null };
  }
}

/** Compute the full git status for a single worktree. */
export async function getWorktreeStatus(
  worktreePath: string,
  mainBranch: string,
): Promise<WorktreeStatus> {
  const statusOut = await runGit(worktreePath, ["status", "--porcelain=v2", "--branch"]);
  const parsed = parseStatusPorcelainV2(statusOut);
  const { ahead, behind } = await aheadBehindMain(worktreePath, mainBranch);

  return {
    hasUnstaged: parsed.hasUnstaged,
    hasStaged: parsed.hasStaged,
    hasUntracked: parsed.hasUntracked,
    aheadOfMain: ahead,
    behindMain: behind,
    hasUpstream: parsed.upstream !== null,
    unpushedCount: parsed.upstream !== null ? parsed.aheadUpstream : 0,
    unpushed: parsed.upstream !== null && parsed.aheadUpstream > 0,
  };
}

/** List all worktrees for a repo, including per-worktree status. */
export async function listWorktrees(repoPath: string, mainBranch: string): Promise<WorktreeInfo[]> {
  const out = await runGit(repoPath, ["worktree", "list", "--porcelain"]);
  const all = parseWorktreePorcelain(out);
  // git lists the primary working tree (or the bare repo dir) first.
  const primaryPath = all.length > 0 && !all[0].bare ? all[0].path : null;
  const parsed = all.filter((w) => !w.bare);

  return Promise.all(
    parsed.map(async (w): Promise<WorktreeInfo> => {
      let status: WorktreeStatus | null = null;
      if (!w.prunable) {
        try {
          status = await getWorktreeStatus(w.path, mainBranch);
        } catch {
          status = null;
        }
      }
      return {
        path: w.path,
        branch: w.branch,
        head: w.head.slice(0, 12),
        isMain: w.path === primaryPath,
        locked: w.locked,
        prunable: w.prunable,
        status,
      };
    }),
  );
}

/** List local branch names. */
export async function listBranches(repoPath: string): Promise<string[]> {
  const out = await runGit(repoPath, ["for-each-ref", "--format=%(refname:short)", "refs/heads"]);
  return out
    .split("\n")
    .map((s) => s.trim())
    .filter(Boolean);
}

/** Validate a user-supplied branch/ref name; throws a GitError if malformed. */
export async function assertValidRef(repoPath: string, name: string): Promise<void> {
  // check-ref-format also rejects names starting with "-", closing the
  // option-injection hole for positional ref arguments.
  await runGit(repoPath, ["check-ref-format", "--branch", name]);
}

/** Whether the repo has a remote with the given name. */
export async function hasRemote(repoPath: string, name = "origin"): Promise<boolean> {
  try {
    await runGit(repoPath, ["remote", "get-url", name]);
    return true;
  } catch {
    return false;
  }
}

/** Whether a branch already exists locally. */
export async function branchExists(repoPath: string, branch: string): Promise<boolean> {
  try {
    await runGit(repoPath, ["show-ref", "--verify", "--quiet", `refs/heads/${branch}`]);
    return true;
  } catch {
    return false;
  }
}

/** Whether a ref resolves in the repo (any kind: branch, tag, remote-tracking). */
export async function refExists(repoPath: string, ref: string): Promise<boolean> {
  try {
    await runGit(repoPath, ["rev-parse", "--verify", "--quiet", ref]);
    return true;
  } catch {
    return false;
  }
}

/**
 * Fetch updates for a repo's default remote (`git fetch`), pruning deleted
 * remote-tracking branches so ahead/behind counts stay accurate. Resolves even
 * on failure is NOT desired here — callers decide how to handle a throw.
 */
export async function fetchRepo(repoPath: string): Promise<void> {
  await runGit(repoPath, ["fetch", "--prune"]);
}

/** Create a new worktree. */
export async function addWorktree(
  repoPath: string,
  worktreePath: string,
  branch: string,
  newBranch: boolean,
  baseRef?: string,
): Promise<void> {
  const args = ["worktree", "add"];
  if (newBranch) {
    // --no-track: even when basing off a remote-tracking ref (origin/main), the
    // new branch must NOT adopt it as upstream. This app's model is "push sets
    // upstream on first push"; an inherited upstream would break that (push
    // would target origin/main, not origin/<branch>).
    args.push("--no-track", "-b", branch, worktreePath);
    if (baseRef) args.push(baseRef);
  } else {
    args.push(worktreePath, branch);
  }
  await runGit(repoPath, args);
}
