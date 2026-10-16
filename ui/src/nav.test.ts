import { describe, expect, it } from "vitest";

import { demoModel } from "./mock";
import { navigate, navRows, repoKey, worktreeKey } from "./nav";

const repos = demoModel().repos;
const allOpen = () => true;

describe("nav", () => {
  it("lists headers and worktrees of open cards, skipping creations", () => {
    const rows = navRows(repos, "", (id) => id !== "site");
    expect(rows.map((r) => r.key)).toEqual([
      "repo:app",
      "wt:/home/demo/src/app",
      "wt:/home/demo/worktrees/app/claude-tauri-rewrite",
      "wt:/home/demo/worktrees/app/fix-login",
      "repo:site",
    ]);
  });

  it("moves up and down, stopping at the ends", () => {
    const rows = navRows(repos, "", allOpen);
    expect(navigate(rows, null, "down", allOpen)).toEqual({ select: "repo:app" });
    expect(navigate(rows, null, "up", allOpen)).toEqual({
      select: worktreeKey("/home/demo/worktrees/site/old-experiment"),
    });
    expect(navigate(rows, repoKey("app"), "up", allOpen)).toEqual({ select: "repo:app" });
    expect(navigate(rows, repoKey("app"), "down", allOpen)).toEqual({
      select: worktreeKey("/home/demo/src/app"),
    });
  });

  it("walks the tree with left and right as an outline does", () => {
    const rows = navRows(repos, "", allOpen);
    const wt = worktreeKey("/home/demo/worktrees/app/fix-login");
    expect(navigate(rows, wt, "left", allOpen)).toEqual({ select: "repo:app" });
    expect(navigate(rows, repoKey("app"), "left", allOpen)).toEqual({ close: "app" });
    expect(navigate(rows, repoKey("app"), "right", () => false)).toEqual({ open: "app" });
    expect(navigate(rows, repoKey("app"), "right", allOpen)).toEqual({
      select: worktreeKey("/home/demo/src/app"),
    });
    expect(navigate(rows, wt, "right", allOpen)).toBeNull();
  });
});
