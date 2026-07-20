import { spawn } from "node:child_process";
import { shell } from "electron";
import { buildCommand } from "./command";
import { getConfig } from "./store";

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

/** Open a terminal at the given path using the globally-configured command. */
export function openInTerminal(targetPath: string): void {
  const { terminalCommand } = getConfig();
  const cmd = terminalCommand.trim();
  if (cmd) {
    runDetached(buildCommand(cmd, targetPath));
    return;
  }
  // Fallbacks when no terminal command is configured.
  if (process.platform === "darwin") {
    runDetached(buildCommand("open -a Terminal", targetPath));
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
