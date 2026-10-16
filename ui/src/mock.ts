// A backend for a plain browser: demo repos, and actions that behave
// roughly as the core's do (a spinner, then a notice). `bun run dev` and
// the screenshot script run the UI against this.

import type { Backend } from "./backend";
import type { Busy } from "./wire/Busy";
import type { Model } from "./wire/Model";
import type { Status } from "./wire/Status";
import type { Worktree } from "./wire/Worktree";

const status = (s: Partial<Status> = {}): Status => ({
  hasStaged: false,
  hasUnstaged: false,
  hasUntracked: false,
  trunkRef: "origin/main",
  ahead: 0,
  behind: 0,
  hasUpstream: true,
  unpushed: 0,
  ...s,
});

const worktree = (path: string, branch: string | null, w: Partial<Worktree> = {}): Worktree => ({
  path,
  displayPath: path.replace(/^\/home\/demo/, "~"),
  branch,
  head: "1a2b3c4",
  isMain: false,
  locked: false,
  prunable: false,
  busy: null,
  status: status(),
  ...w,
});

export function demoModel(): Model {
  return {
    refreshing: false,
    fetching: false,
    notice: null,
    repos: [
      {
        id: "app",
        name: "app",
        path: "/home/demo/src/app",
        displayPath: "~/src/app",
        mainBranch: "main",
        branches: ["main", "fix-login", "claude/tauri-rewrite"],
        error: null,
        loaded: true,
        pending: [],
        worktrees: [
          worktree("/home/demo/src/app", "main", { isMain: true }),
          worktree("/home/demo/worktrees/app/claude-tauri-rewrite", "claude/tauri-rewrite", {
            status: status({ hasUnstaged: true, hasUntracked: true, ahead: 4, unpushed: 2 }),
          }),
          worktree("/home/demo/worktrees/app/fix-login", "fix-login", {
            status: status({ hasStaged: true, behind: 3, hasUpstream: false }),
          }),
        ],
      },
      {
        id: "site",
        name: "site",
        path: "/home/demo/src/site",
        displayPath: "~/src/site",
        mainBranch: "main",
        branches: ["main"],
        error: null,
        loaded: true,
        pending: [{ id: 1, branch: "docs/new-page", folder: "docs-new-page", error: null }],
        worktrees: [
          worktree("/home/demo/src/site", "main", { isMain: true }),
          worktree("/home/demo/worktrees/site/old-experiment", "old-experiment", {
            prunable: true,
            status: null,
          }),
        ],
      },
    ],
  };
}

export function mockBackend(): Backend {
  let model = demoModel();
  const listeners: ((m: Model) => void)[] = [];
  const publish = (next: Model) => {
    model = next;
    for (const f of listeners) f(model);
  };
  let noticeId = 0;
  const notify = (text: string, tone: "info" | "error" = "info") =>
    publish({ ...model, notice: { id: ++noticeId, tone, text } });
  const setBusy = (path: string, busy: Busy | null) =>
    publish({
      ...model,
      repos: model.repos.map((r) => ({
        ...r,
        worktrees: r.worktrees.map((w) => (w.path === path ? { ...w, busy } : w)),
      })),
    });
  const op = (busy: Busy, done: string) => (_repoId: string, path: string) => {
    setBusy(path, busy);
    setTimeout(() => {
      setBusy(path, null);
      notify(done);
    }, 800);
  };
  return {
    boot: { model, fileManager: "Files" },
    ready: async () => model,
    onModel: (f) => listeners.push(f),
    refresh: () => {
      publish({ ...model, refreshing: true });
      setTimeout(() => publish({ ...model, refreshing: false }), 600);
    },
    refreshRepo: () => {},
    clearNotice: () => publish({ ...model, notice: null }),
    openInEditor: (path) => notify(`Would open ${path} in the editor.`),
    openInTerminal: (path) => notify(`Would open a terminal in ${path}.`),
    reveal: (path) => notify(`Would reveal ${path}.`),
    push: op("pushing", "Pushed."),
    pull: op("pulling", "Already up to date."),
    pullMain: op("merging", "Merged origin/main."),
  };
}
