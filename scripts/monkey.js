// Random key presses and clicks for scripts/monkey.sh, sent to a running
// debug build until it dies or the steps run out. Run it through that script,
// which makes the sandbox this assumes.
//
//   osascript -l JavaScript monkey.js <pid> <seed> <steps> <sandbox> <action log> <app log>
//                                     [<run> <runs> <deadline>]
//
// The last three are for the progress panel: this run's number, how many
// there are (0: as many as fit before the deadline), and the deadline in
// seconds since the epoch (0: none). At the deadline the run stops as if its
// steps had run out.
//
// Exit status: 0 every step ran; 1 the app is gone or panicked; 2 it never
// showed a window, or the driver kept failing; 4 a guard stopped the run (the
// last line of the action log says which).
//
// Keys go to the app's pid (CGEventPostToPid), so none can land in another
// app whatever has the focus. Clicks are real (CGEventPost) and are only sent
// where the frontmost window under the pointer is the app's own.

ObjC.import('AppKit');
ObjC.import('CoreGraphics');
ObjC.import('Foundation');
ObjC.import('signal');
ObjC.import('stdlib');

// The bridge will not turn anything JavaScript has into the `UniChar *` this
// takes (an array is silently dropped, and the key types its own letter), so
// it is rebound to take the bytes of an NSData.
ObjC.bindFunction('CGEventKeyboardSetUnicodeString', ['void', ['void *', 'unsigned long', 'void *']]);

// MARK: Keys

const KEY = {
  a: 0, f: 3, r: 15, t: 17, n: 45,
  ret: 36, tab: 48, space: 49, backspace: 51, escape: 53,
  home: 115, pageUp: 116, forwardDelete: 117, end: 119, pageDown: 121,
  left: 123, right: 124, down: 125, up: 126,
};
const CMD = 0x100000, SHIFT = 0x20000, OPT = 0x80000;

// The only ⌘ shortcuts sent. Left out on purpose: ⌘V pastes the clipboard
// into whatever field has focus; ⌘O opens a panel that could add a real repo
// to the sandbox's config; ⌘, opens Settings, where the worktrees root is;
// ⌘Q, ⌘W, ⌘H and ⌘M take the window away; ⌘C and ⌘X overwrite the clipboard.
// Key codes are physical keys, and these five sit in the same place on every
// Latin layout.
const SHORTCUTS = [
  ['⌘N', KEY.n], ['⌘T', KEY.t], ['⌘R', KEY.r], ['⌘F', KEY.f], ['⌘A', KEY.a],
];

// Typed text: what branch names and paths are made of, plus characters that
// change length when lowercased or take more than one UTF-16 unit. No space:
// it presses whatever button has the focus, and Tab can put the focus on
// Settings, which would then take the rest of the text as its worktrees root.
// A space only ever comes alone (`plainKey`), so the guards run before
// anything else arrives in what it opened.
const ALPHABET = 'abcdefghijklmnopqrstuvwxyz0123456789/-._éİßẞ😀#+@~';

// MARK: State

let pid, seed, steps, rand, sandbox, actionLog, appLog;
let runNumber = 1, runCount = 1, deadline = 0;
let se, proc;
let step = 0;
let started = Date.now();
let logLength = 0;
let startConfig;
let targets = { rows: [], buttons: [], toolbar: [], avoid: [], sheet: [], surveyedAt: -1000, buttonsAt: -1000 };
// Sheets on the main window as of the last guard check, and as of the last
// survey: a sheet that has come or gone since means the targets are stale.
let sheetCount = 0;

