import { ipcMain, dialog, BrowserWindow } from "electron";
import type {
  AppConfig,
  AppSettings,
  CreateWorktreeParams,
  CreateWorktreeResult,
  DeleteWorktreeParams,
  DeleteWorktreeResult,
  RepoConfig,
  RepoWithWorktrees,
} from "@shared/types";
import * as store from "./store";
import { assertValidRef, detectMainBranch, listBranches, resolveRepoRoot } from "./git";
import {
  createWorktree,
  deleteWorktree,
  listAllRepos,
  listReposWorktrees,
  pullMainIntoWorktree,
  pullWorktree,
  pushWorktree,
  switchWorktreeBranch,
} from "./worktrees";
import { openInEditor, openInTerminal, revealInFinder } from "./system";

// MARK: Channel names

export const CH = {
  getConfig: "config:get",
  setAppSettings: "config:setAppSettings",
  addRepo: "repo:add",
  updateRepo: "repo:update",
  removeRepo: "repo:remove",
  listRepos: "repo:list",
  listWorktrees: "worktree:list",
  createWorktree: "worktree:create",
  deleteWorktree: "worktree:delete",
  listBranches: "repo:branches",
  pushWorktree: "worktree:push",
  pullWorktree: "worktree:pull",
  pullMainIntoWorktree: "worktree:pullMain",
  switchBranch: "worktree:switchBranch",
  openInEditor: "system:openInEditor",
  openInTerminal: "system:openInTerminal",
  revealInFinder: "system:revealInFinder",
  pickDirectory: "system:pickDirectory",
  windowMinimize: "window:minimize",
  windowZoom: "window:zoom",
  windowClose: "window:close",
  windowSetSize: "window:setSize",
  windowFocusChanged: "window:focusChanged",
} as const;

// MARK: Handler registration

/** Register all IPC handlers. Call once, after `app` is ready. */
export function registerIpc(): void {
  ipcMain.handle(CH.getConfig, (): AppConfig => store.getConfig());

  ipcMain.handle(CH.setAppSettings, (_e, settings: AppSettings): AppConfig =>
    store.setAppSettings(settings),
  );

  ipcMain.handle(CH.addRepo, async (_e, repoPath: string): Promise<AppConfig> => {
    const root = await resolveRepoRoot(repoPath);
    const mainBranch = await detectMainBranch(root);
    return store.addRepo({ path: root, mainBranch });
  });

  ipcMain.handle(CH.updateRepo, async (_e, repo: RepoConfig): Promise<AppConfig> => {
    await assertValidRef(repo.path, repo.mainBranch).catch(() => {
      throw new Error(`Not a valid branch name: ${repo.mainBranch}`);
    });
    return store.updateRepo(repo);
  });

  ipcMain.handle(CH.removeRepo, (_e, repoId: string): AppConfig => store.removeRepo(repoId));

  ipcMain.handle(CH.listRepos, (): Promise<RepoWithWorktrees[]> => listAllRepos());

  ipcMain.handle(CH.listWorktrees, (_e, repoId: string): Promise<RepoWithWorktrees> =>
    listReposWorktrees(repoId),
  );

  ipcMain.handle(
    CH.createWorktree,
    (_e, params: CreateWorktreeParams): Promise<CreateWorktreeResult> => createWorktree(params),
  );

  ipcMain.handle(
    CH.deleteWorktree,
    (_e, params: DeleteWorktreeParams): Promise<DeleteWorktreeResult> => deleteWorktree(params),
  );

  ipcMain.handle(CH.listBranches, (_e, repoId: string): Promise<string[]> => {
    return listBranches(store.getRepo(repoId).path);
  });

  ipcMain.handle(CH.pushWorktree, (_e, repoId: string, worktreePath: string) =>
    pushWorktree(repoId, worktreePath),
  );

  ipcMain.handle(CH.pullWorktree, (_e, repoId: string, worktreePath: string) =>
    pullWorktree(repoId, worktreePath),
  );

  ipcMain.handle(CH.pullMainIntoWorktree, (_e, repoId: string, worktreePath: string) =>
    pullMainIntoWorktree(repoId, worktreePath),
  );

  ipcMain.handle(CH.switchBranch, (_e, repoId: string, worktreePath: string, branch: string) =>
    switchWorktreeBranch(repoId, worktreePath, branch),
  );

  ipcMain.handle(CH.openInEditor, (_e, targetPath: string): void => openInEditor(targetPath));

  ipcMain.handle(CH.openInTerminal, (_e, targetPath: string): void => openInTerminal(targetPath));

  ipcMain.handle(CH.revealInFinder, (_e, targetPath: string): Promise<void> =>
    revealInFinder(targetPath),
  );

  ipcMain.handle(CH.pickDirectory, async (e, title?: string): Promise<string | null> => {
    const win = BrowserWindow.fromWebContents(e.sender) ?? undefined;
    const result = await dialog.showOpenDialog(win!, {
      title: title ?? "Select a folder",
      properties: ["openDirectory", "createDirectory"],
    });
    if (result.canceled || result.filePaths.length === 0) return null;
    return result.filePaths[0];
  });

  // MARK: Window controls (classic close / collapse / zoom boxes)

  ipcMain.handle(CH.windowMinimize, (e): void => {
    BrowserWindow.fromWebContents(e.sender)?.minimize();
  });

  ipcMain.handle(CH.windowZoom, (e): void => {
    const win = BrowserWindow.fromWebContents(e.sender);
    if (!win) return;
    if (win.isMaximized()) win.unmaximize();
    else win.maximize();
  });

  ipcMain.handle(CH.windowClose, (e): void => {
    BrowserWindow.fromWebContents(e.sender)?.close();
  });

  // Drives the classic grow-box drag. The renderer sends the desired outer
  // size in logical pixels; Electron clamps it to the window's min/max.
  ipcMain.handle(CH.windowSetSize, (e, width: number, height: number): void => {
    BrowserWindow.fromWebContents(e.sender)?.setSize(Math.round(width), Math.round(height));
  });
}
