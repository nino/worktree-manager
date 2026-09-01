import type { AddReposResult, UpdateStatus } from "@shared/types";
import { tildify } from "@shared/paths";
import { api } from "./api";

/** Format a path for display, abbreviating the home directory to `~`. */
export function displayPath(path: string): string {
  return tildify(path, api.home);
}

/** A one-line message shown in the banner under the top bar. */
export interface Notice {
  tone: "info" | "error";
  text: string;
}

/**
 * Describe the outcome of adding repositories (picker or Dock drop) in one
 * line. Any rejected path makes it an error notice — those stay on screen —
 * while a clean run is informational and fades once the tree shows the repos.
 */
export function summarizeAddResult(result: AddReposResult): Notice | null {
  const { added, failed } = result;
  const parts: string[] = [];
  if (added.length > 0) parts.push(`Added ${added.join(", ")}`);
  for (const failure of failed) parts.push(`${displayPath(failure.path)}: ${failure.message}`);
  if (parts.length === 0) return null;
  return { tone: failed.length > 0 ? "error" : "info", text: parts.join(" · ") };
}

/**
 * One line describing where the auto-updater stands, for the About dialog.
 * The top bar only ever surfaces the "ready" state (as a restart offer), so
 * this is where a failed check or a download in flight becomes visible.
 */
export function describeUpdate(status: UpdateStatus): string {
  switch (status.state) {
    case "unsupported":
      return status.message ?? "Automatic updates are unavailable.";
    case "checking":
      return "Checking for updates…";
    case "downloading": {
      const what = status.newVersion ? `version ${status.newVersion}` : "an update";
      return status.percent === undefined
        ? `Downloading ${what}…`
        : `Downloading ${what}… ${status.percent}%`;
    }
    case "ready":
      return status.newVersion
        ? `Version ${status.newVersion} is ready — restart to install it.`
        : "An update is ready — restart to install it.";
    case "error":
      return `Last check failed: ${status.message ?? "unknown error"}`;
    case "idle":
      return status.lastCheckedAt ? "Up to date." : "Not checked yet.";
  }
}
