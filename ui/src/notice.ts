// The notice bar's text. Ported from `wtm-ui`'s list module.

/// The first two lines, with a count of the rest. Git's file lists can run
/// to hundreds of lines and must never size the window.
export function noticeSummary(text: string): string {
  const lines = text.split("\n");
  if (lines.length <= 2) return text;
  return `${lines[0].trimEnd()}\n${lines[1].trim()} … (${lines.length - 2} more lines)`;
}

/// A notice too long for the bar, which offers "Details…".
export const noticeIsLong = (text: string) => text.split("\n").length > 3 || text.length > 240;
