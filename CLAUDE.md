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
cargo test                     # unit tests, property tests + an end-to-end core test on a temp repo
cargo fmt                      # format
scripts/bundle.sh              # release build → target/bundle/Worktree Manager.app
scripts/bundle.sh --install    # …and copy it to /Applications
scripts/monkey.sh              # random keys and clicks on a debug build until it crashes
scripts/monkey.sh --minutes 10 # …as many runs as fit in ten minutes
scripts/monkey.sh --sandbox    # just the monkey's demo repos, to run the app against by hand
```

The property tests (`crates/wtm-core/tests/properties.rs`, proptest) feed the
pure core random input. For a longer hunt than `cargo test`'s 256 cases:
`PROPTEST_CASES=100000 cargo test --release -p wtm-core --test properties`.
Whatever one finds becomes a unit test next to the code it broke.

`scripts/monkey.sh` is for what only a debug build shows: objc2 checks each
`msg_send!`'s types only with debug assertions on, so a wrong signature panics
under `cargo run` and passes silently in a release build. It runs the app in a
throwaway sandbox, drives it through `scripts/monkey.js` (JXA: keys with
`CGEventPostToPid`, clicks with `CGEventPost`, positions from System Events),
and reports a crash, a panic or a hang with the seed that replays it. It needs
Accessibility and Screen Recording permission for the terminal, and moves the
real pointer while it runs; a panel in the bottom-right corner shows the run,
the step and the time left.

Environment switches, all dev-only:

| variable | effect |
| --- | --- |
| `WTM_USER_DATA=<dir>` | config, snapshot + window state live there instead of the real profile |
| `WTM_APPEARANCE=dark\|light` | force an appearance without changing the system setting |
| `WTM_NO_LAUNCH=1` | Open in Terminal, Reveal and Open in Editor (and a repo's init command) log instead of opening anything |
| `RUST_LOG=info` | timings for refreshes and git runs |

**Never test against the real config.** Use `WTM_USER_DATA` pointed at a
throwaway directory and demo repos created for the purpose. The real profile is
`~/Library/Application Support/Worktree Manager/`. With `WTM_USER_DATA` set,
the Electron file to import is looked for inside that directory too
(`worktree-manager.json`), never in the real one.

## Layout

```
crates/
  wtm-platform     Traits an OS backend implements (spawn detached, open
                   terminal, reveal, app directories). No dependencies.
  wtm-core         Everything that is not a widget: types, git runner and
                   parsers, create/delete/push/pull/switch, config store,
                   snapshot, window state, background fetch, file watcher,
                   fuzzy matching, branch-prefix splitting, git's
                   branch-name rules, and the `App` facade. Tested.
  wtm-macos        The AppKit UI and the macOS `Platform` implementation.
  wtm-app          The binary; the only place with `cfg(target_os)`.
