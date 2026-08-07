import { existsSync } from "node:fs";
import { join } from "node:path";
import { app, BrowserWindow, nativeImage, shell } from "electron";
import { CH, publishDropResult, registerIpc } from "./ipc";
import { stopAll } from "./commands";
import { startAutoFetch, stopAutoFetch } from "./fetcher";
import { addRepos } from "./repos";

// Screenshot/test sandbox: point config storage at a throwaway profile so
// tooling runs (scripts/screenshot.mjs) never touch the real user's config.
if (process.env.WTM_USER_DATA) app.setPath("userData", process.env.WTM_USER_DATA);

// MARK: Window

/** The app's single window, or null while none is open (macOS keeps running). */
let mainWindow: BrowserWindow | null = null;

function createWindow(): BrowserWindow {
  const win = new BrowserWindow({
    width: 1080,
    height: 760,
    minWidth: 640,
    minHeight: 420,
    show: false,
    title: "Worktree Manager",
    // Frameless: no native title bar or traffic lights, so we draw the Aqua
    // gems and brushed-metal chrome ourselves. Rounded corners match the
    // Mac OS X metal window shape.
    frame: false,
    roundedCorners: true,
    // Dark desktop charcoal (--desktop-lo) regardless of system theme —
    // brushed metal has one appearance.
    backgroundColor: "#414144",
    webPreferences: {
      preload: join(__dirname, "../preload/index.mjs"),
      sandbox: false,
      contextIsolation: true,
      nodeIntegration: false,
    },
  });

  mainWindow = win;
  win.once("ready-to-show", () => win.show());

  // Mirror the window's activation state to the renderer so it can drain the
  // Aqua gems to grey and fade the etched titles when the app isn't frontmost,
  // the way brushed-metal windows render in the background.
  const sendFocus = (): void => {
    if (!win.isDestroyed()) win.webContents.send(CH.windowFocusChanged, win.isFocused());
  };
  win.on("focus", sendFocus);
  win.on("blur", sendFocus);
  win.webContents.on("did-finish-load", sendFocus);

  // Periodically `git fetch` every repo in the background; nudge the renderer to
  // refresh its trees after any cycle that fetched something.
  startAutoFetch(() => {
    if (!win.isDestroyed()) win.webContents.send(CH.reposChanged);
  });
  win.on("closed", () => {
    if (mainWindow === win) mainWindow = null;
    stopAutoFetch();
  });

  // Open external links in the default browser, not inside the app.
  win.webContents.setWindowOpenHandler(({ url }) => {
    void shell.openExternal(url);
    return { action: "deny" };
  });

  if (process.env.ELECTRON_RENDERER_URL) {
    void win.loadURL(process.env.ELECTRON_RENDERER_URL);
  } else {
    void win.loadFile(join(__dirname, "../renderer/index.html"));
  }

  return win;
}

/** The existing window, or a fresh one (the app can be running window-less). */
function ensureWindow(): BrowserWindow {
  if (mainWindow && !mainWindow.isDestroyed()) return mainWindow;
  return createWindow();
}

// MARK: Dock drops

/**
 * Folders dropped on the Dock icon are added as repositories.
 *
 * macOS delivers one `open-file` per dropped item, and can do so *before*
 * `whenReady` when the drop is what launched the app — so the listener is
 * registered at module scope and paths are buffered until the app is ready.
 * A short debounce batches the events of a single multi-item drop into one
 * `addRepos` call, so the UI reports "Added a, b" rather than one banner each.
 * Dropping is only offered on macOS (see CFBundleDocumentTypes in
 * electron-builder.yml); it is a no-op elsewhere.
 */
const droppedPaths: string[] = [];
let dropTimer: NodeJS.Timeout | null = null;

app.on("open-file", (event, path) => {
  // Claim the path: without preventDefault macOS logs a "failed to open" error.
  event.preventDefault();
  droppedPaths.push(path);
  scheduleDropFlush();
});

function scheduleDropFlush(): void {
  if (!app.isReady() || dropTimer) return;
  dropTimer = setTimeout(() => {
    dropTimer = null;
    void flushDrops();
  }, 100);
}

async function flushDrops(): Promise<void> {
  const paths = droppedPaths.splice(0, droppedPaths.length);
  if (paths.length === 0) return;

  const win = ensureWindow();
  if (win.isMinimized()) win.restore();

  // Never let a bad drop take the app down; addRepos already reports per-path
  // failures, so anything thrown here is unexpected.
  const result = await addRepos(paths).catch((error: unknown) => {
    console.error("Failed to add dropped repositories:", error);
    return null;
  });
  if (result) publishDropResult(result);
}

// MARK: App lifecycle

app.whenReady().then(() => {
  // In dev the dock uses Electron's default icon (packaged builds get it from
  // the bundle .icns). Point it at build/icon.png when running from source.
  if (process.platform === "darwin" && app.dock) {
    const devIcon = join(process.cwd(), "build", "icon.png");
    if (existsSync(devIcon)) {
      const image = nativeImage.createFromPath(devIcon);
      if (!image.isEmpty()) app.dock.setIcon(image);
    }
  }

  registerIpc();
  createWindow();

  // Drops that arrived during launch were buffered until now.
  scheduleDropFlush();

  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow();
  });
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") app.quit();
});

// Kill any commands still running so they don't outlive the app as orphans, and
// stop the background fetch loop.
app.on("before-quit", () => {
  stopAll();
  stopAutoFetch();
});
