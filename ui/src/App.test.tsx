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
});
