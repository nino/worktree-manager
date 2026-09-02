import { beforeEach, describe, expect, it } from "vitest";
import { screen } from "@testing-library/react";
import { apiMock, resetApiMock } from "./apiMock";
import { renderApp } from "./renderApp";
import { makeNode, makeWorktree } from "./fixtures";

const WT = "/Users/test/worktrees/app/main";

beforeEach(() => resetApiMock());

describe("command launcher", () => {
  it("stays out of the row when the repo has no commands", async () => {
    apiMock.listRepos.mockResolvedValue([
      makeNode({ commands: [] }, [makeWorktree({ branch: "main", isMain: true, path: WT })]),
    ]);

    renderApp();
    await screen.findByText("app");

    expect(screen.queryByRole("combobox", { name: "Run a command" })).toBeNull();
    // The wrapper span goes too, so the row's action gap doesn't keep a hole.
    expect(document.querySelector(".cmd-runner")).toBeNull();
  });

  it("shows the launcher once a command is configured", async () => {
    apiMock.listRepos.mockResolvedValue([
      makeNode({ commands: [{ id: "c1", name: "dev", command: "pnpm dev" }] }, [
        makeWorktree({ branch: "main", isMain: true, path: WT }),
      ]),
    ]);

    renderApp();
    await screen.findByText("app");

    expect(await screen.findByRole("combobox", { name: "Run a command" })).toBeTruthy();
  });
});
