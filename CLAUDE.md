# Worktree Manager

An Electron desktop app for managing git worktrees across multiple repositories.
The main window is a tree view: repositories at the top level, their worktrees
nested underneath, each with git status and quick actions.

## Stack

- **Electron** (main + preload + renderer) bundled with **electron-vite**
- **React 19** + **TypeScript 7** in the renderer
- **TanStack Query** for renderer data/state (queries + mutations over IPC)
- **electron-store** for persisted preferences
- **pnpm 11** as package manager / task runner, pinned by the `packageManager`
  field and run through corepack (not a global install — see "pnpm settings")
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
pnpm screenshot      # regenerate docs/screenshot.png (throwaway demo repos,
                     # sandboxed config via WTM_USER_DATA — never real user data)
pnpm bake-grain      # re-render the brushed-metal grain tiles into
                     # src/renderer/src/assets/ (only after retuning the filter)
```

## Packaging

`pnpm dist` runs electron-builder with `electron-builder.yml`. ALL runtime deps
(including electron-store) live in devDependencies so electron-vite bundles them
into `out/` — the packaged app ships no node_modules, which avoids
electron-builder's pnpm-symlink issues. Consequence: never add a runtime dep to
"dependencies"; add it to devDependencies and let the bundler inline it. The
build is signed and notarised in CI — see "Releases" below.

## Releases

Every push to `main` — or a manual run from the Actions tab, which is how to
retry after a credentials failure without inventing a commit — builds a macOS
arm64 DMG and a zip of the same `.app`, signs them with a Developer ID
certificate, notarises the app and the disk image, staples both tickets, and
replaces the rolling `latest` GitHub release with them. A downloader can
double-click the DMG; no right-click → Open, and no network round-trip on first
launch.

The release carries the DMG for people and the zip, its blockmap and
`latest-mac.yml` for the in-app updater (see "Automatic updates"). The workflow
stamps the version as `1.0.<commits on main>` into package.json before building
— the tree itself stays at 1.0.0 — because the updater compares semver: the
number has to grow with every release, and re-running the workflow on the same
commit has to produce the _same_ version, or a rebuild would look like an
update. That is also why the DMG and zip are named from the package name
(`mac.artifactName`): GitHub rewrites spaces in asset names, and the URLs in
`latest-mac.yml` have to survive the upload.

Locally `pnpm dist` signs with whatever Developer ID is in your keychain, or
warns and leaves the build unsigned if there is none. Do not set
`CSC_LINK`/`CSC_KEY_PASSWORD` to test the CI path: electron-builder hands
`security set-key-partition-list` the `.p12` password where the keychain's own
password belongs, which macOS 26 rejects outright. The workflow sidesteps that
by building the keychain itself and passing it as `CSC_KEYCHAIN`.

Five repository secrets, under Settings → Secrets and variables → Actions:

| secret                       | what it is                                                                                                                    |
| ---------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| `MACOS_CERTIFICATE_P12`      | The Developer ID Application certificate _and its private key_, exported from Keychain Access as `.p12`, then base64-encoded. |
| `MACOS_CERTIFICATE_PASSWORD` | The password set during that export.                                                                                          |
| `APPLE_API_KEY_P8`           | The whole contents of the `.p8` App Store Connect key, `-----BEGIN PRIVATE KEY-----` line included.                           |
| `APPLE_API_KEY_ID`           | The ten-character key ID.                                                                                                     |
| `APPLE_API_ISSUER_ID`        | The issuer UUID.                                                                                                              |

## pnpm settings

pnpm is pinned by `packageManager` in package.json and executed through
**corepack**, which downloads that exact version on demand — there is no global
pnpm to upgrade. To move versions, edit that field (or run `corepack use
pnpm@<v>`); a `pnpm` on PATH from mise/npm is bypassed.

From pnpm 11 on, settings live in **`pnpm-workspace.yaml`** (present even though
this is not a workspace). `.npmrc` may hold auth/registry config only, and the
`pnpm` field in package.json is **ignored** — putting settings there fails
silently. Dependency build scripts are blocked unless listed in `allowBuilds`
(which replaced pnpm 10's `onlyBuiltDependencies`); an unlisted package that
needs one fails the install with `ERR_PNPM_IGNORED_BUILDS`, and pnpm appends a
`<name>: set this to true or false` placeholder to the file for you to resolve.

## Version constraints (learned the hard way)

- **vite must stay on ^7** while electron-vite is on 5.x (peer range `^5 || ^6 || ^7`).
  With vite 8, electron-vite silently fails to externalize the `electron` package,
  bundling its Node launcher into `out/main/index.js` — the app then dies at startup
  with "Unable to find Electron app at .../out/main/install.js". Don't diagnose
  this by bundle size — `out/main/index.js` is legitimately ~1 MB because
  electron-store (and its `conf`/`ajv` tree) and electron-updater are inlined on
  purpose, per "Packaging" above. Check instead that the bundle still _imports_
  electron rather than inlining it:
  `grep -c 'from "electron"' out/main/index.js` (1 = externalized, good).
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
    repos.ts             add repos by path (picker + Dock drops), per-path errors
    worktrees.ts         create/delete/list orchestration; path building
    system.ts            open in editor / terminal / Finder
    updater.ts           electron-updater loop against the GitHub release
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
      creations.tsx      In-flight worktree creations + arrival highlights
      poof.tsx           Puff-of-smoke layer played over deleted rows
      components/        Modal, RepoNode, WorktreeRow, StatusBadges, dialogs
      styles.css         Brushed-metal skeuomorphic theme (single appearance)
```

