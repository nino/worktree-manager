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
    settings: {
      worktreesRoot: "/home/demo/worktrees",
      editorCommand: "code",
      copyWorktreeInclude: false,
      updateChannel: "stable",
    },
    repos: [
      {
        id: "app",
        name: "app",
        path: "/home/demo/src/app",
        displayPath: "~/src/app",
        mainBranch: "main",
        initCommand: "bun install",
        defaultBaseRef: "origin/main",
        baseRefCandidates: ["origin/main", "main", "fix-login"],
        branches: ["main", "fix-login", "claude/tauri-rewrite", "release/2.0"],
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
        initCommand: "",
        defaultBaseRef: "main",
        baseRefCandidates: ["main"],
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
  let pendingId = 100;
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
    boot: {
      model,
      fileManager: "Files",
      os: "browser",
      terminalNote: "“Open in terminal” uses $TERMINAL, else the first terminal found.",
      view: { closedRepos: [], scroll: 0, focus: null },
      menuBar: false,
    },
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
    switchBranch: (_repoId, path, branch) => {
      setBusy(path, "switching");
      setTimeout(
        () =>
          publish({
            ...model,
            repos: model.repos.map((r) => ({
              ...r,
              worktrees: r.worktrees.map((w) => (w.path === path ? { ...w, branch, busy: null } : w)),
            })),
          }),
        600,
      );
    },
    // As git does: a tree with changes is refused until forced.
    deleteWorktree: async (req) => {
      const w = model.repos.flatMap((r) => r.worktrees).find((w) => w.path === req.path);
      if (!w) return { ok: false, refusal: "changed", message: "That worktree is not listed any more." };
      const s = w.status;
      if (!req.force && s && (s.hasStaged || s.hasUnstaged || s.hasUntracked)) {
        return { ok: false, refusal: "dirty", message: "The worktree has uncommitted changes." };
      }
      publish({
        ...model,
        repos: model.repos.map((r) => ({ ...r, worktrees: r.worktrees.filter((w) => w.path !== req.path) })),
      });
      return { ok: true, refusal: null, message: "Deleted." };
    },
    storeView: () => {},
    setMenuState: () => {},
    onMenu: () => {},
    onFilesOver: () => {},

    // A rough `new_worktree::check`: enough to drive the sheet by hand.
    checkNewWorktree: async (f) => {
      const repo = model.repos.find((r) => r.id === f.repoId)!;
      const name = f.name.trim();
      const off = (problem: string) => ({ create: null, problem, note: null, fromRemote: false, folder });
      const folder = name ? `~/worktrees/${repo.name}/${name.replaceAll("/", "-")}` : null;
      if (!name) return off("");
      if (/\s|\.\.|^-|[~^:?*[\\]/.test(name)) return off("Not a valid branch name.");
      if (repo.worktrees.some((w) => w.branch === name)) return off("Branch already has a worktree.");
      const local = repo.branches.includes(name);
      if (local && f.newBranch) return off("Branch already exists; use Existing branch.");
      if (!local && !f.newBranch) return off("No branch of that name.");
      const source = f.newBranch
        ? { kind: "new" as const, baseRef: f.base.trim() || null }
        : { kind: "existing" as const };
      return { create: { repoId: f.repoId, branch: name, source }, problem: null, note: null, fromRemote: false, folder };
    },
    createWorktree: (req) => {
      const id = ++pendingId;
      const folder = req.branch.replaceAll("/", "-");
      publish({
        ...model,
        repos: model.repos.map((r) =>
          r.id === req.repoId ? { ...r, pending: [...r.pending, { id, branch: req.branch, folder, error: null }] } : r,
        ),
      });
      setTimeout(() => {
        publish({
          ...model,
          repos: model.repos.map((r) =>
            r.id === req.repoId
              ? {
                  ...r,
                  pending: r.pending.filter((p) => p.id !== id),
                  branches: r.branches.includes(req.branch) ? r.branches : [...r.branches, req.branch],
                  worktrees: [...r.worktrees, worktree(`/home/demo/worktrees/${r.name}/${folder}`, req.branch)],
                }
              : r,
          ),
        });
      }, 1200);
    },
    fetchRepo: () => {},
    dismissCreation: (id) =>
      publish({ ...model, repos: model.repos.map((r) => ({ ...r, pending: r.pending.filter((p) => p.id !== id) })) }),
    addRepos: () => notify("Would ask for folders to add."),
    pickFolder: async () => null,
    updateRepo: (edit) =>
      publish({
        ...model,
        repos: model.repos.map((r) =>
          r.id === edit.repoId
            ? {
                ...r,
                name: edit.name.trim() || r.name,
                mainBranch: edit.mainBranch.trim() || "main",
                initCommand: edit.initCommand,
              }
            : r,
        ),
      }),
    removeRepo: (repoId) => publish({ ...model, repos: model.repos.filter((r) => r.id !== repoId) }),
    moveRepo: (repoId, shown, gap) => {
      const from = shown.indexOf(repoId);
      if (from < 0 || gap === from || gap === from + 1 || gap > shown.length) return;
      const before = shown[gap] ?? null;
      const rest = model.repos.filter((r) => r.id !== repoId);
      const moved = model.repos.find((r) => r.id === repoId)!;
      const at = before === null ? rest.length : rest.findIndex((r) => r.id === before);
      publish({ ...model, repos: [...rest.slice(0, at), moved, ...rest.slice(at)] });
    },
    setSettings: (settings) => publish({ ...model, settings }),
  };
}
