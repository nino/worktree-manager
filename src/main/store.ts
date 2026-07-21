import { homedir } from "node:os";
import { join, basename } from "node:path";
import { randomUUID } from "node:crypto";
import Store from "electron-store";
import type { AppConfig, AppSettings, RepoConfig } from "@shared/types";

const defaults: AppConfig = {
  worktreesRoot: join(homedir(), ".claude-worktrees"),
  editorCommand: "code",
  repos: [],
};

let _store: Store<AppConfig> | null = null;

/** Lazily create the electron-store instance (avoids side effects at import). */
function store(): Store<AppConfig> {
  if (!_store) _store = new Store<AppConfig>({ name: "worktree-manager", defaults });
  return _store;
}

/**
 * Migrate a persisted repo forward so older configs (written before per-repo
 * commands existed) don't break: a missing/invalid `commands` becomes `[]`.
 */
function normalizeRepo(repo: RepoConfig): RepoConfig {
  // Drop malformed entries (e.g. a hand-edited/corrupted store) so downstream
  // code can trust every command has string id/name/command fields.
  const commands = Array.isArray(repo.commands)
    ? repo.commands.filter(
        (c) =>
          c != null &&
          typeof c.id === "string" &&
          typeof c.name === "string" &&
          typeof c.command === "string",
      )
    : [];
  return { ...repo, commands };
}

/** Return the full persisted configuration. */
export function getConfig(): AppConfig {
  return {
    worktreesRoot: store().get("worktreesRoot"),
    editorCommand: store().get("editorCommand"),
    repos: store().get("repos").map(normalizeRepo),
  };
}

/** Update the app-wide settings (worktrees root, editor). */
export function setAppSettings(settings: AppSettings): AppConfig {
  store().set("worktreesRoot", settings.worktreesRoot);
  store().set("editorCommand", settings.editorCommand);
  return getConfig();
}

/**
 * Repo display names double as a directory segment under the worktrees root,
 * so they must never contain path separators or traversal sequences.
 */
export function sanitizeRepoName(name: string): string {
  const cleaned = name.replace(/[/\\]/g, "-").replace(/\.\./g, "-").trim().replace(/^\.+/, "");
  return cleaned || "repo";
}

/** Look up a repo by id, or throw if it is not configured. */
export function getRepo(repoId: string): RepoConfig {
  const repo = store()
    .get("repos")
    .find((r) => r.id === repoId);
  if (!repo) throw new Error(`Unknown repo: ${repoId}`);
  return normalizeRepo(repo);
}

/** Add a repository. Refuses duplicates (by resolved path). */
export function addRepo(repo: {
  path: string;
  mainBranch: string;
  name?: string;
  initCommand?: string;
}): AppConfig {
  const repos = store().get("repos");
  if (repos.some((r) => r.path === repo.path)) {
    throw new Error(`Repository already added: ${repo.path}`);
  }
  const entry: RepoConfig = {
    id: randomUUID(),
    name: sanitizeRepoName(repo.name ?? basename(repo.path)),
    path: repo.path,
    mainBranch: repo.mainBranch,
    initCommand: repo.initCommand ?? "",
    commands: [],
  };
  store().set("repos", [...repos, entry]);
  return getConfig();
}

/** Replace an existing repo's configuration. */
export function updateRepo(updated: RepoConfig): AppConfig {
  const repos = store().get("repos");
  const idx = repos.findIndex((r) => r.id === updated.id);
  if (idx === -1) throw new Error(`Unknown repo: ${updated.id}`);
  const next = [...repos];
  next[idx] = normalizeRepo({
    ...updated,
    // Path and id are immutable via updates; name must stay a safe segment.
    id: repos[idx].id,
    path: repos[idx].path,
    name: sanitizeRepoName(updated.name),
  });
  store().set("repos", next);
  return getConfig();
}

/** Remove a repo from the configuration (does not touch the filesystem). */
export function removeRepo(repoId: string): AppConfig {
  store().set(
    "repos",
    store()
      .get("repos")
      .filter((r) => r.id !== repoId),
  );
  return getConfig();
}
