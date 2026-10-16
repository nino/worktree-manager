import { cleanup, fireEvent, render, screen } from "@solidjs/testing-library";
import { afterEach, describe, expect, it, vi } from "vitest";

import { App } from "./App";
import { mockBackend } from "./mock";

afterEach(cleanup);

function setup() {
  const backend = mockBackend();
  const spies = {
    push: vi.spyOn(backend, "push"),
    openInTerminal: vi.spyOn(backend, "openInTerminal"),
    refresh: vi.spyOn(backend, "refresh"),
    switchBranch: vi.spyOn(backend, "switchBranch"),
    deleteWorktree: vi.spyOn(backend, "deleteWorktree"),
    setClosedRepos: vi.spyOn(backend, "setClosedRepos"),
    createWorktree: vi.spyOn(backend, "createWorktree"),
    fetchRepo: vi.spyOn(backend, "fetchRepo"),
    updateRepo: vi.spyOn(backend, "updateRepo"),
    removeRepo: vi.spyOn(backend, "removeRepo"),
    moveRepo: vi.spyOn(backend, "moveRepo"),
    setSettings: vi.spyOn(backend, "setSettings"),
  };
  const result = render(() => <App backend={backend} initial={backend.boot.model} />);
  return { backend, spies, ...result };
}

describe("App", () => {
  it("shows every repo and its worktrees from the first render", () => {
    setup();
    expect(screen.getByText("app")).toBeTruthy();
    expect(screen.getByText("site")).toBeTruthy();
    expect(screen.getByLabelText("claude/tauri-rewrite")).toBeTruthy();
    expect(screen.getByText("folder missing")).toBeTruthy();
    expect(screen.getByText("Creating…")).toBeTruthy();
  });

  it("gives every row its own buttons", () => {
    setup();
    // One per worktree: an icon's DOM node shared between rows would end
    // up in the last row only.
    expect(screen.getAllByLabelText("Push")).toHaveLength(5);
    expect(document.querySelectorAll(".disclosure svg")).toHaveLength(2);
  });

  it("filters rows as the search is typed", () => {
    setup();
    fireEvent.input(screen.getByLabelText("Search worktrees"), { target: { value: "fix" } });
    expect(screen.getByLabelText("fix-login")).toBeTruthy();
    expect(screen.queryByLabelText("claude/tauri-rewrite")).toBeNull();
    expect(screen.queryByLabelText("old-experiment")).toBeNull();
    expect(screen.getByText("main · 1 of 3 worktrees")).toBeTruthy();
  });

  it("sends a row's action with its repo and path", () => {
    const { spies } = setup();
    const row = screen.getByLabelText("fix-login").closest("article")!;
    fireEvent.click(row.querySelector('[aria-label="Push"]')!);
    expect(spies.push).toHaveBeenCalledWith("app", "/home/demo/worktrees/app/fix-login");
    fireEvent.click(row.querySelector('[aria-label="Open in terminal"]')!);
    expect(spies.openInTerminal).toHaveBeenCalledWith("/home/demo/worktrees/app/fix-login");
  });

  it("disables git actions on a missing folder", () => {
    setup();
    const row = screen.getByLabelText("old-experiment").closest("article")!;
    expect(row.querySelector<HTMLButtonElement>('[aria-label="Push"]')!.disabled).toBe(true);
    expect(row.querySelector<HTMLButtonElement>('[aria-label="Open in editor"]')!.disabled).toBe(true);
  });

  it("shows a busy row's label and the notice the core sends", async () => {
    vi.useFakeTimers();
    try {
      setup();
      const row = screen.getByLabelText("fix-login").closest("article")!;
      fireEvent.click(row.querySelector('[aria-label="Push"]')!);
      expect(screen.getByText("Pushing…")).toBeTruthy();
      expect(row.querySelector<HTMLButtonElement>('[aria-label="Push"]')!.disabled).toBe(true);
      await vi.advanceTimersByTimeAsync(1000);
      expect(screen.queryByText("Pushing…")).toBeNull();
      expect(screen.getByText("Pushed.")).toBeTruthy();
    } finally {
      vi.useRealTimers();
    }
  });

  it("closes a card and opens it again", () => {
    setup();
    fireEvent.click(screen.getByLabelText("Collapse site"));
    expect(screen.queryByLabelText("old-experiment")).toBeNull();
    fireEvent.click(screen.getByLabelText("Expand site"));
    expect(screen.getByLabelText("old-experiment")).toBeTruthy();
  });

  it("refreshes on ⌘R / Ctrl+R", () => {
    const { spies } = setup();
    fireEvent.keyDown(document, { key: "r", ctrlKey: true });
    expect(spies.refresh).toHaveBeenCalled();
  });

  it("remembers a closed card", () => {
    const { spies } = setup();
    fireEvent.click(screen.getByLabelText("Collapse site"));
    expect(spies.setClosedRepos).toHaveBeenLastCalledWith(["site"]);
  });

  it("switches branch from the picker, filtered by what is typed", () => {
    const { spies } = setup();
    const row = screen.getByLabelText("fix-login").closest("article")!;
    fireEvent.click(row.querySelector("button.branch")!);
    const field = screen.getByLabelText("Filter branches");
    expect(document.activeElement).toBe(field);
    fireEvent.input(field, { target: { value: "tauri" } });
    expect(screen.getAllByRole("option")).toHaveLength(1);
    fireEvent.keyDown(field, { key: "Enter" });
    expect(spies.switchBranch).toHaveBeenCalledWith(
      "app",
      "/home/demo/worktrees/app/fix-login",
      "claude/tauri-rewrite",
    );
    expect(screen.queryByLabelText("Filter branches")).toBeNull();
  });

  it("does nothing when the current branch is chosen", () => {
    const { spies } = setup();
    const row = screen.getByLabelText("fix-login").closest("article")!;
    fireEvent.click(row.querySelector("button.branch")!);
    // It opens on the current branch.
    fireEvent.keyDown(screen.getByLabelText("Filter branches"), { key: "Enter" });
    expect(spies.switchBranch).not.toHaveBeenCalled();
  });

  it("asks twice before deleting a worktree with changes", async () => {
    const { spies } = setup();
    const row = screen.getByLabelText("claude/tauri-rewrite").closest("article")!;
    fireEvent.click(row.querySelector('[aria-label="Delete worktree"]')!);
    expect(screen.getByText("Delete worktree “claude/tauri-rewrite”?")).toBeTruthy();
    fireEvent.click(screen.getByText("Delete"));
    expect(await screen.findByText("“claude/tauri-rewrite” has uncommitted changes")).toBeTruthy();
    fireEvent.click(screen.getByText("Force Delete — Discard Changes"));
    await vi.waitFor(() => expect(screen.queryByLabelText("claude/tauri-rewrite")).toBeNull());
    expect(spies.deleteWorktree).toHaveBeenLastCalledWith(expect.objectContaining({ force: true }));
  });

  it("cancels a delete with Escape and offers none on the primary tree", () => {
    const { spies } = setup();
    const main = screen.getAllByLabelText("main")[0].closest("article")!;
    expect(main.querySelector('[aria-label="Delete worktree"]')).toBeNull();
    const row = screen.getByLabelText("fix-login").closest("article")!;
    fireEvent.click(row.querySelector('[aria-label="Delete worktree"]')!);
    fireEvent.keyDown(screen.getByRole("alertdialog"), { key: "Escape" });
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(spies.deleteWorktree).not.toHaveBeenCalled();
  });

  it("walks the rows with the arrow keys and closes a card with ←", () => {
    setup();
    const selected = () => document.querySelector(".selected")!;
    fireEvent.keyDown(document, { key: "ArrowDown" });
    expect(selected().textContent).toContain("app");
    fireEvent.keyDown(document, { key: "ArrowDown" });
    fireEvent.keyDown(document, { key: "ArrowDown" });
    expect(selected().querySelector('[aria-label="claude/tauri-rewrite"]')).toBeTruthy();
    fireEvent.keyDown(document, { key: "ArrowLeft" });
    fireEvent.keyDown(document, { key: "ArrowLeft" });
    expect(screen.queryByLabelText("claude/tauri-rewrite")).toBeNull();
    fireEvent.keyDown(document, { key: "ArrowRight" });
    expect(screen.getByLabelText("claude/tauri-rewrite")).toBeTruthy();
  });

  it("opens the picker on the selected worktree with Return", () => {
    setup();
    fireEvent.keyDown(document, { key: "ArrowDown" });
    fireEvent.keyDown(document, { key: "ArrowDown" });
    fireEvent.keyDown(document, { key: "Enter" });
    expect(screen.getByLabelText("Filter branches")).toBeTruthy();
  });

  it("creates a worktree once the core's check lets it", async () => {
    const { spies } = setup();
    fireEvent.click(screen.getAllByText("New Worktree")[0]);
    expect(spies.fetchRepo).toHaveBeenCalledWith("app");
    const name = screen.getByLabelText("Branch name:");
    const create = () => screen.getByText("Create") as HTMLButtonElement;
    fireEvent.input(name, { target: { value: "fix-login" } });
    expect(await screen.findByText("Branch already has a worktree.")).toBeTruthy();
    expect(create().disabled).toBe(true);
    fireEvent.input(name, { target: { value: "release/2.0" } });
    expect(await screen.findByText("Branch already exists; use Existing branch.")).toBeTruthy();
    fireEvent.click(screen.getByText("Existing branch"));
    await vi.waitFor(() => expect(create().disabled).toBe(false));
    fireEvent.click(create());
    expect(spies.createWorktree).toHaveBeenCalledWith({
      repoId: "app",
      branch: "release/2.0",
      source: { kind: "existing" },
    });
    expect(screen.queryByLabelText("Branch name:")).toBeNull();
  });

  it("saves a repo's settings, and removes it after asking", () => {
    const { spies } = setup();
    fireEvent.click(screen.getByLabelText("Settings for site"));
    fireEvent.input(screen.getByLabelText("Init command:"), { target: { value: "make" } });
    fireEvent.click(screen.getByText("Save"));
    expect(spies.updateRepo).toHaveBeenCalledWith({
      repoId: "site",
      name: "site",
      mainBranch: "main",
      initCommand: "make",
    });
    fireEvent.click(screen.getByLabelText("Settings for site"));
    fireEvent.click(screen.getByText("Remove Repo…"));
    expect(screen.getByText("Remove “site” from the list?")).toBeTruthy();
    fireEvent.click(screen.getByText("Remove"));
    expect(spies.removeRepo).toHaveBeenCalledWith("site");
  });

  it("saves the app's settings", () => {
    const { spies } = setup();
    fireEvent.click(screen.getByLabelText("Settings"));
    fireEvent.input(screen.getByLabelText("Editor command:"), { target: { value: "zed" } });
    fireEvent.change(screen.getByLabelText(".worktreeinclude:"), { target: { value: "copy" } });
    fireEvent.click(screen.getByText("Save"));
    expect(spies.setSettings).toHaveBeenCalledWith({
      worktreesRoot: "/home/demo/worktrees",
      editorCommand: "zed",
      copyWorktreeInclude: true,
      updateChannel: "stable",
    });
  });

  it("moves the selected card with ⌥⌘↓", () => {
    const { spies } = setup();
    fireEvent.keyDown(document, { key: "ArrowDown" });
    fireEvent.keyDown(document, { key: "ArrowDown", altKey: true, metaKey: true });
    expect(spies.moveRepo).toHaveBeenCalledWith("app", ["app", "site"], 2);
    expect(document.querySelector(".card")!.textContent).toContain("site");
  });
});
