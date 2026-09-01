import { app } from "electron";
import { autoUpdater } from "electron-updater";
import type { UpdateStatus } from "@shared/types";

// MARK: Automatic updates
//
// Every push to `main` replaces the rolling `latest` GitHub release with a
// freshly signed and notarised build (see .github/workflows/release.yml), and
// stamps it with a version derived from main's commit count. electron-updater
// reads that release's `latest-mac.yml`, compares its version with this app's,
// and — when it is newer — downloads the .zip beside it in the background.
// Installing it is Squirrel.Mac's job and only happens on quit: either the user
// takes the "Restart to update" offer in the top bar, or the staged update is
// swapped in the next time they quit the app.

/** How often to ask GitHub for a newer build: every six hours. */
export const UPDATE_CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;

/** How long after launch the first check waits, to stay out of startup's way. */
export const FIRST_CHECK_DELAY_MS = 10_000;

/** Shown when there is no update feed to talk to (running from source). */
const UNSUPPORTED_MESSAGE = "Automatic updates are off in development builds.";

let status: UpdateStatus = { state: "idle", currentVersion: app.getVersion() };

let publish: ((status: UpdateStatus) => void) | null = null;
let timer: ReturnType<typeof setInterval> | null = null;
let firstCheck: ReturnType<typeof setTimeout> | null = null;
let wired = false;

/**
 * Only packaged builds have an update feed: `app-update.yml` is written into the
 * bundle by electron-builder, so a run from source has nothing to check against.
 */
function isSupported(): boolean {
  return app.isPackaged;
}

/** Merge a change into the status and push the result to the renderer. */
function setStatus(next: Partial<UpdateStatus>): void {
  status = { ...status, ...next };
  publish?.(status);
}

/** electron-updater rejects and emits with plain Errors; keep only the text. */
function reasonOf(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error);
  return message.trim() || "The update check failed.";
}

/** Subscribe to electron-updater's events, once per process. */
function wireEvents(): void {
  if (wired) return;
  wired = true;

  // Download as soon as something newer shows up: the point is that updating
  // costs the user nothing but a restart they choose when to take.
  autoUpdater.autoDownload = true;
  autoUpdater.autoInstallOnAppQuit = true;

  // Each transition clears what it invalidates: the fields are documented per
  // state (see UpdateStatus), so a stale percent or a stale error message must
  // not survive into a state that doesn't have one.
  autoUpdater.on("checking-for-update", () => setStatus({ state: "checking" }));

  autoUpdater.on("update-not-available", () => {
    setStatus({
      state: "idle",
      lastCheckedAt: Date.now(),
      newVersion: undefined,
      percent: undefined,
      message: undefined,
    });
  });

  // autoDownload is on, so an available update is already on its way down.
  autoUpdater.on("update-available", (info) => {
    setStatus({
      state: "downloading",
      newVersion: info.version,
      percent: 0,
      lastCheckedAt: Date.now(),
      message: undefined,
    });
  });

  autoUpdater.on("download-progress", (progress) => {
    setStatus({ state: "downloading", percent: Math.round(progress.percent) });
  });

  autoUpdater.on("update-downloaded", (info) => {
    setStatus({ state: "ready", newVersion: info.version, percent: 100, message: undefined });
  });

  // Never fatal: a failed check leaves the app exactly as it was, running the
  // version it already had.
  autoUpdater.on("error", (error) => {
    console.warn("auto-update:", error);
    // A staged build stays staged. Squirrel keeps reporting through this event
    // after the download is in hand, and dropping "ready" would retract the
    // restart offer for an update that still installs on the next quit.
    if (status.state === "ready") return;
    setStatus({ state: "error", message: reasonOf(error), percent: undefined });
  });
}

/** The updater's current state (starts out reflecting the running build). */
export function getUpdateStatus(): UpdateStatus {
  return status;
}

/**
 * Ask GitHub for a newer build now. Never throws — a failure lands in the
 * status as `state: "error"`, with the reason for the About dialog to show.
 * A check that finds an update leaves it downloading in the background.
 */
export async function checkForUpdates(): Promise<UpdateStatus> {
  if (!isSupported()) return status;
  // An update already in hand: re-checking would only race its own download.
  if (status.state === "downloading" || status.state === "ready") return status;

  wireEvents();
  try {
    const result = await autoUpdater.checkForUpdates();
    // With autoDownload on, the check hands back a download that is already in
    // flight. electron-updater re-throws its failures after emitting "error",
    // so the promise has to be claimed here or a failed download lands as an
    // unhandled rejection in the main process. The event already has the status.
    void result?.downloadPromise?.catch(() => {});
  } catch (error) {
    // The "error" event has usually fired already; this catches the rest.
    setStatus({ state: "error", message: reasonOf(error) });
  }
  return status;
}

/**
 * Quit and let Squirrel swap in the downloaded build. A no-op until one is
 * staged, so a stale renderer can't restart the app for nothing.
 *
 * "Ready" runs slightly ahead of Squirrel: electron-updater announces the
 * download before handing the zip over, and until that handover finishes this
 * call queues the restart instead of performing it (the app goes down when
 * Squirrel is done). The button latches into "Restarting…" for that window —
 * see `UpdateButton` — so the wait reads as progress rather than a dead click.
 */
export function installUpdate(): void {
  if (status.state !== "ready") return;
  autoUpdater.quitAndInstall();
}

/**
 * Check for updates shortly after launch and every UPDATE_CHECK_INTERVAL_MS
 * after that. `onStatus` fires on every state change so the renderer can offer
 * the restart once a build is downloaded.
 */
export function startAutoUpdate(onStatus: (status: UpdateStatus) => void): void {
  publish = onStatus;
  if (!isSupported()) {
    setStatus({ state: "unsupported", message: UNSUPPORTED_MESSAGE });
    return;
  }
  if (timer) return;

  wireEvents();
  firstCheck = setTimeout(() => {
    firstCheck = null;
    void checkForUpdates();
  }, FIRST_CHECK_DELAY_MS);
  timer = setInterval(() => void checkForUpdates(), UPDATE_CHECK_INTERVAL_MS);
}

/** Stop the periodic check (idempotent). Any staged update still installs on quit. */
export function stopAutoUpdate(): void {
  if (firstCheck) clearTimeout(firstCheck);
  if (timer) clearInterval(timer);
  firstCheck = null;
  timer = null;
}