## Key behaviors

- **Persistence**: the worktrees root folder, editor command, and the repo list
  (each with `mainBranch` + `initCommand`) live in electron-store, so config
  survives relaunches.
- **Worktree paths**: new worktrees are created at
  `<worktreesRoot>/<repoName>/<branch-slug>` (see `worktreePathFor`).
- **New-branch base ref**: a new branch defaults to `origin/<mainBranch>` when
  that remote-tracking ref exists, else the local `mainBranch` — see
  `defaultBaseRefFor` in `worktrees.ts`, surfaced to the UI as
  `RepoWithWorktrees.defaultBaseRef` and used as the dialog default. New branches
  are created `--no-track` (see `addWorktree`) so branching off `origin/…` never
  adopts it as upstream — the "push sets upstream on first push" model relies on
  the branch having no upstream until pushed.
- **Background fetch**: `main/fetcher.ts` runs `git fetch --prune` on every repo
  with a remote once at launch and then every 8m43s (`FETCH_INTERVAL_MS`).
  Overlapping cycles are skipped; failures are logged, never fatal. After a cycle
  that fetched anything it pushes `CH.reposChanged`, and the renderer invalidates
  the `repos` query (see `useReposChangedRefresh` in `App.tsx`).
- **Automatic updates**: `main/updater.ts` drives electron-updater against the
  rolling `latest` GitHub release. It checks 10s after launch and every 6h
  (`UPDATE_CHECK_INTERVAL_MS`), downloads a newer build in the background, and
  lets Squirrel.Mac swap it in on quit (`autoInstallOnAppQuit`) — the top bar
  offers "Restart to update" to take it sooner. Every state change is pushed as
  `CH.updateStatus` (`UpdateStatus` in `shared/types.ts`) and cached by
  `useUpdateStatus`; the About dialog shows the running version and the state in
  words (`describeUpdate` in `renderer/src/format.ts`) with a "Check now"
  button. Nothing is fatal: a failed check leaves the app on the version it has.
  Only packaged builds update — `app-update.yml` (which names the feed) is
  written into the bundle by electron-builder, so a run from source reports
  state `"unsupported"` instead. The feed itself is the `publish` block in
  `electron-builder.yml`; without it electron-builder writes neither that file
  nor the `latest-mac.yml` the updater reads, and nothing would ever update.
  Squirrel installs from a **zip**, never a DMG, which is why the release
  carries both.
- **Adding repos** (`addRepos` in `main/repos.ts`) resolves each path to its git
  root, auto-detects the main branch, and immediately lists existing worktrees.
  It is best effort per path — a folder that isn't a repo (or is already added)
  lands in `AddReposResult.failed` instead of aborting the batch — and runs
  sequentially because `store.addRepo` is read-modify-write. The picker allows
  multi-selection (`pickDirectories`), so several repos can be added at once.
  Outcomes are summarized into the banner under the top bar by
  `summarizeAddResult` (`renderer/src/format.ts`): any failure makes the notice
  an error and it stays, a clean add is informational and clears after 5s.
