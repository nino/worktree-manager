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
  it("keeps quiet while a build is still downloading", async () => {
    apiMock.getUpdateStatus.mockResolvedValue({
      state: "downloading",
      currentVersion: "1.0.7",
      newVersion: "1.0.9",
      percent: 40,
    });
    const { user } = renderApp();

    // Prove the downloading status actually reached the renderer — the About
    // dialog is where anything short of "ready" is reported…
    await user.click(await screen.findByRole("button", { name: "About Worktree Manager" }));
    await screen.findByText(/Downloading version 1\.0\.9… 40%/);
    // …and that it offers no restart while the download is in flight.
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

  // The offer can appear seconds before Squirrel has taken the build, and a
  // restart asked for in that window is queued rather than immediate — so the
  // button has to read as busy instead of inviting another click.
  it("latches while the restart is queued", async () => {
    const { user } = renderApp();
    await screen.findByRole("button", { name: "Settings" });
    pushStatus({ state: "ready", currentVersion: "1.0.7", newVersion: "1.0.9" });

    const restart = await screen.findByRole("button", { name: /restart to update/i });
    await user.click(restart);

    const restarting = await screen.findByRole("button", { name: "Restarting…" });
    expect(restarting).toHaveProperty("disabled", true);
    await user.click(restarting);
    expect(apiMock.installUpdate).toHaveBeenCalledOnce();
  });

  it("shows the running version and checks on demand in the About dialog", async () => {
    apiMock.getUpdateStatus.mockResolvedValue({ state: "idle", currentVersion: "1.0.7" });
    const { user } = renderApp();

    await user.click(await screen.findByRole("button", { name: "About Worktree Manager" }));
    await screen.findByText(/Version 1\.0\.7/);
    expect(screen.getByText(/Not checked yet\./)).toBeTruthy();

    await user.click(screen.getByRole("button", { name: "Check now" }));
    expect(apiMock.checkForUpdates).toHaveBeenCalledOnce();
    // The outcome arrives as a pushed status, not as the call's return value.
    pushStatus({ state: "idle", currentVersion: "1.0.7", lastCheckedAt: 1_700_000_000_000 });
    await waitFor(() => expect(screen.getByText(/Up to date\./)).toBeTruthy());
  });
});
