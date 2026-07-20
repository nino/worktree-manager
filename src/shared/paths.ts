/** Abbreviate the user's home directory to `~` for display purposes. */
export function tildify(path: string, home: string): string {
  if (!home) return path;
  if (path === home) return "~";
  if (path.startsWith(home + "/")) return "~" + path.slice(home.length);
  return path;
}