function run(argv) {
  if (argv.length < 6) {
    console.log('usage: monkey.js <pid> <seed> <steps> <sandbox> <action log> <app log>');
    $.exit(2);
  }
  pid = parseInt(argv[0], 10);
  seed = parseInt(argv[1], 10);
  rand = mulberry32(seed);
  steps = parseInt(argv[2], 10);
  sandbox = argv[3];
  actionLog = argv[4];
  appLog = argv[5];
  if (argv.length >= 9) {
    runNumber = parseInt(argv[6], 10);
    runCount = parseInt(argv[7], 10);
    deadline = parseInt(argv[8], 10);
  }
  showHud();
  $.NSFileManager.defaultManager.createFileAtPathContentsAttributes(actionLog, $(), $());
  startConfig = JSON.parse(readFile(sandbox + '/config.start.json'));

  se = Application('System Events');
  // A new binary's first launch waits for the system's security scan.
  if (!waitForWindow(90)) {
    record('STOP: no window within 90s');
    $.exit(alive() ? 2 : 1);
  }
  record('window up');
  proc = se.processes.whose({ unixId: pid })[0];
  // A fixed frame, so the same seed clicks the same places.
  mainWindow().position = [60, 60];
  mainWindow().size = [1000, 760];
  sleep(0.5);

  let failures = 0;
  for (step = 1; step <= steps; step++) {
    if (deadline && Date.now() / 1000 >= deadline) finish(0, 'time is up');
    updateHud();
    try {
      checkApp();
      checkGuards();
      if (step - targets.surveyedAt >= 8 || sheetCount !== targets.sheetCount) survey();
      act();
      failures = 0;
    } catch (e) {
      if (!alive()) finish(1, 'the app is gone');
      if (String(e).startsWith('STOP: ')) finish(4, String(e).slice(6));
      // Usually an element that went away between reading and using it.
      record('driver error: ' + e);
      if (++failures >= 15) finish(2, 'the driver failed 15 times in a row');
    }
    pause();
    if (step % 50 === 0) console.log('step ' + step + '/' + steps);
  }
  checkApp();
  finish(0, 'done');
}

function finish(status, why) {
  record((status === 0 ? '' : 'STOP: ') + why);
  $.exit(status);
}

// MARK: Health and guards

// Two sources, because neither is enough alone. Before AppKit has started,
// the process is not a running application yet; after it dies, it stays a
// zombie until the script reaps it, and `kill` still reports a zombie.
let registered = false;
function alive() {
  if ($.kill(pid, 0) !== 0) return false;
  const app = $.NSRunningApplication.runningApplicationWithProcessIdentifier(pid);
  if (app.isNil()) return !registered;
  registered = true;
  return !app.terminated;
}

// A panic on a tokio worker leaves the process running, so the log is read as
// well as the pid.
function checkApp() {
  if (!alive()) finish(1, 'the app is gone');
  if (step % 3 !== 0) return;
  const text = readFile(appLog);
  const fresh = text.slice(logLength);
  logLength = text.length;
  if (/panicked at|Terminating app due to uncaught exception/.test(fresh)) {
    finish(1, 'the app panicked');
  }
}

// Stop before anything outside the sandbox can be touched: the worktrees
// root and the repo list must stay as the script wrote them (repos may go,
// none may arrive). And no Add Repo panel, which is how a real folder would
// get in. Clicks stay off its toolbar button, but Tab can focus that button
// and Space press it, and a narrow window moves it into the toolbar's
// overflow menu, so the check runs after every step. The same goes for
// Settings, which is closed as soon as it shows.
function checkGuards() {
  const config = JSON.parse(readFile(sandbox + '/profile/config.json') || '{}');
  if (config.worktreesRoot !== startConfig.worktreesRoot) {
    throw 'STOP: the worktrees root changed to ' + config.worktreesRoot;
  }
  const known = startConfig.repos.map((r) => r.path);
  for (const repo of config.repos || []) {
    if (!known.includes(repo.path)) throw 'STOP: a repo from outside the sandbox was added: ' + repo.path;
  }
  if (!mainWindow().exists()) throw 'STOP: the main window is gone';
  // AppKit describes an open panel's sheet as "open". Its view lives in
  // another process and ignores keys sent here, but takes an Accessibility
  // press of Cancel.
  let sheets = mainWindow().sheets.description();
  if (sheets.includes('open')) {
    mainWindow().sheets.whose({ description: 'open' })[0].uiElements[0].buttons.byName('Cancel').click();
    sleep(0.5);
    sheets = mainWindow().sheets.description();
    if (sheets.includes('open')) throw 'STOP: the Add Repo panel opened and would not close';
    record('cancelled the Add Repo panel');
  }
  sheetCount = sheets.length;
  // Settings holds the worktrees root; nothing here opens it on purpose.
  for (const w of windowList()) {
    if (w.kCGWindowOwnerPID === pid && w.kCGWindowName === 'Settings') {
      closeWindow('Settings');
      record('closed Settings');
    }
  }
}

