import { describe, expect, it } from "vitest";
import { slugifyBranch, worktreePathFor } from "./worktrees";

describe("slugifyBranch", () => {
  it("replaces slashes with dashes", () => {
    expect(slugifyBranch("feature/foo")).toBe("feature-foo");
    expect(slugifyBranch("release/v1.2.3")).toBe("release-v1.2.3");
  });

  it("strips unusual characters and collapses dashes", () => {
    expect(slugifyBranch("feat/@weird  name!")).toBe("feat-weird-name");
  });

  it("falls back to a default for empty results", () => {
    expect(slugifyBranch("///")).toBe("worktree");
  });
});

describe("worktreePathFor", () => {
  it("nests under <root>/<repoName>/<branch-slug>", () => {
    expect(worktreePathFor("/home/me/.claude-worktrees", "myrepo", "feature/x")).toBe(
      "/home/me/.claude-worktrees/myrepo/feature-x",
    );
  });
});
