/**
 * Minimal ANSI/terminal-escape stripping for the integrated terminal. Rendering
 * a fully interactive TUI is a non-goal; this just removes the escape sequences
 * a pipe-based stream commonly carries so the scrollback stays readable.
 *
 * The pattern is built from `\u` escapes (rather than a literal regex) so no raw
 * control characters live in the source. It matches, in order:
 *  - CSI sequences (colours, cursor moves): ESC [ params intermediates final
 *  - OSC sequences (e.g. window titles): ESC ] ... terminated by BEL or ESC \
 *  - lone two-character escapes: ESC <byte>
 */
const ANSI_PATTERN = new RegExp(
  [
    "\\u001B\\[[0-9;?]*[ -/]*[@-~]",
    "\\u001B\\][^\\u0007\\u001B]*(?:\\u0007|\\u001B\\\\)",
    "\\u001B[@-Z\\\\-_]",
  ].join("|"),
  "g",
);

/** Remove ANSI escape sequences from a string. */
export function stripAnsi(input: string): string {
  return input.replace(ANSI_PATTERN, "");
}
