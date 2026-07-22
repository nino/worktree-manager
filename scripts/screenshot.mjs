// Regenerates docs/screenshot.png from staged demo data, so the README image
// can be refreshed after any restyle with `pnpm screenshot`.
//
// What it does:
//   1. Builds throwaway demo git repos (with worktrees in a spread of states:
//      clean, ahead/behind, staged/unstaged/untracked, unpushed, no upstream)
//      under ~/wtm-demo so paths render with a tidy `~` in the UI.
//   2. Seeds a sandboxed electron-store profile in a temp dir and points the
//      app at it via WTM_USER_DATA — the real user config is never touched.
//   3. Launches the production build (out/) under Playwright, waits for the
//      rows and badges to render, and screenshots the window.
//   4. Cleans up the demo repos and the temp profile, success or failure.
//
// Run `pnpm screenshot` (which builds out/ first), not this file directly.
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync, appendFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { _electron as electron } from "playwright-core";

const ROOT = resolve(import.meta.dirname, "..");
const OUT_MAIN = join(ROOT, "out", "main", "index.js");
const SHOT = join(ROOT, "docs", "screenshot.png");

// Demo repos live under the real home dir purely so the UI shows `~/…` paths.
const DEMO = join(homedir(), "wtm-demo");
// Marker proving the folder is ours to delete — refuse to clobber anything else.
const MARKER = join(DEMO, ".wtm-demo");

function fail(message) {
  console.error(`screenshot: ${message}`);
  process.exit(1);
}

if (process.platform !== "darwin") fail("macOS only (launches the .app Electron binary).");
if (!existsSync(OUT_MAIN)) fail("no production build in out/ — run `pnpm screenshot`.");
if (existsSync(DEMO) && !existsSync(MARKER)) {
  fail(`${DEMO} exists but wasn't created by this script — move it out of the way first.`);
}

// MARK: Demo repo builders

const GIT_ENV = {
  ...process.env,
  GIT_AUTHOR_NAME: "Demo",
  GIT_AUTHOR_EMAIL: "demo@example.com",
  GIT_COMMITTER_NAME: "Demo",
  GIT_COMMITTER_EMAIL: "demo@example.com",
};

function git(cwd, ...args) {
  execFileSync("git", args, { cwd, env: GIT_ENV, stdio: ["ignore", "ignore", "inherit"] });
}

function write(dir, file, content) {
  writeFileSync(join(dir, file), content);
}

function commit(dir, message) {
  git(dir, "add", "-A");
  git(dir, "commit", "-q", "-m", message);
}

/** Init a repo with a bare "origin" so upstream/unpushed states are real. */
function initRepo(path, defaultBranch) {
  git(ROOT, "init", "-q", "-b", defaultBranch, path);
  const origin = join(DEMO, ".origins", `${path.split("/").pop()}.git`);
  git(ROOT, "init", "-q", "--bare", origin);
  git(path, "remote", "add", "origin", origin);
  return origin;
}

