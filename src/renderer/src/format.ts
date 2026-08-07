import type { AddReposResult } from "@shared/types";
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
