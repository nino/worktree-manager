// The page's only way to the core. In the app this is Tauri's IPC; in a
// plain browser (`bun run dev`, tests, screenshots) it is the mock in
// `mock.ts`, so the whole UI can be worked on without the native shell.

import { tauriBackend } from "./tauri";
import type { Boot } from "./wire/Boot";
import type { DeleteRequest } from "./wire/DeleteRequest";
import type { DeleteResult } from "./wire/DeleteResult";
import type { Model } from "./wire/Model";

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
  /// Remember which cards are closed for the next launch.
  setClosedRepos(ids: string[]): void;
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
