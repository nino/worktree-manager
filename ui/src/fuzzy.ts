// Fuzzy (subsequence) matching for the branch picker. Ported from
// `wtm_core::fuzzy`: the same greedy match and the same ranking.

export interface Match {
  /// Index of the first matched character.
  start: number;
  /// Distance between the first and last matched characters.
  span: number;
  /// Index into the candidate (in code points) of each matched character.
  positions: number[];
}

/// Greedy, case-insensitive subsequence match: the leftmost position of each
/// query character in `candidate`, or `null` when the query is not a
/// subsequence. An empty query matches everything at position 0.
export function fuzzyMatch(query: string, candidate: string): Match | null {
  // Lowercasing can turn one character into two (`İ` is `i` plus a
  // combining dot), so each lowercased character keeps the index of the
  // character it came from.
  const chars: [number, string][] = [];
  [...candidate].forEach((c, i) => {
    for (const l of c.toLowerCase()) chars.push([i, l]);
  });
  const positions: number[] = [];
  let at = 0;
  for (const qc of query.toLowerCase()) {
    while (at < chars.length && chars[at][1] !== qc) at++;
    if (at === chars.length) return null;
    const i = chars[at][0];
    at++;
    if (positions[positions.length - 1] !== i) positions.push(i);
  }
  const start = positions[0] ?? 0;
  const span = positions.length ? positions[positions.length - 1] - start : 0;
  return { start, span, positions };
}

/// Indices into `candidates` of those matching `query`, best first: earlier
/// matches, then tighter ones, then shorter names, then the original order.
/// A blank query keeps every candidate in its original order.
export function fuzzyFilter(query: string, candidates: readonly string[]): [number, Match][] {
  const q = query.trim();
  const scored: [number, Match][] = [];
  candidates.forEach((c, i) => {
    const m = fuzzyMatch(q, c);
    if (m) scored.push([i, m]);
  });
  if (!q) return scored;
  return scored.sort(
    ([ia, a], [ib, b]) =>
      a.start - b.start ||
      a.span - b.span ||
      // Rust compares byte lengths.
      utf8Length(candidates[ia]) - utf8Length(candidates[ib]) ||
      ia - ib,
  );
}

const encoder = new TextEncoder();
const utf8Length = (s: string) => encoder.encode(s).length;
