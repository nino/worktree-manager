import { describe, expect, it } from "vitest";
import { makeNode, makeWorktree } from "./test/fixtures";
import { filterWorktrees, matchesWorktreeSearch, repoHasMatch } from "./worktreeSearch";

describe("matchesWorktreeSearch", () => {
  it("matches a substring of the branch, case-insensitively", () => {
    expect(matchesWorktreeSearch("LOGIN", "fix-login", "/x")).toBe(true);
    expect(matchesWorktreeSearch("nope", "fix-login", "/x")).toBe(false);
  });

  it("matches a substring of the path, case-insensitively", () => {
    expect(matchesWorktreeSearch("WORKTREES", "feature", "/Users/test/Worktrees/app")).toBe(true);
    expect(matchesWorktreeSearch("nope", "feature", "/Users/test/worktrees/app")).toBe(false);
  });

  it("matches when either field matches", () => {
    expect(matchesWorktreeSearch("feature", "feature", "/path/unrelated")).toBe(true);
    expect(matchesWorktreeSearch("path", "feature", "/path/unrelated")).toBe(true);
  });

  it("matches a null (detached) branch against what the row actually displays", () => {
    // WorktreeRow renders "(detached)" (with parens) for a null branch, so
    // both the substring and the exact displayed text must match.
    expect(matchesWorktreeSearch("detach", null, "/x")).toBe(true);
    expect(matchesWorktreeSearch("(detached)", null, "/x")).toBe(true);
    expect(matchesWorktreeSearch("feature", null, "/x")).toBe(false);
  });

  it("treats a blank or whitespace-only query as matching everything", () => {
    expect(matchesWorktreeSearch("", "anything", "/anywhere")).toBe(true);
    expect(matchesWorktreeSearch("   ", "anything", "/anywhere")).toBe(true);
  });

  it("matches on branch alone when path is omitted", () => {
    expect(matchesWorktreeSearch("feat", "feature")).toBe(true);
    expect(matchesWorktreeSearch("nope", "feature")).toBe(false);
  });
});

describe("filterWorktrees", () => {
  it("returns every worktree in original order for a blank query", () => {
    const worktrees = [makeWorktree({ branch: "b" }), makeWorktree({ branch: "a" })];
    expect(filterWorktrees("", worktrees)).toEqual(worktrees);
  });

  it("filters out non-matching worktrees", () => {
    const worktrees = [
      makeWorktree({ branch: "main", path: "/w/main" }),
      makeWorktree({ branch: "fix-login", path: "/w/fix-login" }),
    ];
    expect(filterWorktrees("login", worktrees)).toEqual([worktrees[1]]);
  });
});

describe("repoHasMatch", () => {
  it("is true when any worktree matches", () => {
    const node = makeNode({}, [makeWorktree({ branch: "fix-login" })]);
    expect(repoHasMatch("login", node)).toBe(true);
  });

  it("is false when no worktree matches", () => {
    const node = makeNode({}, [makeWorktree({ branch: "fix-login" })]);
    expect(repoHasMatch("nope", node)).toBe(false);
  });

  it("is false for a repo with no worktrees", () => {
    const node = makeNode({}, []);
    expect(repoHasMatch("anything", node)).toBe(false);
  });
});
