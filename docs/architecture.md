# Architecture

The UI is written once, in `wtm-ui`, as plain values: a description of the
window, its list, dialogs, popover, panels and menus. A thin toolkit layer
(`wtm-toolkit`) defines that vocabulary, and one backend per OS shows it with
that OS's own widgets:

| OS | crate | widgets |
| --- | --- | --- |
| macOS | `wtm-macos` | AppKit through objc2: `NSOutlineView`, `NSToolbar`, `NSAlert` sheets, `NSPopover`, SF Symbols |
| Linux | `wtm-gtk` | GTK 4 through gtk4-rs |
| Windows | `wtm-windows` | Win32 through the `windows` crate: common controls, owner-drawn rows |

All three aim at the lowest possible latency: the UI never waits on git, and
every repaint touches only the rows whose data changed.

```sh
cargo run                 # dev build, runs as a bare binary (menu bar + window)
cargo test                # unit tests + an end-to-end core test on a temp repo
xvfb-run -a cargo test    # the same on Linux, where the GTK tests need a display
scripts/bundle.sh         # macOS release build → target/bundle/Worktree Manager.app
scripts/bundle.sh --install   # …and copy it to /Applications
WTM_USER_DATA=/tmp/x cargo run   # sandboxed config dir (never touches real config)
scripts/monkey.sh         # macOS: random input on a debug build, in a sandbox, until it crashes
```