// MARK: What there is to click

function survey() {
  targets.surveyedAt = step;
  const w = mainWindow();
  const outline = w.scrollAreas[0].outlines[0];
  const rowPos = outline.rows.position();
  const rowSize = outline.rows.size();
  targets.rows = rowPos.map((p, i) => rect(p, rowSize[i], 'row ' + i));

  // Row buttons take a second or two to read, so less often.
  if (step - targets.buttonsAt >= 40) {
    targets.buttonsAt = step;
    const pos = outline.rows.uiElements.buttons.position();
    const size = outline.rows.uiElements.buttons.size();
    const desc = outline.rows.uiElements.buttons.description();
    const title = outline.rows.uiElements.buttons.title();
    targets.buttons = [];
    pos.forEach((cells, r) => cells.forEach((buttons, c) => buttons.forEach((p, b) => {
      const name = title[r][c][b] || desc[r][c][b];
      targets.buttons.push(rect(p, size[r][c][b], 'row ' + r + ' "' + name + '"'));
    })));
  }

  const tb = w.toolbars[0].uiElements;
  const tbPos = tb.position(), tbSize = tb.size(), tbDesc = tb.description();
  targets.toolbar = [];
  targets.avoid = [];
  tbDesc.forEach((d, i) => {
    const r = rect(tbPos[i], tbSize[i], 'toolbar "' + d + '"');
    // Only Refresh and the search field (the group). Add Repo opens a file
    // panel, Settings holds the worktrees root, and in a narrow window the
    // overflow button's menu offers both.
    if (d === 'Refresh' || d === 'group') targets.toolbar.push(r);
    else targets.avoid.push(r);
  });
  // The window buttons close, minimise or full-screen the window.
  const f = frameOf(w);
  targets.avoid.push({ x: f.x, y: f.y, w: 90, h: 30, name: 'window buttons' });

  targets.sheet = [];
  const sheets = w.sheets();
  targets.sheetCount = sheets.length;
  if (sheets.length > 0) {
    for (const el of sheets[0].entireContents().slice(0, 40)) {
      try {
        const role = el.role();
        if (!/Button|TextField|ComboBox|PopUp|CheckBox|RadioButton|Row|Cell|Link/.test(role)) continue;
        const name = el.title() || el.description() || el.value() || role;
        const r = rect(el.position(), el.size(), 'sheet ' + role + ' "' + name + '"');
        // Removing a repo shrinks what is left to exercise.
        if (/Remove Repo/.test(name)) targets.avoid.push(r);
        else targets.sheet.push(r);
      } catch (e) {
        // Gone already.
      }
    }
    record('sheet: ' + targets.sheet.map((t) => t.name.replace(/^sheet /, '')).join(', '));
  }
}

// MARK: Actions

// Weighted: [weight, what]. Keys and clicks on known controls dominate; the
// rest shake layout and timing.
const ACTIONS = [
  [24, arrowKey],
  [14, plainKey],
  [6, shortcut],
  [10, typeText],
  [18, clickTarget],
  [10, clickAnywhere],
  [3, doubleClick],
  [6, scroll],
  [2, resize],
  [7, burst],
  [5, outsideChange],
];

// While a sheet is up the window behind it takes no clicks, so input goes
// to the sheet, or dismisses it; outside changes keep arriving underneath.
const SHEET_ACTIONS = [
  [35, clickSheet],
  [20, typeText],
  [14, plainKey],
  [8, arrowKey],
  [8, outsideChange],
  [5, burst],
  [10, dismiss],
];

function act() {
  const actions = sheetCount > 0 && targets.sheet.length > 0 ? SHEET_ACTIONS : ACTIONS;
  const total = actions.reduce((s, [w]) => s + w, 0);
  let n = rand() * total;
  for (const [w, f] of actions) {
    if ((n -= w) < 0) return f();
  }
}

function clickSheet() {
  const t = pick(targets.sheet);
  click(t.x + t.w * (0.2 + 0.6 * rand()), t.y + t.h * (0.2 + 0.6 * rand()), 1, t.name);
}

function dismiss() {
  key(KEY.escape);
  record('key escape (dismiss)');
}

