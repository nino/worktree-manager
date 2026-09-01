import {
  ipcMain,
  dialog,
  BrowserWindow,
  type IpcMainInvokeEvent,
  type WebContents,
} from "electron";
import type {
  AddReposResult,
  AppConfig,
  AppSettings,
  CreateWorktreeParams,
  CreateWorktreeResult,
  DeleteWorktreeParams,
  DeleteWorktreeResult,
  RepoConfig,
  RepoWithWorktrees,
  UpdateStatus,
} from "@shared/types";
import * as store from "./store";
import { assertValidRef, listBaseRefCandidates, listBranches } from "./git";
import { addRepos } from "./repos";
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
import { getCommandBuffer, listRunningCommands, startCommand, stopCommand } from "./commands";
import { checkForUpdates, getUpdateStatus, installUpdate } from "./updater";

// MARK: Channel names

export const CH = {
  getConfig: "config:get",
  setAppSettings: "config:setAppSettings",
  addRepos: "repo:add",
  updateRepo: "repo:update",
  removeRepo: "repo:remove",
  listRepos: "repo:list",
  listWorktrees: "worktree:list",
  createWorktree: "worktree:create",
  deleteWorktree: "worktree:delete",
  listBranches: "repo:branches",
  listBaseRefCandidates: "repo:baseRefCandidates",
  pushWorktree: "worktree:push",
  pullWorktree: "worktree:pull",
  pullMainIntoWorktree: "worktree:pullMain",
  switchBranch: "worktree:switchBranch",
  startCommand: "command:start",
  stopCommand: "command:stop",
  listRunningCommands: "command:listRunning",
  getCommandBuffer: "command:buffer",
  // Streamed events (send/on). Must match CommandEvents in commands.ts.
  commandOutput: "command:output",
  commandExit: "command:exit",
  openInEditor: "system:openInEditor",
  openInTerminal: "system:openInTerminal",
  revealInFinder: "system:revealInFinder",
  pickDirectory: "system:pickDirectory",
  pickDirectories: "system:pickDirectories",
  windowMinimize: "window:minimize",
  windowZoom: "window:zoom",
  windowClose: "window:close",
  windowSetSize: "window:setSize",
  windowFocusChanged: "window:focusChanged",
  getUpdateStatus: "update:get",
  checkForUpdates: "update:check",
  installUpdate: "update:install",
  // Pushed after a background fetch cycle so the renderer refreshes its trees.
  reposChanged: "repo:changed",
  // Pushed whenever the auto-updater changes state (checking/downloading/ready).
  updateStatus: "update:status",
  // Dock drops: the renderer drains what it missed, then listens for the rest.
  takeDroppedRepos: "repo:takeDropped",
  reposDropped: "repo:dropped",
} as const;

// MARK: Dock-drop delivery

/**
 * Outcomes of Dock drops that no renderer has picked up yet.
 *
 * A drop can be handled before there is a window at all (dropping onto the Dock
 * icon launches the app), so results are queued until a renderer announces
 * itself by calling `takeDroppedRepos`; later drops go straight to it.
 */
let undeliveredDrops: AddReposResult[] = [];
let dropListener: WebContents | null = null;

/** Hand a Dock-drop outcome to the renderer, queueing it if none is listening. */
export function publishDropResult(result: AddReposResult): void {
  if (dropListener && !dropListener.isDestroyed()) dropListener.send(CH.reposDropped, result);
  else undeliveredDrops.push(result);
}

// MARK: Folder picker

/** Show the native folder picker sheet; returns [] when it is cancelled. */
async function pickFolders(
  e: IpcMainInvokeEvent,
  title: string,
  multiple: boolean,
): Promise<string[]> {
  const win = BrowserWindow.fromWebContents(e.sender) ?? undefined;
  const properties: ("openDirectory" | "createDirectory" | "multiSelections")[] = [
    "openDirectory",
    "createDirectory",
  ];
  if (multiple) properties.push("multiSelections");
  const result = await dialog.showOpenDialog(win!, { title, properties });
  return result.canceled ? [] : result.filePaths;
}

// MARK: Handler registration

/** Register all IPC handlers. Call once, after `app` is ready. */
export function registerIpc(): void {
  ipcMain.handle(CH.getConfig, (): AppConfig => store.getConfig());

  ipcMain.handle(CH.setAppSettings, (_e, settings: AppSettings): AppConfig =>
    store.setAppSettings(settings),
  );

  ipcMain.handle(CH.addRepos, (_e, repoPaths: string[]): Promise<AddReposResult> =>
    addRepos(repoPaths),
  );

  ipcMain.handle(CH.takeDroppedRepos, (e): AddReposResult[] => {
    // This renderer is now listening; send subsequent drops straight to it.
    dropListener = e.sender;
    const queued = undeliveredDrops;
    undeliveredDrops = [];
    return queued;
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

  ipcMain.handle(CH.listBaseRefCandidates, (_e, repoId: string): Promise<string[]> => {
    return listBaseRefCandidates(store.getRepo(repoId).path);
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

  // MARK: Per-worktree commands (integrated terminal)

  ipcMain.handle(CH.startCommand, (_e, repoId: string, worktreePath: string, commandId: string) =>
    startCommand(repoId, worktreePath, commandId),
  );

  ipcMain.handle(CH.stopCommand, (_e, worktreePath: string, commandId: string): void =>
    stopCommand(worktreePath, commandId),
  );

  ipcMain.handle(CH.listRunningCommands, () => listRunningCommands());

  ipcMain.handle(CH.getCommandBuffer, (_e, worktreePath: string, commandId: string): string =>
    getCommandBuffer(worktreePath, commandId),
  );

  ipcMain.handle(CH.openInEditor, (_e, targetPath: string): void => openInEditor(targetPath));

  ipcMain.handle(CH.openInTerminal, (_e, targetPath: string): void => openInTerminal(targetPath));

  ipcMain.handle(CH.revealInFinder, (_e, targetPath: string): Promise<void> =>
    revealInFinder(targetPath),
  );

  ipcMain.handle(CH.pickDirectory, async (e, title?: string): Promise<string | null> => {
    const paths = await pickFolders(e, title ?? "Select a folder", false);
    return paths[0] ?? null;
  });

  ipcMain.handle(CH.pickDirectories, (e, title?: string): Promise<string[]> =>
    pickFolders(e, title ?? "Select folders", true),
  );

  // MARK: Automatic updates

  ipcMain.handle(CH.getUpdateStatus, (): UpdateStatus => getUpdateStatus());

  ipcMain.handle(CH.checkForUpdates, (): Promise<UpdateStatus> => checkForUpdates());

  ipcMain.handle(CH.installUpdate, (): void => installUpdate());

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
