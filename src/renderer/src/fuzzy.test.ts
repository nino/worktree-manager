import { describe, expect, it } from "vitest";
import { fuzzyFilterBranches, fuzzyMatchBranch } from "./fuzzy";

describe("fuzzyMatchBranch", () => {
  it("matches a non-contiguous subsequence", () => {
    // "mn" -> m(0) … n(3) in "main"
    expect(fuzzyMatchBranch("mn", "main")).toMatchObject({ startIndex: 0, span: 3 });
  });

  it("is case-insensitive", () => {
    expect(fuzzyMatchBranch("MAIN", "main")).not.toBeNull();
    expect(fuzzyMatchBranch("main", "MAIN")).not.toBeNull();
  });

  it("returns null when the query is not a subsequence", () => {
    expect(fuzzyMatchBranch("xyz", "main")).toBeNull();
    // Right characters, wrong order.
    expect(fuzzyMatchBranch("nm", "main")).toBeNull();
  });

  it("treats an empty query as a zero-span match at position 0", () => {
    expect(fuzzyMatchBranch("", "anything")).toEqual({ startIndex: 0, span: 0, positions: [] });
  });

  it("records the leftmost positions of each matched character", () => {
    expect(fuzzyMatchBranch("ab", "aXb")?.positions).toEqual([0, 2]);
  });
});

describe("fuzzyFilterBranches", () => {
  it("returns every branch in original order for a blank query", () => {
    expect(fuzzyFilterBranches("", ["dev", "main", "release"])).toEqual(["dev", "main", "release"]);
    expect(fuzzyFilterBranches("   ", ["b", "a"])).toEqual(["b", "a"]);
  });

  it("filters out non-matching branches", () => {
    expect(fuzzyFilterBranches("m", ["release", "main", "dev-m"])).toEqual(["main", "dev-m"]);
  });

  it("ranks earlier matches before later ones", () => {
    // 'm' at index 0 in "main" beats index 4 in "dev-m".
    expect(fuzzyFilterBranches("m", ["dev-m", "main"])).toEqual(["main", "dev-m"]);
  });

  it("ranks tighter (more contiguous) matches ahead of looser ones", () => {
    // Both start at 0; "ab" is contiguous (span 1), "aXXb" is not (span 3).
    expect(fuzzyFilterBranches("ab", ["aXXb", "ab"])).toEqual(["ab", "aXXb"]);
  });

  it("prefers shorter names when start and span tie", () => {
    expect(fuzzyFilterBranches("feat", ["feature/long-name", "feat"])).toEqual([
      "feat",
      "feature/long-name",
    ]);
  });

  it("is a stable sort for fully-tied matches", () => {
    expect(fuzzyFilterBranches("feat", ["feat/a", "feat/b"])).toEqual(["feat/a", "feat/b"]);
  });

  it("matches the classic 'mn' -> 'main' example", () => {
    expect(fuzzyFilterBranches("mn", ["release", "main", "develop"])).toEqual(["main"]);
  });
});
