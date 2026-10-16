import { describe, expect, it } from "vitest";

import { fuzzyFilter, fuzzyMatch } from "./fuzzy";

// The same cases as `wtm_core::fuzzy`'s tests.
describe("fuzzy", () => {
  it("matches subsequences case-insensitively", () => {
    const m = fuzzyMatch("fl", "Feature/Login")!;
    expect(m.positions).toEqual([0, 8]);
    expect([m.start, m.span]).toEqual([0, 8]);
    expect(fuzzyMatch("xyz", "main")).toBeNull();
    expect(fuzzyMatch("", "main")!.positions).toEqual([]);
  });

  it("indexes the candidate as written", () => {
    expect(fuzzyMatch("A", "İ\ta")!.positions).toEqual([2]);
    expect(fuzzyMatch("i̇", "İx")!.positions).toEqual([0]);
  });

  it("ranks earlier, tighter, shorter, then original order", () => {
    const branches = ["main", "feature/login", "fix/login", "login", "claude/fix-login"];
    const ranked = fuzzyFilter("login", branches).map(([i]) => branches[i]);
    expect(ranked).toEqual(["login", "claude/fix-login", "fix/login", "feature/login"]);
  });

  it("keeps the order for a blank query", () => {
    expect(fuzzyFilter("  ", ["b", "a"]).map(([i]) => i)).toEqual([0, 1]);
  });
});
