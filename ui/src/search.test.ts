import { describe, expect, it } from "vitest";

import { demoModel } from "./mock";
import { children, matches, normalise, repoMeta, showsRepo } from "./search";

const app = () => demoModel().repos[0];
const site = () => demoModel().repos[1];

describe("search", () => {
  it("normalises the query", () => {
    expect(normalise("  Fix ")).toBe("fix");
  });

  it("matches the branch or the path as shown, not the home folder", () => {
    const w = app().worktrees[2];
    expect(matches("fix", w)).toBe(true);
    expect(matches("worktrees/app", w)).toBe(true);
    expect(matches("demo", w)).toBe(false);
  });

  it("lists the primary tree first, then by folder, with creations in place", () => {
    const rows = children(site(), "");
    expect(rows.map((r) => (r.kind === "worktree" ? r.worktree.branch : r.pending.branch))).toEqual([
      "main",
      "docs/new-page",
      "old-experiment",
    ]);
  });

  it("counts what a search shows", () => {
    expect(repoMeta(app(), "")).toBe("main · 3 worktrees");
    expect(repoMeta(app(), "fix")).toBe("main · 1 of 3 worktrees");
  });

  it("hides a repo with no match, unless its listing failed", () => {
    expect(showsRepo(app(), "nothing-like-this")).toBe(false);
    expect(showsRepo({ ...app(), error: "gone" }, "nothing-like-this")).toBe(true);
  });
});
