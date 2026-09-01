import { beforeEach, describe, expect, it } from "vitest";
import { screen, waitFor } from "@testing-library/react";
import type { UpdateStatus } from "@shared/types";
import { apiMock, resetApiMock } from "./apiMock";
import { renderApp } from "./renderApp";

/** Hand the renderer a status the way the main process pushes them. */
function pushStatus(status: UpdateStatus): void {
  for (const [listener] of apiMock.onUpdateStatus.mock.calls) listener(status);
}

beforeEach(() => resetApiMock());

describe("automatic updates", () => {
  it("keeps quiet until a build has been downloaded", async () => {
    apiMock.getUpdateStatus.mockResolvedValue({
      state: "downloading",
      currentVersion: "1.0.7",
      newVersion: "1.0.9",
      percent: 40,
    });
    renderApp();

    await screen.findByRole("button", { name: "Settings" });
    expect(screen.queryByRole("button", { name: /restart to update/i })).toBeNull();
  });

  it("offers the restart once the main process reports one ready", async () => {
    const { user } = renderApp();
    await screen.findByRole("button", { name: "Settings" });
    expect(screen.queryByRole("button", { name: /restart to update/i })).toBeNull();

    // The updater finishes downloading while the window is open.
    pushStatus({ state: "ready", currentVersion: "1.0.7", newVersion: "1.0.9" });

    const restart = await screen.findByRole("button", { name: /restart to update/i });
    expect(restart.title).toContain("1.0.9");
    await user.click(restart);
    expect(apiMock.installUpdate).toHaveBeenCalledOnce();
  });

  it("shows the running version and checks on demand in the About dialog", async () => {
    apiMock.getUpdateStatus.mockResolvedValue({ state: "idle", currentVersion: "1.0.7" });
    apiMock.checkForUpdates.mockResolvedValue({
      state: "idle",
      currentVersion: "1.0.7",
      lastCheckedAt: 1_700_000_000_000,
    });
    const { user } = renderApp();

    await user.click(await screen.findByRole("button", { name: "About Worktree Manager" }));
    await screen.findByText(/Version 1\.0\.7/);
    expect(screen.getByText(/Not checked yet\./)).toBeTruthy();

    await user.click(screen.getByRole("button", { name: "Check now" }));
    await waitFor(() => expect(screen.getByText(/Up to date\./)).toBeTruthy());
  });
});
