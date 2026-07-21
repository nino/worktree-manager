// MARK: Fuzzy branch filtering

/** Details of a successful fuzzy (subsequence) match, used for ranking. */
export interface FuzzyMatch {
  /** Index in the target of the first matched character (lower = earlier). */
  startIndex: number;
  /** Distance between the first and last matched characters (lower = tighter). */
  span: number;
  /** Indices in the target of each matched query character, in order. */
  positions: number[];
}

/**
 * Greedy, case-insensitive subsequence match: returns the leftmost positions
 * of each query character within `branch`, or `null` when `query` is not a
 * subsequence of `branch`. An empty query matches at position 0 with zero span.
 */
export function fuzzyMatchBranch(query: string, branch: string): FuzzyMatch | null {
  const q = query.toLowerCase();
  const b = branch.toLowerCase();
  if (q.length === 0) return { startIndex: 0, span: 0, positions: [] };

  const positions: number[] = [];
  let bi = 0;
  for (const qc of q) {
    let found = -1;
    while (bi < b.length) {
      const ch = b[bi];
      bi++;
      if (ch === qc) {
        found = bi - 1;
        break;
      }
    }
    if (found === -1) return null;
    positions.push(found);
  }

  const startIndex = positions[0];
  const span = positions[positions.length - 1] - startIndex;
  return { startIndex, span, positions };
}

/**
 * Filter `branches` to those fuzzy-matching `query`, ranked best-first:
 * matches that start earlier come first, then tighter (more contiguous)
 * matches, then shorter names, and finally the original order as a stable
 * tie-breaker. A blank query returns every branch in its original order.
 */
export function fuzzyFilterBranches(query: string, branches: string[]): string[] {
  const trimmed = query.trim();
  if (trimmed.length === 0) return [...branches];

  const scored = branches
    .map((branch, index) => ({ branch, index, match: fuzzyMatchBranch(trimmed, branch) }))
    .filter(
      (entry): entry is { branch: string; index: number; match: FuzzyMatch } =>
        entry.match !== null,
    );

  scored.sort((a, b) => {
    if (a.match.startIndex !== b.match.startIndex) return a.match.startIndex - b.match.startIndex;
    if (a.match.span !== b.match.span) return a.match.span - b.match.span;
    if (a.branch.length !== b.branch.length) return a.branch.length - b.branch.length;
    return a.index - b.index;
  });

  return scored.map((entry) => entry.branch);
}