function arrowKey() {
  const k = pick([KEY.up, KEY.down, KEY.left, KEY.right]);
  const mods = pick([0, 0, 0, 0, SHIFT, OPT, CMD]);
  key(k, mods);
  record('key ' + modName(mods) + keyName(k));
}

function plainKey() {
  const k = pick([KEY.ret, KEY.escape, KEY.tab, KEY.tab, KEY.space, KEY.space, KEY.backspace,
    KEY.forwardDelete, KEY.home, KEY.end, KEY.pageUp, KEY.pageDown]);
  const mods = k === KEY.tab && rand() < 0.4 ? SHIFT : 0;
  key(k, mods);
  record('key ' + modName(mods) + keyName(k));
}

function shortcut() {
  const [name, k] = pick(SHORTCUTS);
  key(k, CMD);
  record('key ' + name);
}

function typeText() {
  const chars = Array.from(ALPHABET);
  let text = '';
  const n = 1 + Math.floor(rand() * 8);
  for (let i = 0; i < n; i++) text += pick(chars);
  type(text);
  record('type ' + JSON.stringify(text));
}

function clickTarget() {
  // Rows scrolled out of sight are still in the outline; aim at what shows.
  const frames = ourFrames();
  const shown = (t) => frames.some((f) => inside(f, t.x + t.w / 2, t.y + t.h / 2));
  const pool = [].concat(targets.rows, targets.buttons, targets.buttons, targets.toolbar,
    targets.sheet, targets.sheet, targets.sheet).filter(shown);
  if (pool.length === 0) return clickAnywhere();
  const t = pick(pool);
  // Somewhere inside it, not always the middle.
  const x = t.x + t.w * (0.2 + 0.6 * rand());
  const y = t.y + t.h * (0.2 + 0.6 * rand());
  click(x, y, 1, t.name);
}

function clickAnywhere() {
  const f = ourFrames();
  if (f.length === 0) return;
  const w = pick(f);
  click(w.x + rand() * w.w, w.y + rand() * w.h, 1, 'window ' + (w.name || '?'));
}

// Double-clicking a title bar zooms or minimises the window (a system
// setting), so double clicks stay on rows.
function doubleClick() {
  if (targets.rows.length === 0) return;
  const t = pick(targets.rows);
  click(t.x + rand() * t.w, t.y + rand() * t.h, 2, t.name);
}

function scroll() {
  const f = frameOf(mainWindow());
  const x = f.x + 40 + rand() * (f.w - 80), y = f.y + 80 + rand() * (f.h - 120);
  if (!ours(x, y)) return record('skip scroll at ' + round(x) + ',' + round(y) + ': not our window');
  const lines = Math.round((rand() - 0.5) * 30);
  $.CGEventPost($.kCGHIDEventTap, $.CGEventCreateMouseEvent($(), $.kCGEventMouseMoved, { x: x, y: y }, 0));
  $.CGEventPost($.kCGHIDEventTap, $.CGEventCreateScrollWheelEvent2($(), 1, 1, lines, 0, 0));
  record('scroll ' + lines + ' at ' + round(x) + ',' + round(y));
}

function resize() {
  const width = 480 + Math.floor(rand() * 800), height = 260 + Math.floor(rand() * 600);
  mainWindow().size = [width, height];
  targets.surveyedAt = -1000;
  record('resize to ' + width + 'x' + height);
}

// Several inputs with no pause between them: collapse animations, popovers
// and sheets are where timing matters. No Space or Return, and no clicks on
// whatever they open: the guards do not run between these.
function burst() {
  const n = 2 + Math.floor(rand() * 5);
  record('burst of ' + n);
  for (let i = 0; i < n; i++) pick([arrowKey, arrowKey, clickTarget, shortcut])();
}

// MARK: Changes from outside

