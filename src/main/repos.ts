import { stat } from "node:fs/promises";
import { basename, dirname } from "node:path";
import type { AddRepoFailure, AddReposResult } from "@shared/types";
import { detectMainBranch, resolveRepoRoot } from "./git";
import * as store from "./store";

// MARK: Failure messages

/**
 * Turn whatever `resolveRepoRoot`/`detectMainBranch` threw into one short line
 * for the UI. git's own text is kept for anything unrecognised, minus the
 * `git <args> failed:` prefix `runGit` adds and any trailing hint lines.
 */
export function describeAddFailure(error: unknown): string {
  const raw = error instanceof Error ? error.message : String(error);
  if (/not a git repository/i.test(raw)) return "Not a git repository";
  if (/ENOENT|ENOTDIR/.test(raw)) return "Folder not found";
  const withoutPrefix = raw.replace(/^git .*? failed: /, "");
  const firstLine = withoutPrefix.split("\n")[0]!.trim();
  return firstLine.replace(/^fatal: /, "") || "Could not add this folder";
}

/**
 * git needs a directory to run in, and a Dock drop can hand us a file (dragging
 * a repo's README, say). Fall back to the containing folder; an unreadable path
 * is passed through so git reports it.
 */
async function containingDirectory(path: string): Promise<string> {
  try {
    return (await stat(path)).isDirectory() ? path : dirname(path);
  } catch {
    return path;
  }
}

// MARK: Adding

/**
 * Add repositories by path — used by both the picker and Dock drops.
 *
 * Best effort per path: each is resolved to its repository root, gets its main
 * branch auto-detected, and is appended to the store. A path that isn't a repo
 * (or is already configured) lands in `failed` instead of aborting the batch.
 * Sequential on purpose: `store.addRepo` is read-modify-write, and duplicates
 * within one batch must be caught.
 */
export async function addRepos(paths: string[]): Promise<AddReposResult> {
  const added: string[] = [];
  const failed: AddRepoFailure[] = [];

  for (const path of paths) {
    try {
      const root = await resolveRepoRoot(await containingDirectory(path));
      if (store.getConfig().repos.some((r) => r.path === root)) {
        failed.push({ path, message: "Already added" });
        continue;
      }
      const mainBranch = await detectMainBranch(root);
      const config = store.addRepo({ path: root, mainBranch });
      // Report the stored (sanitized) display name, not the raw dropped path.
      added.push(config.repos.find((r) => r.path === root)?.name ?? basename(root));
    } catch (error) {
      failed.push({ path, message: describeAddFailure(error) });
    }
  }

  return { config: store.getConfig(), added, failed };
}
