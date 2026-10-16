// The branch picker's state: a filter over a worktree's branches, ranked
// by `fuzzyFilter`, with one row selected. Ported from `wtm-ui`'s picker.

import { fuzzyFilter, type Match } from "./fuzzy";

export interface PickerState {
  /// Every branch it offers: the current one first if the list lacks it.
  all: string[];
  current: string | null;
  query: string;
  rows: { branch: string; match: Match }[];
  selected: number | null;
}

export function openPicker(branches: readonly string[], current: string | null): PickerState {
  // The current branch stays choosable even if the list is stale.
  const all = current !== null && !branches.includes(current) ? [current, ...branches] : [...branches];
  const s = refilter({ all, current, query: "", rows: [], selected: null });
  // Start on the current branch, as a menu would.
  const at = s.rows.findIndex((r) => r.branch === current);
  return at >= 0 ? { ...s, selected: at } : s;
}

function refilter(s: PickerState): PickerState {
  const rows = fuzzyFilter(s.query, s.all).map(([i, match]) => ({ branch: s.all[i], match }));
  return { ...s, rows, selected: rows.length ? 0 : null };
}

export const setQuery = (s: PickerState, query: string): PickerState =>
  query === s.query ? s : refilter({ ...s, query });

/// Move the selection by `by` rows, stopping at either end.
export function moveBy(s: PickerState, by: number): PickerState {
  if (!s.rows.length) return s;
  const from = s.selected ?? -1;
  return { ...s, selected: Math.min(Math.max(from + by, 0), s.rows.length - 1) };
}

/// The branch to switch to for row `row`: none for the one already out.
export function choice(s: PickerState, row: number | null): string | null {
  const branch = row === null ? undefined : s.rows[row]?.branch;
  return branch === undefined || branch === s.current ? null : branch;
}
