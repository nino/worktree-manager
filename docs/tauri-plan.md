# Tauri rewrite: staged plan

An experiment on the `claude/tauri-rewrite-xiwmjw` branch: the same app, with
the window drawn by a webview through [Tauri 2](https://tauri.app) instead of
three native toolkits. Nothing here is released; see "Not releasing" below.

## Why

The native backends (`wtm-macos`, `wtm-gtk`, `wtm-windows`, about 22,000
lines) cost the most to change, and the macOS one can only be run on a Mac.
A webview UI is written once, runs the same in a browser, and can be
built, tested and photographed in the Linux cloud container end to end.

## What carries over unchanged

| crate | fate |
| --- | --- |
| `wtm-core` | unchanged. `App`, `Action`, `Model`, git, config, snapshot, watcher, fetcher, the delete safety ladder, `new_worktree::check`, fuzzy matching, `ui_state` |
| `wtm-platform` | unchanged |
| `wtm-os` (new) | the three `Platform` impls, moved out of the native backends as they were. On Linux the webview is GTK 3 and must not share a process with `wtm-gtk`'s GTK 4, so the services had to stop living in a toolkit crate |
| `wtm-tauri` (new) | the Tauri binary: owns the `App`, turns `Model` into a JSON wire type, exposes `Action`s as commands, pushes each new model as an event |
| `ui/` (new) | the frontend: Solid + TypeScript + Vite, no CSS framework |
| `wtm-toolkit`, `wtm-ui`, `wtm-macos`, `wtm-gtk`, `wtm-windows`, `wtm-app` | kept, and still built and tested, until the Tauri app does everything they do; deleted in the last stage. Presentation logic in `wtm-ui` (badges, search matching, notice folding) is ported to TypeScript with its tests as each stage needs it |

## Shape

- **Rust owns the truth, the webview owns the view.** The webview never runs
  git or touches files. It receives the whole model on every change and
  sends commands back, the same contract `wtm-ui` has with the core today.
- **One wire type.** `wtm-tauri/src/wire.rs` defines what crosses the bridge
  and derives TypeScript for it (`ts-rs`) into `ui/src/wire.ts`. A test fails
  when the two drift.
- **Coalesced pushes.** The core's listener fires on any thread; it only
  wakes one emitter thread, which serialises the latest model and emits it.
  A burst of changes becomes one event. Solid's `reconcile` patches the store
  by key (repo id, worktree path), so only rows whose data changed re-render.
- **Startup.** The config and the last listing's snapshot are already on
  disk, read before the window exists. The window is created hidden, the
  first model is injected as an initialisation script (no IPC round trip
  before the first paint), the page renders it synchronously and calls
  `ready`, and only then is the window shown, so it opens with content and
  no white flash. Git starts listing in parallel. The bundle is one small
  JS file and one CSS file, served from memory by Tauri's custom protocol.
  `RUST_LOG=info` logs the time from process start to first paint.
- **Testing from the cloud.** `cargo test` covers the core and the bridge;
  `npm test` (Vitest) covers the frontend; `npm run dev` serves the UI to a
  browser with a mock backend, and the real app runs under Xvfb on Linux
  (`scripts/tauri-screenshot.sh`).

## Stages

1. **Read-only window (this PR).** `wtm-os` extracted; `wtm-tauri` and `ui/`
   scaffolded; the live repo and worktree list with status badges, search,
   the notice bar, refresh, and the row actions that need no dialog (open in
   editor / terminal / file manager, push, pull, merge trunk). Fast startup
   path, CI for the new crate and the frontend, the release guard.
2. **Row workflows.** Branch picker (switch), delete with its confirmation
   ladder, keyboard navigation and selection, closed cards.
3. **Sheets and settings.** New Worktree (with the remote-branch check from
   `new_worktree::check`), add / remove / reorder repos (drag), repo
   settings, app settings, `.worktreeinclude` copying.
4. **Window and menus.** `ui_state` (frame, scroll, focused row, closed
   cards), native menus and shortcuts, drag-and-drop of folders,
   accessibility pass, appearance check in light and dark.
5. **Shipping.** Bundle identifier `uk.org.plinth.worktree-manager`,
   Developer ID signing and notarisation in `release.yml`, the updater
   (keep `updater.rs`'s checks or move to `tauri-plugin-updater`), startup
   and memory measured against the native build. Then the native crates go.

Each stage is a draft PR into this branch's line of work; nothing merges to
`main` until Nino says so.

## Not releasing

Every push to `main` releases, and `release.yml` can also be run by hand on
any branch or by moving the `beta` tag. On this branch `release.yml` refuses
to run while `crates/wtm-tauri` exists, so neither a manual run nor a beta
tag on one of these commits can publish it.

## Costs to watch

- A webview costs memory: expect tens of megabytes more than the AppKit
  build. Measured in stage 5.
- Three webviews (WebKit on macOS and Linux, WebView2 on Windows) instead of
  three toolkits: far fewer differences, but not none.
