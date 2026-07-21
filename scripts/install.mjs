// Copies the packaged app bundle from release/ into /Applications.
// macOS only — the packaged target is a .app bundle, so this refuses to run
// anywhere else. Run `pnpm dist` first to produce the bundle.
import { existsSync, readdirSync, rmSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { join, resolve } from "node:path";

const APPLICATIONS = "/Applications";

/** Print an error and exit non-zero. */
function fail(message) {
  console.error(`install: ${message}`);
  process.exit(1);
}

if (process.platform !== "darwin") {
  fail(
    `macOS only — cannot install a .app bundle on "${process.platform}". Run \`pnpm dist\` on a Mac.`,
  );
}

// electron-builder writes the bundle to release/mac* (mac-arm64, mac, …),
// so scan those for the first .app rather than hard-coding the arch.
const releaseDir = resolve("release");
if (!existsSync(releaseDir)) {
  fail("no release/ folder — run `pnpm dist` to build the app first.");
}

const appPath = readdirSync(releaseDir)
  .filter((name) => name.startsWith("mac"))
  .map((macDir) => join(releaseDir, macDir))
  .flatMap((dir) => readdirSync(dir).map((entry) => join(dir, entry)))
  .find((entry) => entry.endsWith(".app"));

if (!appPath) {
  fail("no .app bundle found under release/ — run `pnpm dist` to build the app first.");
}

const appName = appPath.slice(appPath.lastIndexOf("/") + 1);
const dest = join(APPLICATIONS, appName);

console.log(`Installing ${appName} → ${APPLICATIONS}`);
// Remove any existing install first so a stale file from an older version
// can't linger inside the bundle.
rmSync(dest, { recursive: true, force: true });
// `ditto` is the macOS-blessed way to copy a bundle: it preserves symlinks,
// metadata, and code signatures.
execFileSync("ditto", [appPath, dest], { stdio: "inherit" });
console.log(`Installed. Launch it from ${dest}`);
