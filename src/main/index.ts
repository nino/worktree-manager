import { existsSync } from "node:fs";
import { join } from "node:path";
import { app, BrowserWindow, nativeImage, shell } from "electron";
import { CH, registerIpc } from "./ipc";
import { stopAll } from "./commands";

// Screenshot/test sandbox: point config storage at a throwaway profile so
// tooling runs (scripts/screenshot.mjs) never touch the real user's config.
if (process.env.WTM_USER_DATA) app.setPath("userData", process.env.WTM_USER_DATA);

// MARK: Window

function createWindow(): void {
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

  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow();
  });
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") app.quit();
});

// Kill any commands still running so they don't outlive the app as orphans.
app.on("before-quit", () => stopAll());
