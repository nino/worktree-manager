import { execFileSync, spawn } from "node:child_process";
import { homedir } from "node:os";
import { join } from "node:path";
import { shell } from "electron";
import { buildCommand } from "./command";
import { getConfig } from "./store";
import { DEFAULT_TERMINAL_BUNDLE_ID, parseDefaultTerminalBundleId } from "./terminal";

/**
 * Run a command detached via the user's login shell so it inherits the
 * interactive PATH (needed for tools like `code` on macOS GUI launches).
 */
function runDetached(command: string): void {
  const loginShell = process.env.SHELL || "/bin/zsh";
  const child = spawn(loginShell, ["-lc", command], { detached: true, stdio: "ignore" });
  child.unref();
}

/** Open a path in the globally-configured editor. */
export function openInEditor(targetPath: string): void {
  const { editorCommand } = getConfig();
  const cmd = editorCommand.trim() || "code";
  runDetached(buildCommand(cmd, targetPath));
}

/**
 * Resolve the bundle id of the user's system-wide default terminal on macOS.
 *
 * The default terminal is the Launch Services handler for the
 * `public.unix-executable` content type (see `terminal.ts`). We read the user's
 * Launch Services database via `plutil` and fall back to Terminal.app when no
 * override has been set or anything goes wrong.
 */
function defaultTerminalBundleId(): string {
  const plist = join(
    homedir(),
    "Library/Preferences/com.apple.LaunchServices/com.apple.launchservices.secure.plist",
  );
  try {
    const json = execFileSync("plutil", ["-convert", "json", "-o", "-", plist], {
      encoding: "utf8",
    });
    const handlers = JSON.parse(json).LSHandlers;
    if (Array.isArray(handlers)) {
      const id = parseDefaultTerminalBundleId(handlers);
      if (id) return id;
    }
  } catch {
    // No override yet, unreadable plist, or plutil failure — use the fallback.
  }
  return DEFAULT_TERMINAL_BUNDLE_ID;
}

/**
 * Open the user's default terminal at the given path.
 *
 * On macOS we hand the folder to the default terminal's bundle via `open -b`
 * (without `-n`, so a running instance is reused rather than spawning a fresh
 * app instance). Opening a directory makes the terminal start a shell there.
 */
export function openInTerminal(targetPath: string): void {
  if (process.platform === "darwin") {
    const child = spawn("open", ["-b", defaultTerminalBundleId(), targetPath], {
      detached: true,
      stdio: "ignore",
    });
    child.unref();
    return;
  }
  if (process.platform === "win32") {
    spawn("cmd.exe", ["/c", "start", "cmd.exe"], {
      cwd: targetPath,
      detached: true,
      stdio: "ignore",
    }).unref();
    return;
  }
  // Linux: best-effort with x-terminal-emulator.
  const child = spawn("x-terminal-emulator", [], {
    cwd: targetPath,
    detached: true,
    stdio: "ignore",
  });
  child.unref();
}

/** Reveal a path in the OS file manager. */
export async function revealInFinder(targetPath: string): Promise<void> {
  const err = await shell.openPath(targetPath);
  if (err) shell.showItemInFolder(targetPath);
}
