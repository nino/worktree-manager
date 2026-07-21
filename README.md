# Worktree Manager

A compact macOS desktop app for managing [git worktrees](https://git-scm.com/docs/git-worktree)
across multiple repositories.

The main window is a tree: repositories at the top level, their worktrees nested
underneath — each with branch, path, live git status, and one-click actions.

## Features

- **Status at a glance** — staged / unstaged / untracked changes, commits ahead of
  or behind the repo's primary branch, unpushed commits, `✓` for clean trees, and a
  "folder missing" badge for worktrees whose directory was deleted outside the app.
- **One-click git ops per worktree** — push (sets upstream on first push), pull
  (fast-forward only), merge the primary branch in (`--no-rebase`, your
  `pull.rebase` config can't rewrite history from a button), and a branch-switch
  dropdown backed by plain `git switch`.
- **Create worktrees** — new or existing branch, based on any ref. Worktrees land in
  `<worktrees root>/<repo name>/<branch-slug>` and the repo's init command (e.g.
  `pnpm i`) runs automatically in the new tree.
- **Delete with a safety ladder** — the path is verified against git's own worktree
  list, the branch is revalidated at delete time (stale rows refuse), and
  `git worktree remove` runs _without_ `--force` first: deleting a dirty worktree
  requires an explicit second "Force delete — discard changes" confirmation.
  The primary working tree can never be deleted.
- **Open in editor / terminal / Finder** — editor and terminal commands are
  configurable globally and support a `{path}` placeholder
  (e.g. Ghostty: `open -na Ghostty --args --working-directory={path}`).
- **Persistent config** — worktrees root, editor/terminal commands, and the repo
  list (with per-repo primary branch + init command) survive relaunches.
- Light/dark theme following the system.

## Install & run

Requires [pnpm](https://pnpm.io) and git ≥ 2.36.

```sh
pnpm install   # postinstall fetches the Electron binary
pnpm dev       # run in development (renderer HMR)
```

## Build

Run `pnpm i && pnpm build && pnpm dist`, and boom, you'll have an app ready in the dist folder.

```sh
pnpm build     # typecheck + production build into out/
pnpm start     # preview the production build
pnpm dist      # package a macOS .app + .dmg into release/
```

## Install into /Applications

Once you've packaged the app, copy the bundle into your Applications folder:

```sh
pnpm dist         # build the .app (skip if you already have one in release/)
pnpm install-app  # copy release/…/Worktree Manager.app → /Applications
```

`install-app` is macOS-only — it exits with an error on other platforms, since
the packaged target is a `.app` bundle. It overwrites any existing install.

## Development

```sh
pnpm test          # vitest (pure parsers, path logic)
pnpm typecheck     # tsc for main + renderer projects
pnpm format        # prettier
```

Stack: Electron (electron-vite), React 19, TypeScript 7, TanStack Query,
electron-store, Vitest. See [CLAUDE.md](CLAUDE.md) for architecture notes,
conventions, and version constraints.

## Configuration

| Setting          | Scope    | Default                                                |
| ---------------- | -------- | ------------------------------------------------------ |
| Worktrees root   | global   | `~/.claude-worktrees`                                  |
| Editor command   | global   | `code`                                                 |
| Terminal command | global   | `open -a Terminal`                                     |
| Primary branch   | per repo | auto-detected from `origin/HEAD`, else `main`/`master` |
| Init command     | per repo | _(empty)_                                              |

Adding a repository resolves the picked folder to its primary working tree (even
if you pick a linked worktree) and lists all existing worktrees immediately.
