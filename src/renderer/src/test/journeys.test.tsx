import { beforeEach, describe, expect, it } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";
import type { CreateWorktreeResult } from "@shared/types";
import { apiMock, resetApiMock } from "./apiMock";
import { renderApp } from "./renderApp";
import { deferred, makeNode, makeStatus, makeWorktree } from "./fixtures";

beforeEach(() => {
  resetApiMock();
});

describe("empty state", () => {
  it("prompts to add a repository when none are configured", async () => {
    renderApp();
    expect(await screen.findByText("No repositories yet")).toBeInTheDocument();
  });
});

describe("add a repository", () => {
  it("picks a folder, adds the repo, and shows it in the tree", async () => {
    const { user } = renderApp();
    await screen.findByText("No repositories yet");

    apiMock.pickDirectory.mockResolvedValue("/Users/test/dev/app");
    apiMock.addRepo.mockResolvedValue({
      worktreesRoot: "/Users/test/worktrees",
      editorCommand: "code",
      repos: [
        { id: "r1", name: "app", path: "/Users/test/dev/app", mainBranch: "main", initCommand: "" },
      ],
    });
    // After the add, the invalidated repos query refetches this populated list.
    apiMock.listRepos.mockResolvedValue([
      makeNode({}, [makeWorktree({ branch: "main", isMain: true, status: makeStatus() })]),
    ]);

    // Two "+ Add repo" buttons exist in the empty state (top bar + welcome).
    await user.click(screen.getAllByRole("button", { name: "+ Add repo" })[0]);

    expect(apiMock.pickDirectory).toHaveBeenCalledWith("Select a git repository");
    expect(await screen.findByText("app")).toBeInTheDocument();
    // The worktree row and a status badge render.
    expect(screen.getByText("primary")).toBeInTheDocument();
    expect(apiMock.addRepo).toHaveBeenCalledWith("/Users/test/dev/app");
  });

  it("does nothing when the folder picker is cancelled", async () => {
    const { user } = renderApp();
    await screen.findByText("No repositories yet");
    apiMock.pickDirectory.mockResolvedValue(null);

    await user.click(screen.getAllByRole("button", { name: "+ Add repo" })[0]);

    expect(apiMock.addRepo).not.toHaveBeenCalled();
  });
});

describe("create a worktree", () => {
  beforeEach(() => {
    apiMock.listRepos.mockResolvedValue([
      makeNode({}, [makeWorktree({ branch: "main", isMain: true })]),
    ]);
  });

  it("shows a Creating… placeholder, then the real row when it resolves", async () => {
    const { user } = renderApp();
    await screen.findByText("app");

    const create = deferred<CreateWorktreeResult>();
    apiMock.createWorktree.mockReturnValue(create.promise);

    await user.click(screen.getByRole("button", { name: "+ Worktree" }));
    await user.type(await screen.findByPlaceholderText("e.g., feature/my-thing"), "feat/login");
    await user.click(screen.getByRole("button", { name: "Create" }));

    // Fire-and-close: the dialog closes and a placeholder row appears.
    expect(await screen.findByText("Creating…")).toBeInTheDocument();
    // New branches default to the remote trunk (origin/<trunk>), so they start
    // from the latest fetched state rather than a possibly-stale local trunk.
    expect(apiMock.createWorktree).toHaveBeenCalledWith({
      repoId: "r1",
      branch: "feat/login",
      newBranch: true,
      baseRef: "origin/main",
    });

    // Once it lands, the refetched tree includes the new worktree.
    apiMock.listRepos.mockResolvedValue([
      makeNode({}, [
        makeWorktree({ branch: "main", isMain: true }),
        makeWorktree({ branch: "feat/login", path: "/Users/test/worktrees/app/feat-login" }),
      ]),
    ]);
    create.resolve({ worktree: makeWorktree({ branch: "feat/login" }), initStarted: false });

    // The real row (identified by its unique path) replaces the placeholder.
    expect(await screen.findByText("~/worktrees/app/feat-login")).toBeInTheDocument();
    await waitFor(() => expect(screen.queryByText("Creating…")).not.toBeInTheDocument());
  });

  it("shows an Initialising badge while the init command runs in the background", async () => {
    const { user } = renderApp();
    await screen.findByText("app");

    const wtPath = "/Users/test/worktrees/app/feat-x";
    apiMock.createWorktree.mockResolvedValue({
      worktree: makeWorktree({ branch: "feat/x", path: wtPath }),
      initStarted: true,
    });
    // The refetched tree includes the new worktree...
    apiMock.listRepos.mockResolvedValue([
      makeNode({ initCommand: "pnpm i" }, [
        makeWorktree({ branch: "main", isMain: true, path: "/Users/test/worktrees/app/main" }),
        makeWorktree({ branch: "feat/x", path: wtPath }),
      ]),
    ]);
    // ...and the init run is reported as running on it, driving the badge.
    apiMock.listRunningCommands.mockResolvedValue([
      {
        repoId: "r1",
        worktreePath: wtPath,
        commandId: "__init__",
        name: "Initialising",
        command: "pnpm i",
        startedAt: 0,
      },
    ]);

    await user.click(screen.getByRole("button", { name: "+ Worktree" }));
    await user.type(await screen.findByPlaceholderText("e.g., feature/my-thing"), "feat/x");
    await user.click(screen.getByRole("button", { name: "Create" }));

    // The row is present (usable) and carries the Initialising badge — the app
    // is not blocked waiting for the init command to finish.
    expect(await screen.findByText("~/worktrees/app/feat-x")).toBeInTheDocument();
    expect(await screen.findByText("Initialising…")).toBeInTheDocument();
  });
});

