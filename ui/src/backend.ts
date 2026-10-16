// The page's only way to the core. In the app this is Tauri's IPC; in a
// plain browser (`bun run dev`, tests, screenshots) it is the mock in
// `mock.ts`, so the whole UI can be worked on without the native shell.

import { tauriBackend } from "./tauri";
import type { Boot } from "./wire/Boot";
import type { DeleteRequest } from "./wire/DeleteRequest";
import type { CreateRequest } from "./wire/CreateRequest";
import type { DeleteResult } from "./wire/DeleteResult";
import type { NewWorktreeCheck } from "./wire/NewWorktreeCheck";
import type { NewWorktreeFields } from "./wire/NewWorktreeFields";
import type { RepoEdit } from "./wire/RepoEdit";
import type { Settings } from "./wire/Settings";
import type { MenuState } from "./wire/MenuState";
import type { Model } from "./wire/Model";
import type { ViewState } from "./wire/ViewState";

/// The menu bar's own items (`crates/wtm-tauri/src/menu.rs`).
export type MenuId =
  | "settings"
  | "add-repo"
  | "new-worktree"
  | "switch-branch"
  | "move-up"
  | "move-down"
  | "refresh"
  | "find";

export interface Backend {
  /// What the Rust side handed the page before it loaded.
  boot: Boot;
  /// The first frame is on screen: show the window. Resolves to the model
  /// as it is now, which may be newer than the boot snapshot.
  ready(): Promise<Model>;
  /// Called with every new model.
  onModel(f: (m: Model) => void): void;
  refresh(): void;
  refreshRepo(repoId: string): void;
  clearNotice(): void;
  openInEditor(path: string): void;
  openInTerminal(path: string): void;
  reveal(path: string): void;
  push(repoId: string, path: string): void;
  pull(repoId: string, path: string): void;
  pullMain(repoId: string, path: string): void;
  switchBranch(repoId: string, path: string, branch: string): void;
  /// Runs the core's safety ladder; a refusal says which question is next.
  deleteWorktree(request: DeleteRequest): Promise<DeleteResult>;
  /// What the list looks like now (closed cards, scroll, selection), for
  /// the next launch. Called on every change.
  storeView(view: ViewState): void;
  /// Which of the menu bar's items apply now (macOS; elsewhere a no-op).
  setMenuState(state: MenuState): void;
  /// Called with each menu item chosen, by its id.
  onMenu(f: (id: MenuId) => void): void;
  /// Called as folders are dragged over the window (true) and away or
  /// dropped (false). The drop itself is handled in Rust.
  onFilesOver(f: (over: boolean) => void): void;

  /// The core's verdict on the New Worktree sheet's fields.
  checkNewWorktree(fields: NewWorktreeFields): Promise<NewWorktreeCheck>;
  /// Create, as the last check said; a placeholder row shows at once.
  createWorktree(request: CreateRequest): void;
  /// Fetch a repo's remotes now (the New Worktree sheet opened).
  fetchRepo(repoId: string): void;
  /// Forget a failed creation's row.
  dismissCreation(id: number): void;
  /// Ask for folders and add them as repos.
  addRepos(): void;
  /// Ask for one folder; `null` when cancelled.
  pickFolder(): Promise<string | null>;
  updateRepo(edit: RepoEdit): void;
  removeRepo(repoId: string): void;
  /// Drop a repo in the gap before `shown[gap]` of the repos shown.
  moveRepo(repoId: string, shown: string[], gap: number): void;
  setSettings(settings: Settings): void;
}

declare global {
  interface Window {
    __WTM_BOOT__?: Boot;
    __TAURI_INTERNALS__?: unknown;
  }
}

export async function connect(): Promise<Backend> {
  // Synchronous in the app, so the first render has the boot model; the
  // mock and its demo data are a separate file the app never loads.
  if (window.__TAURI_INTERNALS__ && window.__WTM_BOOT__) {
    return tauriBackend(window.__WTM_BOOT__);
  }
  const { mockBackend } = await import("./mock");
  return mockBackend();
}
