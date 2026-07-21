import { homedir } from "node:os";
import { contextBridge, ipcRenderer } from "electron";
import type {
  AppSettings,
  CreateWorktreeParams,
  CreateWorktreeResult,
  DeleteWorktreeParams,
  RepoConfig,
  RepoWithWorktrees,
  WorktreeApi,
} from "@shared/types";

const CH = {
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

const api: WorktreeApi = {
  home: homedir(),
  getConfig: () => ipcRenderer.invoke(CH.getConfig),
  setAppSettings: (settings: AppSettings) => ipcRenderer.invoke(CH.setAppSettings, settings),
  addRepo: (repoPath: string) => ipcRenderer.invoke(CH.addRepo, repoPath),
  updateRepo: (repo: RepoConfig) => ipcRenderer.invoke(CH.updateRepo, repo),
  removeRepo: (repoId: string) => ipcRenderer.invoke(CH.removeRepo, repoId),
  listRepos: () => ipcRenderer.invoke(CH.listRepos),
  listWorktrees: (repoId: string) => ipcRenderer.invoke(CH.listWorktrees, repoId),
  createWorktree: (params: CreateWorktreeParams): Promise<CreateWorktreeResult> =>
    ipcRenderer.invoke(CH.createWorktree, params),
  deleteWorktree: (params: DeleteWorktreeParams) => ipcRenderer.invoke(CH.deleteWorktree, params),
  listBranches: (repoId: string) => ipcRenderer.invoke(CH.listBranches, repoId),
  pushWorktree: (repoId: string, worktreePath: string) =>
    ipcRenderer.invoke(CH.pushWorktree, repoId, worktreePath),
  pullWorktree: (repoId: string, worktreePath: string) =>
    ipcRenderer.invoke(CH.pullWorktree, repoId, worktreePath),
  pullMainIntoWorktree: (repoId: string, worktreePath: string) =>
    ipcRenderer.invoke(CH.pullMainIntoWorktree, repoId, worktreePath),
  switchBranch: (repoId: string, worktreePath: string, branch: string) =>
    ipcRenderer.invoke(CH.switchBranch, repoId, worktreePath, branch),
  openInEditor: (targetPath: string) => ipcRenderer.invoke(CH.openInEditor, targetPath),
  openInTerminal: (targetPath: string) => ipcRenderer.invoke(CH.openInTerminal, targetPath),
  revealInFinder: (targetPath: string) => ipcRenderer.invoke(CH.revealInFinder, targetPath),
  pickDirectory: (title?: string) => ipcRenderer.invoke(CH.pickDirectory, title),
  minimizeWindow: () => ipcRenderer.invoke(CH.windowMinimize),
  zoomWindow: () => ipcRenderer.invoke(CH.windowZoom),
  closeWindow: () => ipcRenderer.invoke(CH.windowClose),
  setWindowSize: (width: number, height: number) =>
    ipcRenderer.invoke(CH.windowSetSize, width, height),
  onWindowFocusChange: (listener: (focused: boolean) => void) => {
    const handler = (_e: unknown, focused: boolean) => listener(focused);
    ipcRenderer.on(CH.windowFocusChanged, handler);
    return () => ipcRenderer.removeListener(CH.windowFocusChanged, handler);
  },
};

contextBridge.exposeInMainWorld("api", api);