- **Dock drops**: dragging repo folders onto the app icon adds them. macOS only
  offers this for declared document types, so `electron-builder.yml` declares
  `public.folder` in `CFBundleDocumentTypes` with `LSHandlerRank: Alternate`
  (Finder stays the owner of folders — never make this app their default
  handler). Drops arrive as `open-file` events, which can fire _before_
  `app.whenReady` when the drop launches the app: the listener sits at module
  scope in `main/index.ts`, buffers paths, and a 100ms debounce batches one
  multi-item drop into a single `addRepos` call. The result is queued in `ipc.ts`
  (`publishDropResult`) until a renderer claims it via `CH.takeDroppedRepos` —
  a window may not exist yet, and the renderer's listener attaches after the page
  loads, so pushing blindly would drop the outcome on the floor. Dock drops
  cannot be exercised with `pnpm dev` (dev runs Electron.app, whose Info.plist
  declares no document types) — test them against `pnpm dist`, where
  `open -a "<app>" <folder>` sends the same event a real drop does.
- **Status** per worktree: staged / unstaged / untracked, ahead/behind the repo's
  trunk, and unpushed commits vs upstream. The ahead/behind comparison uses the
  _remote_ trunk (`origin/<mainBranch>`) when that remote-tracking ref exists,
  else the local branch — see `resolveTrunkRef` in `git.ts`. The ref used is
  returned as `WorktreeStatus.trunkRef` and is what the ↑/↓ badges show, so a
  never-checked-out local trunk can't report a stale 0. `listReposWorktrees`
  resolves it once per repo (same value as `defaultBaseRef`) and passes it down.
  Parsing is done by pure functions in `git.ts` (`parseWorktreePorcelain`,
  `parseStatusPorcelainV2`) which are unit-tested.
- **Delete** is a safety ladder (see `deleteWorktree` in `worktrees.ts`): path must
  match git's own worktree list, primary tree refused, branch revalidated against
  what the UI showed (`expectedBranch`), and `git worktree remove` runs WITHOUT
  `--force` first — a dirty tree returns reason `"dirty"` and the UI demands an
  explicit second "Force delete — discard changes" confirmation. No blanket
  `worktree prune`, no `rm -rf` fallback. Missing-folder ("prunable") worktrees
  are cleaned up via targeted prune, refused if other prunable worktrees exist.
  Mutating ops are serialized per worktree path (`withWorktreeLock`).
- **Editor** command is global; **init command** and **main branch** are
  per-repo. The editor command supports a `{path}` placeholder (see
  `buildCommand` in `main/command.ts`), otherwise the path is appended as a
  quoted argument.
- **Open in terminal** uses the user's system-wide default terminal — there is
  no configurable command. On macOS the "default terminal" is the Launch
  Services handler for the `public.unix-executable` content type (what Ghostty /
  iTerm register via "Set as default terminal"); `main/terminal.ts` parses that
  handler out of the LS database and `system.ts` opens the folder with
  `open -b <bundleId> <path>` (no `-n`, so a running instance is reused). Falls
  back to `Terminal.app` when no override is set.
- **Per-worktree git ops**: push (auto `-u origin HEAD` on first push), pull
  (`--ff-only`), pull-primary-branch (`pull origin <main>`, or local merge when
  no remote), and a branch-switch dropdown (`git switch` — git refuses unsafe
  switches). These return `GitOpResult` (`{ ok, message }`) instead of throwing,
  so git's stderr surfaces in the row UI. All ops validate the worktree belongs
  to the repo first (`requireWorktree`).
- **Branch labels**: a branch under a coding agent's prefix shows that agent's
  mark instead of the prefix text — `claude/…` and `cursor/…` today. The split is
  `splitToolPrefix` (`renderer/src/branchTool.ts`, pure and unit-tested); the
  icons live in `components/ToolIcons.tsx`; `BranchLabel` renders both and is
  used by the row plate, the picker trigger, and every picker option. The prefix
  text stays in the DOM behind `.sr-only`, so an accessible name and a copied
  selection still carry the whole branch name, and picker options repeat it in
  `aria-label`. Agent branches are long, which is why the plate allows 44ch and
  the picker popup opens at 260px.
- **Command launcher**: `WorktreeCommands` renders nothing when the repo has no
  configured commands — no disabled placeholder plate in every row. Runs still
  in flight keep their Stop button, so a command deleted from settings mid-run
  can still be stopped.