// What a terminal beside the app does to the same repos: files appear,
// commits land, branches switch, worktrees come and go. Each reaches the UI
// through the file watcher while it is in the middle of something else, which
// is when a row index taken before a reload is used after it.
function outsideChange() {
  // doShellScript hands back lines ending in CR.
  const trees = shell('ls -d ' + quote(sandbox) + '/worktrees/*/* ' + quote(sandbox) + '/repos/* 2>/dev/null || true')
    .split(/[\r\n]+/).filter(Boolean);
  if (trees.length === 0) return;
  const tree = pick(trees);
  const n = Math.floor(rand() * 1e6);
  const git = 'GIT_CONFIG_GLOBAL=' + quote(sandbox + '/gitconfig') + ' GIT_CONFIG_NOSYSTEM=1 git -C ' + quote(tree) + ' ';
  const extra = quote(sandbox + '/worktrees/ext') + '/*';
  const [what, command] = pick([
    ['new file', 'echo ' + n + ' > ' + quote(tree + '/scratch-' + n + '.txt')],
    ['remove new files', 'rm -f ' + quote(tree) + '/scratch-*.txt'],
    ['commit', git + 'commit -q --allow-empty -m ' + n],
    ['stage everything', git + 'add -A'],
    ['switch to a new branch', git + 'switch -q -c ext/' + n],
    ['add a worktree', git + 'worktree add -q -b ext/w' + n + ' ' + quote(sandbox + '/worktrees/ext/' + n)],
    // These two undo `add a worktree`, whichever tree was picked.
    ['remove a worktree', 'for d in ' + extra + '; do [ -d "$d" ] && git -C "$d" worktree remove --force "$d"; break; done'],
    ['delete a worktree folder', 'for d in ' + extra + '; do rm -rf "$d"; break; done'],
  ]);
  const where = /^(remove|delete) a worktree/.test(what) ? ' under worktrees/ext' : ' in ' + tree.slice(sandbox.length + 1);
  // Every path above is inside the sandbox; this is the check that it stays so.
  if (!tree.startsWith(sandbox + '/')) throw 'STOP: outside change aimed at ' + tree;
  shell('export GIT_CONFIG_GLOBAL=' + quote(sandbox + '/gitconfig') + ' GIT_CONFIG_NOSYSTEM=1; ('
    + command + ') >/dev/null 2>&1 || true');
  record('outside: ' + what + where);
}

const shellApp = Application.currentApplication();
shellApp.includeStandardAdditions = true;

function shell(command) {
  return shellApp.doShellScript(command);
}

