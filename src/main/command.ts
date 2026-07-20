/** Pure helpers for building shell commands (kept electron-free for testing). */

/** Quote a string for safe use inside a POSIX shell command. */
export function shellQuote(value: string): string {
  return `'${value.replace(/'/g, `'\\''`)}'`;
}

/**
 * Combine a configured command with a target path.
 *
 * If the command contains `{path}` placeholders, each is replaced with the
 * quoted path (e.g. `open -na Ghostty --args --working-directory={path}`).
 * Otherwise the quoted path is appended as a final argument (e.g. `code`).
 */
export function buildCommand(command: string, targetPath: string): string {
  const quoted = shellQuote(targetPath);
  if (command.includes("{path}")) {
    return command.replaceAll("{path}", quoted);
  }
  return `${command} ${quoted}`;
}