Config lives in the platform's app-data directory
(`~/Library/Application Support/Worktree Manager/config.json` on macOS,
`$XDG_CONFIG_HOME/worktree-manager/` on Linux, `%APPDATA%\Worktree Manager\`
on Windows), in the same JSON shape the Electron app this replaced used, and
that app's `worktree-manager.json` is imported on first launch if it is still
there.

## Crates

```
crates/
  wtm-platform   Traits every OS backend implements (spawn detached, open
                 terminal, reveal, app directories, the updater). No deps.
  wtm-core       Everything that is not a widget: data model, git runner and
                 parsers, create/delete safety ladder, push/pull/merge/switch,
                 config + startup snapshot + window state, background fetch,
                 file watcher, and the `App` facade. Depends only on
                 wtm-platform. Tested.
  wtm-toolkit    The wrapper: the `View` vocabulary, the `Program` and
                 `Backend` traits and the loop between them, a headless
                 backend for tests. Knows nothing of the app or any OS.
  wtm-ui         The UI, once: `Ui` is the `Program`. Turns the core's model
                 into a `View` and the view's messages into core `Action`s.
                 Tested headlessly.
  wtm-macos      The AppKit backend + the macOS `Platform` and updater.
  wtm-gtk        The GTK 4 backend + the Linux `Platform`.
  wtm-windows    The Win32 backend + the Windows `Platform`.
  wtm-app        The binary. Picks a backend by `cfg(target_os)` and hands
                 it, the core and the updater to `wtm_ui::run`.
```

Dependency direction is strictly `wtm-app → wtm-<os> → wtm-ui → wtm-toolkit`,
with `wtm-ui → wtm-core → wtm-platform` beside it. Neither the core nor the
UI names a toolkit, a thread of the UI, or an OS. Each backend crate is
`#![cfg(target_os = ...)]` as a whole, so the workspace builds everywhere and
only the matching backend has any code in it.

## One UI, three toolkits

**The loop.** `wtm-toolkit` is shaped like Elm. `Ui::view` returns the whole
UI as a `View`: the main window (toolbar, search, notice, a sectioned
`TreeList` of repo cards and their worktree rows, an empty state), at most one
open dialog first in a queue, the branch picker popover, panels such as
Settings, and the menus. Handlers in it are values that send a message.
`Backend::render` brings the native widgets in line with a view;
`Backend::perform` runs one-off `Effect`s (copy, a folder picker, focus,
scroll). A handler only queues its message; the queue is drained on the next
turn of the main loop, so the program never runs inside a native callback and
a backend is never asked to render while it is rendering. `Runtime::flush`
drains at once for the two places that must see the result before returning:
quitting, and menu validation.

**What a backend owes.** The contract is the same everywhere, and
`crates/wtm-gtk/tests/conformance.rs` checks the GTK backend against it:

- Report only what the user did. A row expanded, a field's text written or a
  selection moved *by a render* sends nothing back.
- Keep widgets across renders and patch them in place. A row is identified by
  its key (`r:<repo id>`, `w:<path>`, `p:<pending id>`), a dialog by its id,
  the popover by its id, a panel by its key; a field being edited is never
  rewritten under the cursor.
- Show only the first dialog in the queue, and never show again one that a
  button already closed, even if a render arrives before the program has
  taken it out.
- A popover or panel the view drops is closed without reporting `on_dismiss`
  or `on_close`; one the user closes reports it.
- Report host events: activation, the window's frame, the scroll offset, open
  requests from the OS, and quitting.

**Where the UI ends.** `wtm-ui` holds everything the window adds to the
model (the search, closed cards, the selection, open dialogs and their
drafts, the picker's query), and its tests drive it through the headless
backend. Anything an OS decides — sheets versus modal windows, how a popover
closes, which key is the menu shortcut modifier — stays in the backend.

## The macOS backend

The rest of this section is about `wtm-macos`. The GTK and Win32 backends
follow the same rules with their own widgets; the notes that differ are under
"Other backends" below.


**One immutable snapshot.** `wtm_core::Model` is a plain value (config, repo
nodes with their worktrees, per-worktree busy flags, pending creations, a
notice). `App::dispatch(Action)` never blocks: it applies whatever is knowable
right now to a copy of the model (a spinner flag, a placeholder row), publishes
it, and hands the slow part to a tokio runtime. Results come back the same
way. Listeners are called on whichever thread finished; the macOS backend
coalesces bursts with an atomic flag and hops to the main queue once.

**Targeted reloads.** The controller keeps one long-lived `WTMItem` object per
row key, so `NSOutlineView` identity (and thus expansion state) survives
updates. On each render it diffs the view's `TreeList` against the one it
last rendered and calls `reloadItem:` only for rows whose data differs.
Rows that a search query or a snapshot adds or drops are inserted and removed
in place (`wtm_core::splice` works out which), so every row that stays keeps
its view, and a closed card whose rows changed reloads its children. A splice
that names a row the outline does not have raises an AppKit exception, so
before one the controller compares the outline's rows, one by one, with the
tree it was last given, and falls back to `reloadData` when they differ, when
worktree rows were reordered, and for the first tree. A repo dragged to a new
place (`Action::MoveRepo`, which reorders the config) comes back as the same
roots in a new order, and `wtm_core::splice::moves` turns that into
`moveItemAtIndex:` calls — one for one repo moved — so the selection and
every row's view stay with their repo. The repo header's cell draws its own
drag image, its row rendered as a closed card: `NSTableCellView` builds one
from outlets this cell does not set, and the card is the row view's drawing,
not the cell's. Debug builds then
check that the outline's rows and heights match the tree after every update.
This is what keeps the search field responsive: in this outline, every
`reloadData` built all the visible rows' views anew, about 60 ms with 40
worktrees, while a held key repeats every 30 to 90 ms. Scrolling and
inserted rows take their views from `makeViewWithIdentifier:`'s reuse queue
and re-configure them in place (badge views are reused).

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
that fetched, and a full refresh runs when the app becomes active. Opening the
New Worktree sheet fetches every remote of its repo and re-lists it if a
remote-tracking ref moved; the sheet checks the name again when that listing
lands, so a branch pushed a minute ago is found. Fetches of one repo, and
making a worktree from a remote branch, take a per-repo lock (`lock_refs`), so
they never contend for a ref's lock file.

**Bounded git.** All git calls run through one runner with a 12-process
semaphore; a repo's worktree statuses are computed concurrently. Every process
gets `GIT_OPTIONAL_LOCKS=0` (status never rewrites the index, so our own
reads never trigger the watcher), `GIT_TERMINAL_PROMPT=0`, `GIT_EDITOR=true`.

**Same safety rules as the Electron app.** Create validates the ref with
`check-ref-format`, refuses existing paths and branches, and creates new
branches `--no-track`. A branch only one remote has is fetched first and
checked out `--track`ing it; one that several remotes have is refused in the
sheet, since which to track cannot be told. Delete verifies the path against
git's own list, refuses the primary tree, revalidates the branch the user saw,
and runs `git worktree remove` without `--force` first — a dirty tree comes
back as `Dirty` and the UI asks a second, destructive-styled question.
Mutations are serialised per worktree path.

## Other backends

**GTK.** `wtm-gtk` builds the list as a plain vertical box in a scrolled
window, one widget per row key, rather than a recycling `GtkListView`: the
list holds tens of rows, and a box keeps each row's widget for render to
patch. Cards are drawn by CSS (`style.rs`), some icons and the agent marks by
cairo; dialogs are modal windows on the main one, panels are windows of their
own, and the picker is a `GtkPopover`. GTK cannot place or read a toplevel's position under Wayland,
so `FrameChanged` carries the size with a position of zero, and only the
size is restored. The screen sizes it reports are whole monitors, not their work
areas. In a dialog Return activates the focused button rather than the
default one, as GTK does everywhere.

It can photograph itself, which is how a UI change is checked in a Linux
container: `WTM_SCREENSHOT=<png>` writes the window once the first listing
has settled, `WTM_SCREENSHOT_STEPS` drives the UI first (open a dialog, the
picker, Settings, type into a field), and `WTM_SCREENSHOT_QUIT=1` exits
afterwards. Under Xvfb set `GSK_RENDERER=cairo`. The `screenshot` skill has
the whole recipe.

**Win32.** `wtm-windows` is plain Win32 and common controls (comctl32 v6
through the manifest): the list is an owner-drawn window that paints the
cards, dialogs are modal windows owned by the main one, the picker is a
borderless popup, and the menus are a menu bar with accelerators on Ctrl.
It builds from Linux for `x86_64-pc-windows-gnu`.

## Adding a platform

1. Create `crates/wtm-<os>`, gated with `#![cfg(target_os = "...")]`, that
   implements `wtm_toolkit::Toolkit` and `Backend`, and
   `wtm_platform::Platform`, and exposes `fn app_dirs() -> AppDirs`.
2. Add a `#[cfg(target_os = "...")]` arm in `crates/wtm-app/src/main.rs`
   that builds the `App` with that platform and calls `wtm_ui::run`.

Nothing in `wtm-core` or `wtm-ui` changes. `wtm_toolkit::headless` is the
smallest backend there is and shows what `render` is handed;
`crates/wtm-gtk/tests/conformance.rs` is the list of behaviours to match.

**Updating itself (macOS).** `wtm-macos`'s `updater.rs` reads `appcast.json` from the release its
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
app, so their `worktree-manager.json` is imported by the config store.

Linux and Windows builds have no updater yet (`wtm_platform::NoUpdater`).

## Not carried over from the Electron app

The command runner with its terminal drawer, and the brushed-metal appearance
(this uses each platform's standard look, light and dark). Both are in git history if
they are wanted back.
