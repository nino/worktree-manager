# Architecture

**Real AppKit widgets** (`NSOutlineView`, `NSToolbar`, `NSAlert` sheets,
`NSPopover`, SF Symbols) driven from Rust through objc2, aimed at the lowest
possible latency: the UI never waits on git, and every repaint touches only the
rows whose data changed.

```sh
cargo run                 # dev build, runs as a bare binary (menu bar + window)
cargo test                # unit tests + an end-to-end core test on a temp repo
scripts/bundle.sh         # release build → target/bundle/Worktree Manager.app
scripts/bundle.sh --install   # …and copy it to /Applications
WTM_USER_DATA=/tmp/x cargo run   # sandboxed config dir (never touches real config)
```

Config lives in `~/Library/Application Support/Worktree Manager/config.json`,
in the same JSON shape the Electron app this replaced used, and that app's
`worktree-manager.json` is imported on first launch if it is still there.

## Crates

```
crates/
  wtm-platform   Traits every OS backend implements (spawn detached, open
                 terminal, reveal, app directories). ~60 lines, no deps.
  wtm-core       Everything that is not a widget: data model, git runner and
                 parsers, create/delete safety ladder, push/pull/merge/switch,
                 config + startup snapshot + window state, background fetch,
                 file watcher, and the `App` facade. Depends only on
                 wtm-platform. Tested.
  wtm-macos      AppKit UI through objc2 + the macOS `Platform` impl.
  wtm-app        The binary. The only place with `cfg(target_os)`: it picks a
                 backend and hands it to the core.
```

Dependency direction is strictly `wtm-app → wtm-macos → wtm-core → wtm-platform`.
The core never names AppKit, threads of the UI, or an OS.

## Architecture

**One immutable snapshot.** `wtm_core::Model` is a plain value (config, repo
nodes with their worktrees, per-worktree busy flags, pending creations, a
notice). `App::dispatch(Action)` never blocks: it applies whatever is knowable
right now to a copy of the model (a spinner flag, a placeholder row), publishes
it, and hands the slow part to a tokio runtime. Results come back the same
way. Listeners are called on whichever thread finished; the macOS backend
coalesces bursts with an atomic flag and hops to the main queue once.

**Targeted reloads.** The controller keeps one long-lived `WTMItem` object per
repo, worktree and pending creation, so `NSOutlineView` identity (and thus
expansion state) survives updates. On each snapshot it diffs against the one
it last rendered and calls `reloadItem:` only for rows whose data differs;
structure changes reload one repo's children; only a query change or a repo
added/removed triggers `reloadData`. Cell views are recycled through
`makeViewWithIdentifier:` and re-configured in place (badge views are reused).

**Branch picker.** The branch name in each plate is a small bezelled button
(`(detached)` for a detached HEAD) that opens a popover (`picker.rs`): a filter
field over a list ranked by `wtm_core::fuzzy` — the same subsequence match and
ordering as the Electron app — with ↑/↓, Return and Escape handled in the
field's delegate. Choosing dispatches `Action::Switch`. The popover is
`ApplicationDefined`, not `Transient`: a transient one closes on the
mouse-down and still lets the click reach the button, whose action reopens it
on the mouse-up, so clicking an open picker never closed it. A local event
monitor closes it instead, swallowing the click when it lands on the button
that opened it, and it also closes on Escape, on a click anywhere else, and
when the app is deactivated. A `claude/` or
`cursor/` prefix is drawn as that agent's mark (`toolicon.rs`, the same
16-unit grid as the web app's SVGs) by `branchlabel.rs`, in both the button
and the list; matching, tooltips and the dispatched value keep the whole name.

**Tooltips.** The row icons carry no label, so they explain themselves
quickly: `tooltip.rs` shows a small panel of its own 250ms after the pointer
settles on an `IconButton` (and at once while another tooltip is up), driven
by the buttons' tracking areas. The system's own tooltips take about a second
and a half and AppKit will not shorten that, so those buttons set no
`toolTip` at all.

