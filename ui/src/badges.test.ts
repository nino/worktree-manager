import { describe, expect, it } from "vitest";

import { badgesFor } from "./badges";
import type { Status } from "./wire/Status";
import type { Worktree } from "./wire/Worktree";

const clean = (): Status => ({
  hasStaged: false,
  hasUnstaged: false,
  hasUntracked: false,
  trunkRef: "origin/main",
  ahead: 0,
  behind: 0,
  hasUpstream: true,
  unpushed: 0,
});

const wt = (w: Partial<Worktree> = {}): Worktree => ({
  path: "/w",
  displayPath: "/w",
  branch: "b",
  head: "abc",
  isMain: false,
  locked: false,
  prunable: false,
  busy: null,
  status: clean(),
  ...w,
});

const texts = (w: Worktree) => badgesFor(w, "main").map((b) => b.text);
const emphases = (w: Worktree) => badgesFor(w, "main").map((b) => b.emphasis);

describe("badgesFor", () => {
  it("shows a tick for a clean, in-sync worktree and spends no colour", () => {
    expect(texts(wt())).toEqual(["✓"]);
    expect(emphases(wt())).toEqual(["quiet"]);
  });

  it("keeps distance from the trunk quiet", () => {
    const w = wt({ status: { ...clean(), ahead: 2, behind: 1 } });
    expect(texts(w)).toEqual(["✓", "↑2 origin/main", "↓1 origin/main"]);
    expect(emphases(w).every((e) => e === "quiet")).toBe(true);
  });

  it("makes unpushed commits notable rather than coloured", () => {
    const w = wt({ status: { ...clean(), unpushed: 3 } });
    expect(badgesFor(w, "main").at(-1)).toMatchObject({ text: "⇡3 unpushed", emphasis: "notable" });
  });

  it("colours only uncommitted work and a missing folder", () => {
    const dirty = wt({ status: { ...clean(), hasStaged: true, hasUnstaged: true, hasUntracked: true } });
    expect(badgesFor(dirty, "main").filter((b) => b.emphasis === "attention").map((b) => b.text)).toEqual([
      "staged",
      "unstaged",
      "untracked",
    ]);
    expect(emphases(wt({ prunable: true }))).toEqual(["alarm"]);
  });

  it("stops at a missing folder, whatever the status", () => {
    expect(texts(wt({ prunable: true, isMain: true }))).toEqual(["primary", "folder missing"]);
  });

  it("says when the trunk could not be compared, naming the configured branch if git gave no ref", () => {
    const w = wt({ status: { ...clean(), trunkRef: "", ahead: null, behind: null } });
    expect(texts(w)).toContain("? main");
  });

  it("notes a branch with no upstream", () => {
    expect(texts(wt({ status: { ...clean(), hasUpstream: false } }))).toContain("no upstream");
  });

  it("says when there is no status at all", () => {
    expect(texts(wt({ status: null, locked: true }))).toEqual(["locked", "no status"]);
  });
});
