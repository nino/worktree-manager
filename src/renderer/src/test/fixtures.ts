import type { RepoConfig, RepoWithWorktrees, WorktreeInfo, WorktreeStatus } from "@shared/types";

export function makeStatus(over: Partial<WorktreeStatus> = {}): WorktreeStatus {
  return {
    hasUnstaged: false,
    hasStaged: false,
    hasUntracked: false,
    aheadOfMain: 0,
    behindMain: 0,
    unpushed: false,
    unpushedCount: 0,
    hasUpstream: true,
    ...over,
  };
}

export function makeWorktree(over: Partial<WorktreeInfo> = {}): WorktreeInfo {
  return {
    path: "/Users/test/worktrees/app/feature",
    branch: "feature",
    head: "abc1234",
    isMain: false,
    locked: false,
    prunable: false,
    status: makeStatus(),
    ...over,
  };
}

export function makeRepo(over: Partial<RepoConfig> = {}): RepoConfig {
  return {
    id: "r1",
    name: "app",
    path: "/Users/test/dev/app",
    mainBranch: "main",
    initCommand: "",
    commands: [],
    ...over,
  };
}

/** A repo view-model node (repo + its worktrees), as `listRepos` returns. */
export function makeNode(
  repo: Partial<RepoConfig> = {},
  worktrees: WorktreeInfo[] = [],
): RepoWithWorktrees {
  const r = makeRepo(repo);
  return { repo: r, worktrees, defaultBaseRef: `origin/${r.mainBranch}` };
}

/** A promise you resolve/reject by hand — for asserting in-flight UI states. */
export function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}
