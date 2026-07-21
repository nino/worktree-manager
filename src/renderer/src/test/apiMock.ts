import { type Mock, vi } from "vitest";
import type { WorktreeApi } from "@shared/types";

/** Every method of the API surface becomes a vi mock; non-fn props stay as-is. */
type MockedApi = {
  [K in keyof WorktreeApi]: WorktreeApi[K] extends (...args: never[]) => unknown
    ? Mock
    : WorktreeApi[K];
};

/**
 * A single mock of the preload `window.api`. `api.ts` captures this object by
 * reference at import time, so the same instance lives for the whole run and
 * tests reconfigure its methods via `resetApiMock()` + per-test `mock*` calls.
 */
export const apiMock: MockedApi = {
  home: "/Users/test",
  getConfig: vi.fn(),
  setAppSettings: vi.fn(),
  addRepo: vi.fn(),
  updateRepo: vi.fn(),
  removeRepo: vi.fn(),
  listRepos: vi.fn(),
  listWorktrees: vi.fn(),
  createWorktree: vi.fn(),
  deleteWorktree: vi.fn(),
  listBranches: vi.fn(),
  pushWorktree: vi.fn(),
  pullWorktree: vi.fn(),
  pullMainIntoWorktree: vi.fn(),
  switchBranch: vi.fn(),
  openInEditor: vi.fn(),
  openInTerminal: vi.fn(),
  revealInFinder: vi.fn(),
  pickDirectory: vi.fn(),
  minimizeWindow: vi.fn(),
  zoomWindow: vi.fn(),
  closeWindow: vi.fn(),
  setWindowSize: vi.fn(),
  onWindowFocusChange: vi.fn(() => () => {}),
  startCommand: vi.fn(),
  stopCommand: vi.fn(),
  listRunningCommands: vi.fn(),
  getCommandBuffer: vi.fn(),
  onCommandOutput: vi.fn(() => () => {}),
  onCommandExit: vi.fn(() => () => {}),
};

/** Reset every mock and install safe, empty defaults. Call in `beforeEach`. */
export function resetApiMock(): void {
  for (const value of Object.values(apiMock)) {
    if (typeof value === "function" && "mockReset" in value) value.mockReset();
  }
  apiMock.getConfig.mockResolvedValue({
    worktreesRoot: "/Users/test/worktrees",
    editorCommand: "code",
    repos: [],
  });
  apiMock.listRepos.mockResolvedValue([]);
  apiMock.listBranches.mockResolvedValue([]);
  apiMock.pickDirectory.mockResolvedValue(null);
  // Window-focus subscription must hand back an unsubscribe fn for React cleanup.
  apiMock.onWindowFocusChange.mockReturnValue(() => {});
  // No commands running by default; event subscriptions hand back unsubscribers.
  apiMock.listRunningCommands.mockResolvedValue([]);
  apiMock.getCommandBuffer.mockResolvedValue("");
  apiMock.onCommandOutput.mockReturnValue(() => {});
  apiMock.onCommandExit.mockReturnValue(() => {});
}