function buildDemoRepos() {
  rmSync(DEMO, { recursive: true, force: true });
  mkdirSync(join(DEMO, ".origins"), { recursive: true });
  mkdirSync(join(DEMO, "worktrees"), { recursive: true });
  writeFileSync(MARKER, "throwaway demo data for scripts/screenshot.mjs\n");

  // rocket-app: the busy repo — primary + three worktrees in varied states.
  const rocket = join(DEMO, "rocket-app");
  initRepo(rocket, "main");
  write(rocket, "README.md", "# rocket-app\n");
  mkdirSync(join(rocket, "src"));
  write(rocket, "src/main.ts", 'export const launch = () => "liftoff";\n');
  commit(rocket, "Initial scaffold");
  write(rocket, "src/fuel.ts", "export const fuel = 100;\n");
  commit(rocket, "Add fuel gauge");
  write(rocket, "src/telemetry.ts", "export const telemetry: number[] = [];\n");
  commit(rocket, "Wire telemetry");
  git(rocket, "push", "-q", "-u", "origin", "main");
  write(rocket, "src/stage2.ts", "export const stage2 = true;\n");
  commit(rocket, "Stage-2 separation logic");
  write(rocket, "src/heatshield.ts", 'export const heatShield = "ok";\n');
  commit(rocket, "Heat-shield checks");
  git(rocket, "push", "-q", "origin", "main");

  // feature/onboarding-flow: ahead 3 / behind 2, clean, no upstream.
  const onboarding = join(DEMO, "worktrees", "rocket-app", "feature-onboarding-flow");
  git(rocket, "worktree", "add", "-q", "-b", "feature/onboarding-flow", onboarding, "main~2");
  for (const step of ["welcome screen", "profile form", "sample project"]) {
    appendFileSync(join(onboarding, "onboarding.md"), `- ${step}\n`);
    commit(onboarding, `Onboarding: ${step}`);
  }

  // fix/login-crash: 1 unpushed commit, unstaged edit, untracked file.
  const loginFix = join(DEMO, "worktrees", "rocket-app", "fix-login-crash");
  git(rocket, "worktree", "add", "-q", "-b", "fix/login-crash", loginFix, "main");
  write(loginFix, "fix.md", "Guard against null session on resume.\n");
  commit(loginFix, "Guard against null session");
  git(loginFix, "push", "-q", "-u", "origin", "fix/login-crash");
  appendFileSync(join(loginFix, "fix.md"), "Handle expired refresh tokens too.\n");
  commit(loginFix, "Handle expired tokens");
  appendFileSync(join(loginFix, "src", "main.ts"), "// TODO: retry once\n");
  write(loginFix, "repro-notes.md", "1. sign in\n2. sleep laptop\n3. resume\n");

  // chore/upgrade-deps: staged change, behind 2, no upstream.
  const upgrade = join(DEMO, "worktrees", "rocket-app", "chore-upgrade-deps");
  git(rocket, "worktree", "add", "-q", "-b", "chore/upgrade-deps", upgrade, "main~2");
  write(upgrade, "deps.md", "- react 19\n- vite 7\n");
  git(upgrade, "add", "deps.md");

  // docs-site: a quiet single-worktree repo.
  const docs = join(DEMO, "docs-site");
  initRepo(docs, "main");
  write(docs, "index.md", "# Docs\n");
  commit(docs, "Docs scaffold");
  git(docs, "push", "-q", "-u", "origin", "main");

  // api-gateway: master-branch repo with one clean feature worktree.
  const api = join(DEMO, "api-gateway");
  initRepo(api, "master");
  write(api, "gateway.go", "package gateway\n");
  commit(api, "Bootstrap gateway");
  write(api, "ratelimit.go", "package gateway // token bucket\n");
  commit(api, "Rate limiting");
  git(api, "push", "-q", "-u", "origin", "master");
  const cache = join(DEMO, "worktrees", "api-gateway", "perf-cache-tuning");
  git(api, "worktree", "add", "-q", "-b", "perf/cache-tuning", cache, "master");
  write(cache, "cache.go", "package gateway // LRU response cache\n");
  commit(cache, "LRU response cache");

  return { rocket, docs, api };
}

// MARK: Sandboxed config profile

function seedProfile(repos) {
  const userData = mkdtempSync(join(tmpdir(), "wtm-shot-"));
  const config = {
    worktreesRoot: join(DEMO, "worktrees"),
    editorCommand: "code",
    repos: [
      {
        id: "demo-rocket",
        name: "rocket-app",
        path: repos.rocket,
        mainBranch: "main",
        initCommand: "pnpm install",
        commands: [
          { id: "dev", name: "dev", command: "pnpm dev" },
          { id: "test", name: "test", command: "pnpm test" },
        ],
      },
      {
        id: "demo-docs",
        name: "docs-site",
        path: repos.docs,
        mainBranch: "main",
        initCommand: "",
        commands: [],
      },
      {
        id: "demo-api",
        name: "api-gateway",
        path: repos.api,
        mainBranch: "master",
        initCommand: "",
        commands: [{ id: "serve", name: "serve", command: "go run ." }],
      },
    ],
  };
  // electron-store file name matches the store's `name` option in store.ts.
  writeFileSync(join(userData, "worktree-manager.json"), JSON.stringify(config, null, 2));
  return userData;
}

// MARK: Launch & capture

async function capture(userData) {
  const app = await electron.launch({
    executablePath: join(ROOT, "node_modules/electron/dist/Electron.app/Contents/MacOS/Electron"),
    args: [ROOT],
    env: { ...process.env, WTM_USER_DATA: userData },
    timeout: 30_000,
  });
  try {
    const page = await app.firstWindow();
    // Statuses are computed before listRepos resolves, so once rows and badges
    // exist the data is final; the extra beat lets paint/fonts settle.
    await page.waitForSelector(".wt-row", { timeout: 15_000 });
    await page.waitForSelector(".badge", { timeout: 15_000 });
    await page.waitForTimeout(800);
    mkdirSync(join(ROOT, "docs"), { recursive: true });
    await page.screenshot({ path: SHOT });
  } finally {
    await app.close().catch(() => {});
  }
}

// MARK: Main

let userData = null;
try {
  console.log(`Building demo repos in ${DEMO}…`);
  const repos = buildDemoRepos();
  userData = seedProfile(repos);
  console.log("Launching app and capturing…");
  await capture(userData);
  console.log(`Wrote ${SHOT}`);
} finally {
  rmSync(DEMO, { recursive: true, force: true });
  if (userData) rmSync(userData, { recursive: true, force: true });
}
