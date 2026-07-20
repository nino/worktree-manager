import { describe, expect, it } from "vitest";
import { parseLeftRightCount, parseStatusPorcelainV2, parseWorktreePorcelain } from "./git";

describe("parseWorktreePorcelain", () => {
  it("parses multiple worktrees with branches and detached heads", () => {
    const out = [
      "worktree /repo/main",
      "HEAD 1111111111111111111111111111111111111111",
      "branch refs/heads/main",
      "",
      "worktree /wt/feature",
      "HEAD 2222222222222222222222222222222222222222",
      "branch refs/heads/feature/foo",
      "",
      "worktree /wt/detached",
      "HEAD 3333333333333333333333333333333333333333",
      "detached",
      "",
    ].join("\n");

    const result = parseWorktreePorcelain(out);
    expect(result).toHaveLength(3);
    expect(result[0]).toMatchObject({ path: "/repo/main", branch: "main", bare: false });
    expect(result[1]).toMatchObject({ path: "/wt/feature", branch: "feature/foo" });
    expect(result[2]).toMatchObject({ path: "/wt/detached", branch: null, detached: true });
  });

  it("captures the prunable flag for worktrees whose folder was deleted", () => {
    const out = [
      "worktree /wt/gone",
      "HEAD 5555555555555555555555555555555555555555",
      "branch refs/heads/gone",
      "prunable gitdir file points to non-existent location",
      "",
    ].join("\n");

    const result = parseWorktreePorcelain(out);
    expect(result[0].prunable).toBe(true);
  });

  it("captures the locked flag and skips bare entries via caller filter", () => {
    const out = [
      "worktree /bare",
      "HEAD 0000000000000000000000000000000000000000",
      "bare",
      "",
      "worktree /wt/locked",
      "HEAD 4444444444444444444444444444444444444444",
      "branch refs/heads/x",
      "locked reason here",
      "",
    ].join("\n");

    const result = parseWorktreePorcelain(out);
    expect(result[0].bare).toBe(true);
    expect(result[1].locked).toBe(true);
  });
});

describe("parseStatusPorcelainV2", () => {
  it("reads branch, upstream and ahead/behind headers", () => {
    const out = [
      "# branch.oid abc123",
      "# branch.head feature",
      "# branch.upstream origin/feature",
      "# branch.ab +2 -1",
    ].join("\n");

    const s = parseStatusPorcelainV2(out);
    expect(s.head).toBe("feature");
    expect(s.detached).toBe(false);
    expect(s.upstream).toBe("origin/feature");
    expect(s.aheadUpstream).toBe(2);
    expect(s.behindUpstream).toBe(1);
  });

  it("detects staged, unstaged and untracked changes", () => {
    const out = [
      "# branch.head main",
      "1 M. N... 100644 100644 100644 aaa bbb staged-only.txt",
      "1 .M N... 100644 100644 100644 ccc ddd unstaged-only.txt",
      "1 MM N... 100644 100644 100644 eee fff both.txt",
      "? untracked.txt",
    ].join("\n");

    const s = parseStatusPorcelainV2(out);
    expect(s.hasStaged).toBe(true);
    expect(s.hasUnstaged).toBe(true);
    expect(s.hasUntracked).toBe(true);
  });

  it("marks a clean tree with no change flags", () => {
    const out = ["# branch.head main", "# branch.oid abc"].join("\n");
    const s = parseStatusPorcelainV2(out);
    expect(s.hasStaged).toBe(false);
    expect(s.hasUnstaged).toBe(false);
    expect(s.hasUntracked).toBe(false);
    expect(s.upstream).toBeNull();
  });

  it("treats unmerged entries as unstaged", () => {
    const out = ["# branch.head main", "u UU N... 1 1 1 1 h1 h2 h3 conflict.txt"].join("\n");
    const s = parseStatusPorcelainV2(out);
    expect(s.hasUnstaged).toBe(true);
  });

  it("recognizes a detached head", () => {
    const s = parseStatusPorcelainV2("# branch.head (detached)");
    expect(s.detached).toBe(true);
    expect(s.head).toBeNull();
  });
});

describe("parseLeftRightCount", () => {
  it("maps left/right to behind/ahead", () => {
    expect(parseLeftRightCount("3\t5\n")).toEqual({ behind: 3, ahead: 5 });
    expect(parseLeftRightCount("0 0")).toEqual({ behind: 0, ahead: 0 });
  });

  it("is resilient to garbage", () => {
    expect(parseLeftRightCount("")).toEqual({ behind: 0, ahead: 0 });
  });
});
