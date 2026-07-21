import { beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor } from "@testing-library/react";
import { apiMock, resetApiMock } from "./apiMock";
import { renderApp } from "./renderApp";
import { deferred, makeNode, makeWorktree } from "./fixtures";

// xterm.js needs a real browser; stub it so the drawer can mount under happy-dom.
vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    loadAddon() {}
    open() {}
    write() {}
    dispose() {}
  },
}));
vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    fit() {}
  },
}));

const WT = "/Users/test/worktrees/app/main";

beforeEach(() => resetApiMock());

describe("terminal drawer", () => {
  // Regression for a leak: the terminal view registered its IPC listeners
  // before `await getCommandBuffer`, but only wired up teardown afterwards — so
  // closing the drawer mid-load left the listeners (and terminal) dangling.
  it("disposes the terminal and IPC listeners when closed mid-load", async () => {
    const offOutput = vi.fn();
    const offExit = vi.fn();
    apiMock.onCommandOutput.mockReturnValue(offOutput);
    apiMock.onCommandExit.mockReturnValue(offExit);
    // Keep the backlog fetch pending so we close during the load window.
    const buf = deferred<string>();
    apiMock.getCommandBuffer.mockReturnValue(buf.promise);
    apiMock.listRunningCommands.mockResolvedValue([
      {
        repoId: "r1",
        worktreePath: WT,
        commandId: "c1",
        name: "dev",
        command: "pnpm dev",
        startedAt: 0,
      },
    ]);
    apiMock.listRepos.mockResolvedValue([
      makeNode({ commands: [{ id: "c1", name: "dev", command: "pnpm dev" }] }, [
        makeWorktree({ branch: "main", isMain: true, path: WT }),
      ]),
    ]);

    const { user } = renderApp();
    await screen.findByText("app");

    // Open the drawer via the running badge (labelled with the command name).
    await user.click(await screen.findByRole("button", { name: "dev" }));
    // The terminal view mounts and registers its live subscriptions.
    await waitFor(() => expect(apiMock.onCommandOutput).toHaveBeenCalled());

    // Close the drawer while the backlog fetch is still in flight.
    await user.click(screen.getByRole("button", { name: "Close terminal" }));

    // Both subscriptions must be torn down (the bug left them attached).
    await waitFor(() => {
      expect(offOutput).toHaveBeenCalledTimes(1);
      expect(offExit).toHaveBeenCalledTimes(1);
    });
  });
});