function quote(s) {
  return "'" + s.replace(/'/g, "'\\''") + "'";
}

// MARK: Input

function key(code, flags) {
  activate();
  for (const down of [true, false]) {
    const e = $.CGEventCreateKeyboardEvent($(), code, down);
    $.CGEventSetFlags(e, flags || 0);
    $.CGEventPostToPid(pid, e);
  }
}

function type(text) {
  activate();
  for (const ch of Array.from(text)) {
    const utf16 = $(ch).dataUsingEncoding($.NSUTF16LittleEndianStringEncoding);
    for (const down of [true, false]) {
      const e = $.CGEventCreateKeyboardEvent($(), 0, down);
      $.CGEventKeyboardSetUnicodeString(e, utf16.length / 2, utf16.bytes);
      $.CGEventPostToPid(pid, e);
    }
  }
}

function click(x, y, count, what) {
  if (!ours(x, y)) return record('skip click at ' + round(x) + ',' + round(y) + ' (' + what + '): not our window');
  for (const a of targets.avoid) {
    if (inside(a, x, y)) return record('skip click on ' + a.name);
  }
  activate();
  const pt = { x: x, y: y };
  $.CGEventPost($.kCGHIDEventTap, $.CGEventCreateMouseEvent($(), $.kCGEventMouseMoved, pt, 0));
  for (let n = 1; n <= count; n++) {
    for (const type of [$.kCGEventLeftMouseDown, $.kCGEventLeftMouseUp]) {
      const e = $.CGEventCreateMouseEvent($(), type, pt, $.kCGMouseButtonLeft);
      $.CGEventSetIntegerValueField(e, $.kCGMouseEventClickState, n);
      $.CGEventPost($.kCGHIDEventTap, e);
    }
  }
  record((count === 2 ? 'double-click ' : 'click ') + round(x) + ',' + round(y) + ' ' + what);
}

// Keys go to the key window, and the app only has one while it is active.
function activate() {
  if ($.NSRunningApplication.runningApplicationWithProcessIdentifier(pid).active) return;
  proc.frontmost = true;
  sleep(0.2);
}

// Whether a click at this point would reach one of the app's windows: the
// frontmost window there must be the app's, and the point on screen. The
// Dock's full-screen window lets clicks through, and a transparent one is not
// there to hit.
function ours(x, y) {
  const screen = $.NSScreen.mainScreen;
  const full = screen.frame, vis = screen.visibleFrame;
  const top = full.size.height - vis.origin.y - vis.size.height;
  if (x < vis.origin.x || x > vis.origin.x + vis.size.width || y < top || y > top + vis.size.height) return false;
  for (const w of windowList()) {
    const b = w.kCGWindowBounds;
    if (x < b.X || x >= b.X + b.Width || y < b.Y || y >= b.Y + b.Height) continue;
    if (w.kCGWindowOwnerPID === pid) return true;
    // The progress panel ignores the mouse; a click there lands below it.
    if (w.kCGWindowOwnerPID === selfPid) continue;
    if (w.kCGWindowAlpha === 0) continue;
    if (w.kCGWindowOwnerName === 'Dock' && w.kCGWindowName === 'Dock') continue;
    return false;
  }
  return false;
}

function ourFrames() {
  return windowList()
    .filter((w) => w.kCGWindowOwnerPID === pid && w.kCGWindowLayer < 25)
    .map((w) => ({ x: w.kCGWindowBounds.X, y: w.kCGWindowBounds.Y, w: w.kCGWindowBounds.Width, h: w.kCGWindowBounds.Height, name: w.kCGWindowName }));
}

// On-screen windows, frontmost first.
function windowList() {
  const ref = $.CGWindowListCopyWindowInfo($.kCGWindowListOptionOnScreenOnly | $.kCGWindowListExcludeDesktopElements, 0);
  return ObjC.deepUnwrap(ObjC.castRefToObject(ref)) || [];
}

// MARK: Progress panel

// A small panel in the bottom-right corner saying which run and step this is
// and how long is left, for whoever is kept off their own mouse meanwhile. It
// never takes the focus and lets clicks through, so it cannot change what the
// app sees.
const selfPid = $.NSProcessInfo.processInfo.processIdentifier;
const hud = { panel: null, lines: [], last: '', startedAt: 0 };

function showHud() {
  const app = $.NSApplication.sharedApplication;
  app.setActivationPolicy(1); // accessory: no Dock icon, never frontmost
  const w = 380, h = 88;
  const vis = $.NSScreen.mainScreen.visibleFrame;
  const frame = $.NSMakeRect(vis.origin.x + vis.size.width - w - 16, vis.origin.y + 16, w, h);
  // Borderless (0) and non-activating (1 << 7).
  const panel = $.NSPanel.alloc.initWithContentRectStyleMaskBackingDefer(frame, 1 << 7, $.NSBackingStoreBuffered, false);
  panel.opaque = false;
  panel.backgroundColor = $.NSColor.clearColor;
  panel.hasShadow = true;
  panel.ignoresMouseEvents = true;
  panel.level = 25; // NSStatusWindowLevel: above the app's sheets and popovers
  panel.collectionBehavior = 1 | 16; // every Space, and stays put in Mission Control
  const box = $.NSBox.alloc.initWithFrame($.NSMakeRect(0, 0, w, h));
  box.boxType = 4; // custom: drawn from the fill colour and radius below
  box.titlePosition = 0;
  box.borderWidth = 0;
  box.cornerRadius = 10;
  box.contentViewMargins = $.NSMakeSize(0, 0);
  box.fillColor = $.NSColor.colorWithWhiteAlpha(0.1, 0.9);
  panel.contentView = box;
  const mono = $.NSFont.monospacedDigitSystemFontOfSizeWeight(12, 0);
  const styles = [
    [$.NSFont.boldSystemFontOfSize(12), 1],
    [mono, 1],
    [mono, 1],
    [$.NSFont.systemFontOfSize(11), 0.6],
  ];
  hud.lines = styles.map(([font, alpha], i) => {
    const label = $.NSTextField.labelWithString($(''));
    label.font = font;
    label.textColor = $.NSColor.colorWithWhiteAlpha(1, alpha);
    label.lineBreakMode = 4; // truncate the tail
    label.frame = $.NSMakeRect(14, h - 24 - i * 18, w - 28, 16);
    box.contentView.addSubview(label);
    return label;
  });
  hud.panel = panel;
  hud.startedAt = Date.now();
  updateHud();
  panel.orderFrontRegardless;
  pump();
}

function updateHud() {
  if (!hud.panel) return;
  const of = runCount > 0 ? ' of ' + runCount : '';
  const text = [
    'Monkey test running · hands off the mouse',
    'run ' + runNumber + of + ' · step ' + step + '/' + steps + ' · seed ' + seed,
    timeLeft() + ' · Ctrl-C in its terminal stops it',
    hud.last || 'starting the app',
  ];
  text.forEach((t, i) => { hud.lines[i].stringValue = $(t); });
  hud.panel.display;
  pump();
}

// Against the deadline when there is one; otherwise from the pace so far,
// over this run's remaining steps and the runs still to come.
function timeLeft() {
  if (deadline) {
    const left = Math.max(0, Math.round(deadline - Date.now() / 1000));
    return Math.floor(left / 60) + ':' + String(left % 60).padStart(2, '0') + ' left';
  }
  if (step < 10) return 'working out the pace';
  const perStep = (Date.now() - hud.startedAt) / 1000 / step;
  const left = perStep * ((steps - step) + (runCount - runNumber) * steps);
  return 'about ' + Math.max(1, Math.round(left / 60)) + ' min left';
}

// Lets the window server take what the panel drew.
function pump() {
  $.NSRunLoop.currentRunLoop.runUntilDate($.NSDate.dateWithTimeIntervalSinceNow(0.01));
}

// MARK: Timing

function sleep(seconds) {
  pump();
  delay(seconds);
}

// Mostly short: most bugs of this kind want input arriving mid-animation or
// before an async result lands. Now and then long enough for git to finish.
function pause() {
  const r = rand();
  if (r < 0.35) return;
  if (r < 0.7) return sleep(0.03 + rand() * 0.1);
  if (r < 0.92) return sleep(0.1 + rand() * 0.4);
  sleep(0.5 + rand() * 1.5);
}

// MARK: Helpers

function mainWindow() {
  return proc.windows.byName('Worktree Manager');
}

function waitForWindow(seconds) {
  for (let i = 0; i < seconds * 4; i++) {
    if (!alive()) return false;
    try {
      const p = se.processes.whose({ unixId: pid });
      if (p.length > 0 && p[0].windows.byName('Worktree Manager').exists()) return true;
    } catch (e) {
      // Not registered with System Events yet.
    }
    sleep(0.25);
  }
  return false;
}

function closeWindow(name) {
  const w = proc.windows.byName(name);
  w.buttons.whose({ subrole: 'AXCloseButton' })[0].click();
}

function frameOf(w) {
  const p = w.position(), s = w.size();
  return { x: p[0], y: p[1], w: s[0], h: s[1] };
}

function rect(p, s, name) {
  return { x: p[0], y: p[1], w: s[0], h: s[1], name: name };
}

function inside(r, x, y) {
  return x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h;
}

function readFile(path) {
  const s = $.NSString.stringWithContentsOfFileEncodingError($(path), $.NSUTF8StringEncoding, $());
  return s.isNil() ? '' : s.js;
}

function record(line) {
  hud.last = line;
  const t = ((Date.now() - started) / 1000).toFixed(2);
  const text = $('step ' + step + ' ' + t + 's ' + line + '\n');
  const h = $.NSFileHandle.fileHandleForWritingAtPath(actionLog);
  h.seekToEndOfFile;
  h.writeData(text.dataUsingEncoding($.NSUTF8StringEncoding));
  h.closeFile;
}

function pick(list) {
  return list[Math.floor(rand() * list.length)];
}

function round(v) {
  return Math.round(v);
}

function keyName(code) {
  return Object.keys(KEY).find((k) => KEY[k] === code) || String(code);
}

function modName(flags) {
  return (flags & CMD ? '⌘' : '') + (flags & OPT ? '⌥' : '') + (flags & SHIFT ? '⇧' : '');
}

// A small seeded generator: the same seed gives the same inputs.
function mulberry32(seed) {
  let a = seed >>> 0;
  return function () {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