**Keyboard.** The arrow keys move the outline's selection through repos and
worktrees; the row draws it as an accent-coloured glow around the header band
or the plate (`rowview::draw_selection`) rather than as a system highlight.
Space or ⌘T opens the selected worktree's branch picker and ⌘N creates a
worktree in the selected repo; the menu items are validated against the
selection. A popover hands the keyboard back to the tree when it closes
(`controller::focus_tree`), so the arrow keys keep working after a switch. Row
buttons are `button.rs`'s subclass: they accept focus without Full Keyboard
Access, and resolve their previous/next key view through the rows
(`controller::key_view`), bringing off-screen rows into view, so Tab walks
every button. Tab landing on the outline is forwarded to the first or last
row button.

**Settings.** `settings.rs` is a plain window, not a sheet: no Save or
Cancel, every edit is applied as it is typed.

**Cards, not a flat list.** `rowview.rs` gives each row a custom
`NSTableRowView` that draws its slice of a repo card (gradient header band with
a bevel hairline, straight sides, rounded shadowed bottom) and, for worktrees,
an inset plate. Rows stay virtualised and reused; the controller re-tags row
views after every structural change so the last plate closes the card.
Selection is off: every control lives in the rows. `WTM_APPEARANCE=dark|light`
forces an appearance for checking both looks.

**Expand and collapse, frame by frame.** Row heights never change when a card
opens or closes: the header is one height either way and the first and last
child rows carry the well's padding, so the outline's own slide animation is
never interrupted by a height note. The header flips to its open look in
`outlineViewItemWillExpand:`, before the first frame. Collapsing is harder:
AppKit moves the removed row views into a clip view and slides them away but
never lets them draw in there, so text and card slices would vanish for the
animation. `outlineViewItemWillCollapse:` therefore renders each row to a
bitmap that becomes the row layer's contents (`RowView::set_snapshot`), and the
header keeps its open look until the last of those rows is dropped from the
clip view (`viewWillMoveToSuperview:` → `child_row_leaving`), drawn in that
same frame.

**Instant launch.** The last listing is written to `snapshot.json`; at the
next launch the tree is on screen before any git process has started, then
replaced as real listings land (repo rows show a spinner until then).

**Where it was left.** `ui-state.json` holds the window frame, how far the
list was scrolled, which row had the keyboard and which cards were closed
(`wtm_core::ui_state`). It sits beside the config rather than in
NSUserDefaults or AppKit's window restoration, so `WTM_USER_DATA` sandboxes a
dev run's window along with its config, and the values are plain enough for a
backend on another OS to use. The controller records on every window move,
scroll, selection change and card opened or closed — the core drops an
unchanged value and writes a changed one 750ms after the first change not yet
written, so a burst costs one write per interval, with a synchronous flush
from `applicationWillTerminate:`. In full screen the frame from before it is
recorded instead, taken in `windowWillEnterFullScreen:` so that none of the
transition's frames is: the next launch opens a window, and one the size of
the screen is not the window that was left.

Earlier versions set the frame autosave name `WTMMainWindow`, so AppKit kept
the frame in the user defaults (`NSWindow Frame WTMMainWindow`) and restored
its size; `window.center()` replaced only the position. Until `ui-state.json`
has a frame, that one is used, through the same on-screen check, so the first
launch after the update keeps the window's size.

Restoring has two wrinkles. A frame is only reused when at least half of it
lands on a screen's visible frame, summed across screens so a window that
straddled two comes back straddling; a display that has been unplugged would
otherwise put the window somewhere it cannot be dragged back from. And the
focused row is stored by identity (repo id, or worktree path), never by index,
because the tree is rebuilt from a fresh listing: the state is applied once,
on the run-loop turn after the first tree that has its worktrees in it — from
the cached snapshot, or from the first listing when there is none — since the
outline's height only settles after its reload, and an offset clamped against
a one-row outline comes out at zero. Until it has been applied the saved
offset and row are recorded in place of the list's own, or the list sitting
at the top would be written over what is about to be restored; the frame and
the closed cards are live from the start. A pending creation is never
recorded: it is not there next time.

**Fresh without asking.** Each worktree directory (and, for linked worktrees,
its `.git/worktrees/<name>` metadata dir) is watched with FSEvents via
`notify`; events are debounced per worktree (350 ms) and re-read *that
worktree's* status only. A `git switch` in a terminal updates the right row.
Repos are also `git fetch --prune`d every 8m43s and re-listed after any cycle
that fetched, and a full refresh runs when the app becomes active.

