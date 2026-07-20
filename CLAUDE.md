# Worktree Manager

An Electron desktop app for managing git worktrees across multiple repositories.
The main window is a tree view: repositories at the top level, their worktrees
nested underneath, each with git status and quick actions.

## Stack

- **Electron** (main + preload + renderer) bundled with **electron-vite**
- **React 19** + **TypeScript 7** in the renderer
- **TanStack Query** for renderer data/state (queries + mutations over IPC)
- **electron-store** for persisted preferences
- **pnpm** as package manager / task runner
- **Vitest** for unit tests
- **Prettier** for formatting (`objectWrap: collapse`, double quotes, semicolons,
  `trailingComma: all`)

## Commands

```sh
pnpm install         # install deps (postinstall fetches the Electron binary)
pnpm dev             # launch the app in dev (HMR renderer)
pnpm build           # typecheck + production build
pnpm start           # preview a production build
pnpm test            # run vitest once
pnpm test:watch      # vitest watch mode
pnpm typecheck       # tsc --noEmit for node + web projects
pnpm format          # prettier --write .
pnpm dist            # package macOS .app + .dmg into release/ (electron-builder)
```

## Packaging

`pnpm dist` runs electron-builder with `electron-builder.yml`. ALL runtime deps
(including electron-store) live in devDependencies so electron-vite bundles them
into `out/` — the packaged app ships no node_modules, which avoids
electron-builder's pnpm-symlink issues. Consequence: never add a runtime dep to
"dependencies"; add it to devDependencies and let the bundler inline it. The
build is unsigned (`identity: null`) — set a real identity before distributing.

## Version constraints (learned the hard way)

- **vite must stay on ^7** while electron-vite is on 5.x (peer range `^5 || ^6 || ^7`).
  With vite 8, electron-vite silently fails to externalize the `electron` package,
  bundling its Node launcher into `out/main/index.js` — the app then dies at startup
  with "Unable to find Electron app at .../out/main/install.js". If the main bundle
  is suspiciously large (hundreds of kB instead of ~16 kB), suspect this first.
- **@vitejs/plugin-react must stay on ^5** (6.x requires vite 8).
- The `electron` npm package here has **no postinstall script**, so the root
  `postinstall` in package.json runs `node node_modules/electron/install.js` to
  download the binary into the pnpm store. Without it, launches fail with
  "Electron failed to install correctly".

## Layout

```
src/
  shared/types.ts        Types shared by main & renderer (the IPC contract)
  main/                  Electron main process (Node)
    index.ts             App/window lifecycle
    ipc.ts               ipcMain handlers; CH channel-name map
    store.ts             electron-store persistence (AppConfig, RepoConfig)
    git.ts               git command runner + pure porcelain parsers
    worktrees.ts         create/delete/list orchestration; path building
    system.ts            open in editor / terminal / Finder
    *.test.ts            Vitest unit tests (pure parsers, path logic)
  preload/
    index.ts             contextBridge → window.api (typed WorktreeApi)
    index.d.ts           global Window.api augmentation
  renderer/
    index.html
    src/
      main.tsx           React root + QueryClientProvider
      App.tsx            Top bar + repo tree + empty state
      api.ts             window.api accessor
      queries.ts         TanStack Query hooks (keys, queries, mutations)
      components/        Modal, RepoNode, WorktreeRow, StatusBadges, dialogs
      styles.css         Compact theme, light/dark via prefers-color-scheme
```

## Key behaviors

- **Persistence**: the worktrees root folder, editor command, and the repo list
  (each with `mainBranch` + `initCommand`) live in electron-store, so config
  survives relaunches.
- **Worktree paths**: new worktrees are created at
  `<worktreesRoot>/<repoName>/<branch-slug>` (see `worktreePathFor`).
- **Adding a repo** resolves the path to its git root, auto-detects the main
  branch, and immediately lists existing worktrees.
- **Status** per worktree: staged / unstaged / untracked, ahead/behind the repo's
  configured main branch, and unpushed commits vs upstream. Parsing is done by
  pure functions in `git.ts` (`parseWorktreePorcelain`, `parseStatusPorcelainV2`)
  which are unit-tested.
- **Delete** is a safety ladder (see `deleteWorktree` in `worktrees.ts`): path must
  match git's own worktree list, primary tree refused, branch revalidated against
  what the UI showed (`expectedBranch`), and `git worktree remove` runs WITHOUT
  `--force` first — a dirty tree returns reason `"dirty"` and the UI demands an
  explicit second "Force delete — discard changes" confirmation. No blanket
  `worktree prune`, no `rm -rf` fallback. Missing-folder ("prunable") worktrees
  are cleaned up via targeted prune, refused if other prunable worktrees exist.
  Mutating ops are serialized per worktree path (`withWorktreeLock`).
- **Editor** and **terminal** commands are global; **init command** and **main
  branch** are per-repo. Both editor/terminal commands support a `{path}`
  placeholder (see `buildCommand` in `main/command.ts`), otherwise the path is
  appended as a quoted argument.
- **Per-worktree git ops**: push (auto `-u origin HEAD` on first push), pull
  (`--ff-only`), pull-primary-branch (`pull origin <main>`, or local merge when
  no remote), and a branch-switch dropdown (`git switch` — git refuses unsafe
  switches). These return `GitOpResult` (`{ ok, message }`) instead of throwing,
  so git's stderr surfaces in the row UI. All ops validate the worktree belongs
  to the repo first (`requireWorktree`).
- Icons are **lucide-react**; icon-only buttons need `btn-icon` plus `title`
  and `aria-label`.

## Conventions

- Section headers in code use `// MARK:` comments so they show up in the minimap.
- All input-field placeholders must start with `e.g.,`.
- Paths shown in the UI are abbreviated with `~` via `displayPath`
  (`renderer/src/format.ts`, backed by `tildify` in `shared/paths.ts`); keep the
  full path in the `title` tooltip. Inputs hold real, unabbreviated paths.
- **Theming**: all colors are CSS custom properties in `styles.css` — light values
  in `:root`, dark overrides in `@media (prefers-color-scheme: dark)`. Tints derive
  from base tokens via `color-mix()`; never hard-code a hex outside the token blocks.
  The native window background follows `nativeTheme` in `main/index.ts`.
- The IPC contract is the `WorktreeApi` interface in `src/shared/types.ts`; the
  channel-name map (`CH`) is duplicated in `ipc.ts` and `preload/index.ts` — keep
  them in sync.
- Prefer adding pure, testable helpers (like the parsers) over inline logic.
