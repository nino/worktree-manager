/** Abbreviate the user's home directory to `~` for display purposes. */
export function tildify(path: string, home: string): string {
  if (!home) return path;
  if (path === home) return "~";
  if (path.startsWith(home + "/")) return "~" + path.slice(home.length);
  return path;
}

/**
 * Turn a branch name into a filesystem-safe directory segment. Shared by the
 * main process (which builds the on-disk worktree path) and the renderer (which
 * predicts where a still-being-created worktree will sort in the tree, so the
 * "Creating…" placeholder appears at the row's final position — see
 * `worktreePathFor` and git's alphabetical worktree ordering).
 */
export function slugifyBranch(branch: string): string {
  return (
    branch
      .replace(/[/\\]/g, "-")
      .replace(/[^a-zA-Z0-9._-]/g, "-")
      .replace(/-+/g, "-")
      .replace(/^-|-$/g, "") || "worktree"
  );
}