**Bounded git.** All git calls run through one runner with a 12-process
semaphore; a repo's worktree statuses are computed concurrently. Every process
gets `GIT_OPTIONAL_LOCKS=0` (status never rewrites the index, so our own
reads never trigger the watcher), `GIT_TERMINAL_PROMPT=0`, `GIT_EDITOR=true`.

**Same safety rules as the Electron app.** Create validates the ref with
`check-ref-format`, refuses existing paths and branches, and creates new
branches `--no-track`. Delete verifies the path against git's own list, refuses
the primary tree, revalidates the branch the user saw, and runs
`git worktree remove` without `--force` first — a dirty tree comes back as
`Dirty` and the UI asks a second, destructive-styled question. Mutations are
serialised per worktree path.

## Adding a platform

1. Create `crates/wtm-<os>` implementing `wtm_platform::Platform` and exposing
   `fn app_dirs() -> AppDirs` and `fn run(app: wtm_core::App) -> !`.
2. Add a `#[cfg(target_os = "...")]` arm in `crates/wtm-app/src/main.rs`.

Nothing in `wtm-core` changes. The UI contract is: subscribe to `Event`, read
`App::model()`, call `App::dispatch(Action)`; `Action::DeleteWorktree` carries a
reply callback for the confirmation ladder. The macOS controller (`rebuild` in
`controller.rs`) is a reference for the snapshot-diffing approach.

**Updating itself.** `updater.rs` reads `appcast.json` from the release its
channel names — the rolling `latest` one for stable, the `beta` tag's
prerelease for beta (`UpdateChannel::feed_url`) — 10s after launch and every
six hours. Both are plain download URLs, so there is no API call and no token,
and `releases/latest` resolves to the newest release that is *not* a
prerelease, which is what keeps a beta out of the stable channel. The channel
is a setting; changing it checks the new feed at once rather than at the next
six-hourly tick. A newer version is downloaded, checked against the manifest's SHA-256,
unpacked, and then checked where it counts: `codesign --verify`, a Team ID
equal to the running copy's, and `spctl` reporting a notarised Developer ID
app. Only then is the bundle swapped — the old one is moved aside first and
removed once the new one is in place, so a failure leaves something that runs.
The running image is the old code until a restart, which the notice bar
offers.

A standard account cannot write to `/Applications`, which used to be the end
of it: the notice said a version was available and named the folder, and
nothing else happened. Now the download is checked and held in the work
directory, and the notice bar's button reads "Install and Restart". Pressing it
hands the same steps `swap` takes — copy in beside, move the old one aside, put
the new one in place, and put the old one back if that fails — to
`osascript`'s `do shell script … with administrator privileges`, which is what
raises macOS's authentication dialog; a standard account answers it with an
administrator's name and password. Two things are deliberate. The dialog
follows the button and never a background check, because a password prompt
nobody asked for is indistinguishable from a phishing attempt. And the
signature is verified a second time immediately before that command runs,
because between the first check and the privileged copy the bundle sits in a
directory this account can write to — what is authorised has to be what was
checked. None of this happens unless the running copy is itself an installed,
Developer-ID-signed build with a stamped version: a `cargo run` build has no
Team ID to match and would think every release is an upgrade.

The stable release carries one more file this app never reads:
`latest-mac.yml`, the same zip described for electron-updater. The Electron app
checks that release every six hours, and until the rewrite started publishing
the file, every check 404'd — those copies could see no update at all, least of
all this one. They can install this one because the bundle kept their
identifier (`uk.org.plinth.worktree-manager`) and is signed by the same team,
which between them satisfy the designated requirement Squirrel.Mac checks a
downloaded bundle against. Their first launch of it is a first launch of this
app: no `SEEN_REWRITE_ANNOUNCEMENT` default, so the note about the rewrite
comes up, and their `worktree-manager.json` is imported by the config store.

## Not carried over from the Electron app

The command runner with its terminal drawer, and the brushed-metal appearance
(this uses the standard macOS look, light and dark). Both are in git history if
they are wanted back.