bundle/Info.plist  Bundle metadata (id uk.org.plinth.worktree-manager —
                   the Electron app's; see "Releases")
build/             Icon sources (icon.icns, Assets.car) copied into the bundle
scripts/bundle.sh  Builds the .app
scripts/monkey.*   The random UI driver (see "Commands")
docs/architecture.md  How it fits together, and why the fiddly parts are so
```

Dependency direction is strictly `wtm-app → wtm-macos → wtm-core →
wtm-platform`. The core never names AppKit, a thread of the UI, or an OS.

`wtm-macos` modules: `controller` (window, outline data source/delegate,
selection, key loop), `cells` (row cell views), `rowview` (the card drawing),
`outline` (NSOutlineView subclass), `button` (focusable buttons, the branch
bezel, icon buttons), `picker` (branch popover), `branchlabel` + `toolicon`
(agent marks), `tooltip`, `dialogs` (sheets), `settings` (its own window),
`announcement` (the one-off note about the rewrite, shown until the
`SEEN_REWRITE_ANNOUNCEMENT` user default is set), `badge`, `items`, `menu`,
`platform`, `util`.

## Key behaviors

- **One immutable snapshot.** `wtm_core::Model` is a plain value; `App::dispatch`
  never blocks. It applies what is knowable now (a spinner, a placeholder row),
  publishes, and hands the slow part to tokio. See `docs/architecture.md`.
- **Config** is JSON in the same shape the Electron app used, and that app's
  file is imported on first launch if present.
- **The window comes back as it was**: its frame, the list's scroll offset,
  the focused row and which cards were closed live in `ui-state.json` beside
  the config, so `WTM_USER_DATA` sandboxes them too. Rows are remembered by
  identity, never by index. A frame less than half on any screen is ignored
  and the window centres instead.
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
- **Update channels**: stable follows the rolling `latest` release, beta the
  prerelease under the `beta` tag. Which one is followed is in the config
  (`updateChannel`) and in Settings. See "Releases" below.
- **The updater** (`updater.rs`) only runs for an installed Developer ID build
  with a stamped version — a local build has no Team ID to match and its
  `0.1.0` placeholder would treat every release as an upgrade. It refuses any
  download whose hash, signature, Team ID or notarisation does not check out.
  A copy that *nobody* can replace where it runs (App Translocation, a disk
  image) is caught before downloading, and the notice says a version is
  available and how to install it. A folder this account may not write to —
  `/Applications` for a standard account — is not that: the download is
  checked and held, the notice offers "Install and Restart", and the swap is
  run by an administrator through macOS's own authentication dialog, raised by
  `osascript`. The dialog only ever follows that button, never a background
  check, and the signature is checked again immediately before the privileged
  copy, because in between the bundle sits somewhere this account can write.
  `swap` moves the old bundle aside before putting the new one in place, and
  is unit-tested; so is the signature check, against a notarised app on the
  machine, and the command the administrator is asked to run.

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

Pushing the `beta` tag runs the same job for the beta channel:

```sh
git push --force origin HEAD:refs/tags/beta
```

It publishes a **prerelease** under the `beta` tag, from any branch, and never
touches the stable release — `releases/latest` skips prereleases, so nothing
published this way can reach someone on stable. A beta is versioned
`1.0.<commits>-beta.<run number>`, which sorts above the stable build of the
same commit and below the next one; because the run number always grows,
rebuilding a beta from an unchanged tree is still an update, which is what
makes the update path testable without inventing commits.

Each release carries the DMG for people, and the zip plus `appcast.json` for
the in-app updater.

The stable release also carries `latest-mac.yml`: the same zip, described the
way electron-updater reads it. Nothing in this app reads that file — it is
there for the copies still running the Electron app, which check this release
on a timer and, without it, get a 404 and stay on the build they are on
forever. They can take this app as an update because the bundle keeps their
identifier and is signed by the same team: Squirrel.Mac only installs a bundle
whose signature satisfies the running copy's designated requirement, and that
requirement names the identifier. That is why `CFBundleIdentifier` is
`uk.org.plinth.worktree-manager` and not something ending in `-native`. Both
the file and the identifier can be revisited once nobody is left on 1.0.86 —
the last Electron release, where the rewrite's first was 1.0.120.

Five repository secrets drive the signing, under
Settings → Secrets and variables → Actions:

| secret | what it is |
| --- | --- |
| `MACOS_CERTIFICATE_P12` | Developer ID Application certificate *and its private key*, exported as `.p12`, base64-encoded |
| `MACOS_CERTIFICATE_PASSWORD` | the password set during that export |
| `APPLE_API_KEY_P8` | the whole `.p8` App Store Connect key, `-----BEGIN PRIVATE KEY-----` included |
| `APPLE_API_KEY_ID` | the ten-character key ID |
| `APPLE_API_ISSUER_ID` | the issuer UUID |

Local `scripts/bundle.sh` builds are ad-hoc signed, which is enough for a
stable identity (the user defaults the announcement is marked seen in) but is
not a Developer ID signature.