describe("delete a worktree (safety ladder)", () => {
  it("escalates a dirty worktree to an explicit force confirmation", async () => {
    apiMock.listRepos.mockResolvedValue([
      makeNode({}, [
        makeWorktree({ branch: "main", isMain: true, path: "/Users/test/worktrees/app/main" }),
        makeWorktree({
          branch: "feature",
          path: "/Users/test/worktrees/app/feature",
          status: makeStatus({ hasUnstaged: true }),
        }),
      ]),
    ]);
    const { user } = renderApp();
    await screen.findByText("app");

    // First delete is refused as dirty; the second (forced) succeeds.
    apiMock.deleteWorktree
      .mockResolvedValueOnce({ ok: false, reason: "dirty", message: "worktree is dirty" })
      .mockResolvedValueOnce({ ok: true, message: "" });

    await user.click(screen.getByRole("button", { name: "Delete worktree" }));
    await user.click(screen.getByRole("button", { name: "Delete" }));

    // Escalates to the force step.
    const forceBtn = await screen.findByRole("button", { name: "Force delete — discard changes" });

    // After the forced delete, the refetched tree no longer has the worktree.
    apiMock.listRepos.mockResolvedValue([
      makeNode({}, [
        makeWorktree({ branch: "main", isMain: true, path: "/Users/test/worktrees/app/main" }),
      ]),
    ]);
    await user.click(forceBtn);

    expect(apiMock.deleteWorktree).toHaveBeenCalledTimes(2);
    expect(apiMock.deleteWorktree).toHaveBeenLastCalledWith({
      repoId: "r1",
      worktreePath: "/Users/test/worktrees/app/feature",
      expectedBranch: "feature",
      force: true,
    });
    await waitFor(() => expect(screen.queryByText("feature")).not.toBeInTheDocument());
  });
});

describe("git op errors", () => {
  it("shows git's message in the row when a push is rejected", async () => {
    apiMock.listRepos.mockResolvedValue([makeNode({}, [makeWorktree({ branch: "feature" })])]);
    apiMock.pushWorktree.mockResolvedValue({ ok: false, message: "rejected: non-fast-forward" });
    const { user } = renderApp();
    await screen.findByText("app");

    await user.click(screen.getByRole("button", { name: "Push" }));

    expect(await screen.findByText("rejected: non-fast-forward")).toBeInTheDocument();
    expect(apiMock.pushWorktree).toHaveBeenCalledWith("r1", "/Users/test/worktrees/app/feature");
  });
});

describe("settings", () => {
  it("saves the edited editor command", async () => {
    apiMock.getConfig.mockResolvedValue({
      worktreesRoot: "/Users/test/worktrees",
      editorCommand: "code",
      repos: [],
    });
    apiMock.setAppSettings.mockResolvedValue({
      worktreesRoot: "/Users/test/worktrees",
      editorCommand: "vim",
      repos: [],
    });
    const { user } = renderApp();
    await screen.findByText("No repositories yet");

    await user.click(screen.getByRole("button", { name: "Settings" }));
    const dialog = await screen.findByRole("dialog", { name: "Settings" });
    const editor = within(dialog).getByRole("textbox", { name: /Editor command/i });
    await user.clear(editor);
    await user.type(editor, "vim");
    await user.click(within(dialog).getByRole("button", { name: "Save" }));

    await waitFor(() =>
      expect(apiMock.setAppSettings).toHaveBeenCalledWith({
        worktreesRoot: "/Users/test/worktrees",
        editorCommand: "vim",
      }),
    );
  });
});