- **Worktree row layout**: each row is two full-width lines (`.wt-line1`,
  `.wt-line2` in `WorktreeRow.tsx`). Line one holds the branch picker + copy
  button on the left (`.wt-ident`, also the anchor for the departure poof) and
  the status tags right-aligned (`.wt-tags`); line two holds the path on the
  left and the action buttons on the right. Tags and buttons never share a line,
  which is what keeps long agent branch names, several badges and every button
  visible at once. Keep new per-row indicators in `.wt-tags` and new per-row
  controls in `.wt-actions`.
- **Row animations**: a created worktree lands with a specular highlight raked
  across its slat (`.wt-new` in `styles.css`); `CreationsProvider` flags the
  branch as arriving for `ARRIVAL_MS` after the create resolves, and
  `WorktreeRow` applies the class. A deleted one leaves in a Dock-style puff of
  smoke. The deleted row unmounts as soon as the tree refetches, so the cloud
  can't live in it: `WorktreeRow` measures `.wt-info` _before_ awaiting the
  delete and, on success, hands the rect to `PoofProvider` (`poof.tsx`), which
  plays it in a fixed click-through layer. The cloud is seven overlapping,
  blurred lobes; the _cluster_ is what expands — animating lobes individually
  tears it into clumps. Both effects are decorative, aria-hidden, and stand
  down under `prefers-reduced-motion`.
- Icons are **lucide-react**; icon-only buttons need `btn-icon` plus `title`
  and `aria-label`.

## Conventions

- Section headers in code use `// MARK:` comments so they show up in the minimap.
- A React component's props `interface` is named after the component
  (`FooProps` for component `Foo`) and goes directly before the component it
  belongs to (a doc comment may sit between) — never separated by other
  declarations like constants or helpers.
- All input-field placeholders must start with `e.g.,`.
- Paths shown in the UI are abbreviated with `~` via `displayPath`
  (`renderer/src/format.ts`, backed by `tildify` in `shared/paths.ts`); keep the
  full path in the `title` tooltip. Inputs hold real, unabbreviated paths.
- **Theming**: the UI is a single skeuomorphic "brushed metal" appearance (there
  is no dark variant). The UI face is the system font (`--font-ui`, `system-ui`
  — San Francisco on macOS), the one deliberate departure from classic Aqua;
  name plates, paths and terminal output stay monospace. The
  `@media (prefers-color-scheme: dark)` block re-asserts the same look. Every
  color literal lives in the `:root` token block of
  `styles.css` (including gradient/texture/shadow-stack tokens); rules below it
  only reference tokens, `color-mix()` of tokens, and `rgba()` light/shade
  overlays — never hard-code a hex outside the token block. The native window
  background is the fixed desktop charcoal in `main/index.ts`. The `--grain`
  texture is the one non-literal: it is a **baked PNG pair** (1x/2x, chosen by
  `image-set`), not a live `feTurbulence` SVG, because the rasteriser re-runs a
  filter for every tile of every element carrying it — ~30 rules, including
  every button and every worktree row — which cost a third of the paint time of
  a window resize. `scripts/bake-grain.mjs` holds the filter and is the only
  place to retune the texture; re-run `pnpm bake-grain` rather than editing the
  PNGs.
- **Animation cost**: row flourishes animate `transform` and `opacity` only.
  Those are composited; anything else (`background-position`, `box-shadow`,
  `background-color`) repaints the row's whole grain-plus-gradient-plus-blend
  stack every frame — the arrival animation measured ~9x cheaper once moved onto
  compositable properties. Keep new animations on that pair.
- **Text selection**: `body` is `user-select: none` (native-app feel — labels,
  badges, and button captions aren't selectable). Content worth copying opts back
  in through the list in the "Text selection" block of `styles.css`; new
  copyable text goes on that list, or gets the `selectable` class. Never drop the
  global rule. The terminal and `.branch-select` are intentionally excluded (see
  the comment there).
- The IPC contract is the `WorktreeApi` interface in `src/shared/types.ts`; the
  channel-name map (`CH`) is duplicated in `ipc.ts` and `preload/index.ts` — keep
  them in sync.
- Prefer adding pure, testable helpers (like the parsers) over inline logic.
