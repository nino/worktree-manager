import { existsSync } from "node:fs";
import { join } from "node:path";
import { app, BrowserWindow, nativeImage, shell } from "electron";
import { registerIpc } from "./ipc";

// MARK: Window

function createWindow(): void {
  const win = new BrowserWindow({
    width: 1080,
    height: 760,
    minWidth: 640,
    minHeight: 420,
    show: false,
    title: "Worktree Manager",
    titleBarStyle: process.platform === "darwin" ? "hiddenInset" : "default",
    // Platinum gray regardless of system theme — Mac OS 9 has one appearance.
    backgroundColor: "#cccccc",
    webPreferences: {
      preload: join(__dirname, "../preload/index.mjs"),
      sandbox: false,
      contextIsolation: true,
      nodeIntegration: false,
    },
  });

  win.once("ready-to-show", () => win.show());

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
