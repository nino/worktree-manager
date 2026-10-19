import { describe, expect, it } from "vitest";

import { demoModel } from "./mock";
import { confirmDelete, forceDelete } from "./questions";

const [app, site] = demoModel().repos;

describe("delete questions", () => {
  it("names the branch, warns about changes and carries what the row showed", () => {
    const q = confirmDelete(app, "/home/demo/worktrees/app/claude-tauri-rewrite")!;
    expect(q.title).toBe("Delete worktree “claude/tauri-rewrite”?");
    expect(q.message).toBe("It has uncommitted changes. ~/worktrees/app/claude-tauri-rewrite");
    expect(q.request).toEqual({
      repoId: "app",
      path: "/home/demo/worktrees/app/claude-tauri-rewrite",
      expectedBranch: "claude/tauri-rewrite",
      force: false,
    });
  });

  it("says a missing folder is only bookkeeping", () => {
    const q = confirmDelete(site, "/home/demo/worktrees/site/old-experiment")!;
    expect(q.message).toMatch(/^The folder is already gone/);
  });

  it("never offers the primary tree", () => {
    expect(confirmDelete(app, "/home/demo/src/app")).toBeNull();
    expect(confirmDelete(app, "/nowhere")).toBeNull();
  });

  it("asks a second time before discarding changes", () => {
    const first = confirmDelete(app, "/home/demo/worktrees/app/fix-login")!;
    const second = forceDelete(first.request);
    expect(second.critical).toBe(true);
    expect(second.request.force).toBe(true);
    expect(second.title).toBe("“fix-login” has uncommitted changes");
  });
});
