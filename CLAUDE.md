# Worktree Manager

A native macOS app for managing git worktrees across multiple repositories.
The main window is a tree view: repositories at the top level, their worktrees
nested underneath, each with git status and quick actions.

It began as an Electron app; that version was replaced by this Rust one and
deleted. Anything the old app did that this one does not is in git history
(the last Electron commit is the parent of the rewrite's first).

## Stack

- **Rust** (2021 edition), a Cargo workspace of four crates
- **AppKit through [objc2](https://docs.rs/objc2)** — real `NSOutlineView`,
  `NSToolbar`, `NSAlert` sheets, `NSPopover`, SF Symbols. No web view, no
  cross-platform toolkit
- **tokio** for the git work, **notify** for FSEvents, **serde** for the config
- **rustfmt** defaults for formatting; no clippy config

## Commands

```sh
cargo run                      # dev build (menu bar + window)
cargo test                     # unit tests + an end-to-end core test on a temp repo
cargo fmt                      # format
scripts/bundle.sh              # release build → target/bundle/Worktree Manager.app
scripts/bundle.sh --install    # …and copy it to /Applications
```

Environment switches, all dev-only:

| variable | effect |
| --- | --- |
| `WTM_USER_DATA=<dir>` | config + snapshot live there instead of the real profile |
| `WTM_APPEARANCE=dark\|light` | force an appearance without changing the system setting |
| `RUST_LOG=info` | timings for refreshes and git runs |

**Never test against the real config.** Use `WTM_USER_DATA` pointed at a
throwaway directory and demo repos created for the purpose. The real profile is
`~/Library/Application Support/Worktree Manager/`.

## Layout

```
crates/
  wtm-platform     Traits an OS backend implements (spawn detached, open
                   terminal, reveal, app directories). No dependencies.
  wtm-core         Everything that is not a widget: types, git runner and
                   parsers, create/delete/push/pull/switch, config store,
                   snapshot, background fetch, file watcher, fuzzy matching,
                   branch-prefix splitting, and the `App` facade. Tested.
  wtm-macos        The AppKit UI and the macOS `Platform` implementation.
  wtm-app          The binary; the only place with `cfg(target_os)`.
bundle/Info.plist  Bundle metadata (id uk.org.plinth.worktree-manager-native)
build/             Icon sources (icon.icns, Assets.car) copied into the bundle
scripts/bundle.sh  Builds the .app
docs/architecture.md  How it fits together, and why the fiddly parts are so
```

Dependency direction is strictly `wtm-app → wtm-macos → wtm-core →
wtm-platform`. The core never names AppKit, a thread of the UI, or an OS.

`wtm-macos` modules: `controller` (window, outline data source/delegate,
selection, key loop), `cells` (row cell views), `rowview` (the card drawing),
`outline` (NSOutlineView subclass), `button` (focusable buttons, the branch
bezel, icon buttons), `picker` (branch popover), `branchlabel` + `toolicon`
(agent marks), `tooltip`, `dialogs` (sheets), `settings` (its own window),
`badge`, `items`, `menu`, `platform`, `util`.

## Key behaviors

- **One immutable snapshot.** `wtm_core::Model` is a plain value; `App::dispatch`
  never blocks. It applies what is knowable now (a spinner, a placeholder row),
  publishes, and hands the slow part to tokio. See `docs/architecture.md`.
- **Config** is JSON in the same shape the Electron app used, and that app's
  file is imported on first launch if present.
- **Worktree paths**: `<worktrees root>/<repo name>/<branch-slug>`.
- **New-branch base ref** defaults to `origin/<trunk>` when that remote-tracking
  ref exists, else the local trunk; new branches are created `--no-track`.
- **Delete is a safety ladder**: the path is checked against git's own worktree
  list, the primary tree is refused, the branch is revalidated against what the
  row showed, and `git worktree remove` runs without `--force` first — a dirty
  tree comes back as `Dirty` and the UI asks a second, destructive question.
- **Status** per worktree: staged / unstaged / untracked, ahead/behind the
  repo's trunk (against `origin/<trunk>` when it exists), unpushed commits.
- **Open in terminal** uses the Launch Services handler for
  `public.unix-executable` — what a terminal registers as "default terminal" —
  and falls back to Terminal.app. The editor command is configurable and takes
  a `{path}` placeholder.
- **Branch labels**: a `claude/` or `cursor/` prefix is drawn as that agent's
  mark. The full name is what gets matched, dispatched, and read out by
  assistive technology (`setAccessibilityLabel`).
- **Releases**: pushing to `main` builds, signs, notarises and publishes a
  rolling `latest` release; installed copies update themselves from it. See
  "Releases" below.
- **The updater** (`updater.rs`) only runs for an installed Developer ID build
  with a stamped version — a local build has no Team ID to match and its
  `0.1.0` placeholder would treat every release as an upgrade. It refuses any
  download whose hash, signature, Team ID or notarisation does not check out.
  `swap` moves the old bundle aside before putting the new one in place, and
  is unit-tested; so is the signature check, against a notarised app on the
  machine.

## objc2 and AppKit notes (learned the hard way)

- **No `?` or early `return` inside a `define_class!` method body.** The macro
  wraps them; put the logic in a plain `impl` method and call that.
- **A class with Rust ivars cannot be built by AppKit's factory methods**
  (`buttonWithTitle:` and friends): they allocate without `set_ivars`, so the
  ivars are uninitialised memory. Construct with `initWithFrame:` and configure
  by hand, or leave the class ivar-free.
- **`NSGradient` angles are in unflipped coordinates.** Table rows are flipped,
  so 90 fades downwards on screen and −90 upwards.
- **Row views do not clip their drawing.** A card slice drawn past the row
  bounds is visible over the next row.
- **AppKit never redraws a row it is animating away.** On collapse it moves
  the row views into a clip view; anything drawn with `drawRect:` disappears
  for the animation. Rows are snapshotted to their layer first (`set_snapshot`).
- **`setAutoresizesOutlineColumn(false)`**, or the outline column grows by the
  deepest indentation and rows end up wider than the window.
- **Note height changes off the notification.** Doing it inside an expand or
  collapse delegate callback re-enters the table; dispatch to the next turn.
- **Font weight constants** (`NSFontWeightMedium`, …) are extern statics;
  `util::{REGULAR, MEDIUM, SEMIBOLD}` holds the documented numbers instead.
- **The system tooltip delay cannot be shortened**, which is why row icons draw
  their own (`tooltip.rs`) and set no `toolTip`.

## Conventions

- Section headers in code use `// MARK:` comments.
- Comments say *why*, and are worth writing for anything an AppKit quirk forced.
  Do not narrate what the next line plainly does.
- Prefer pure, tested helpers in `wtm-core` (parsers, fuzzy matching, path
  building, notice folding) over logic embedded in a view.
- Placeholders in text fields start with `e.g.,`.
- Paths shown in the UI are abbreviated with `~` (`wtm_core::paths::tildify`);
  the full path goes in the tooltip.
- Every new control belongs in the Tab loop: use `button::Button` (or a
  subclass), which takes focus without Full Keyboard Access.
- Icons need a hint: `IconButton::set_hint` sets both the tooltip and the
  accessibility help.
- Colours are system colours or blends of them, so both appearances work.
  Check with `WTM_APPEARANCE`.

## Releases

Every push to `main` — or a manual run from the Actions tab — builds a
universal (arm64 + x86_64) app, signs it with a Developer ID certificate,
notarises and staples the app and the disk image, and replaces the rolling
`latest` GitHub release. The version is stamped as `1.0.<commits on main>`:
it only grows, and re-running the workflow on the same commit produces the
same version, so a rebuild is never mistaken for an update.

The release carries the DMG for people, and the zip plus `appcast.json` for
the in-app updater. Five repository secrets drive the signing, under
Settings → Secrets and variables → Actions:

| secret | what it is |
| --- | --- |
| `MACOS_CERTIFICATE_P12` | Developer ID Application certificate *and its private key*, exported as `.p12`, base64-encoded |
| `MACOS_CERTIFICATE_PASSWORD` | the password set during that export |
| `APPLE_API_KEY_P8` | the whole `.p8` App Store Connect key, `-----BEGIN PRIVATE KEY-----` included |
| `APPLE_API_KEY_ID` | the ten-character key ID |
| `APPLE_API_ISSUER_ID` | the issuer UUID |

Local `scripts/bundle.sh` builds are ad-hoc signed, which is enough for a
stable identity (window-frame autosave) but is not a Developer ID signature.
