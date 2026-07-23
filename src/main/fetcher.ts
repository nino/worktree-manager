import { getConfig } from "./store";
import { fetchRepo, hasRemote } from "./git";

// MARK: Periodic background fetch

/** How often to `git fetch` every configured repo: 8 minutes and 43 seconds. */
export const FETCH_INTERVAL_MS = (8 * 60 + 43) * 1000;

let timer: ReturnType<typeof setInterval> | null = null;
let running = false;

/**
 * Fetch every configured repo that has a remote, in parallel. Never throws:
 * one repo's failure (offline, auth prompt suppressed, etc.) can't abort the
 * others or the loop. Returns the number of repos that fetched successfully.
 */
async function fetchAllRepos(): Promise<number> {
  const { repos } = getConfig();
  const results = await Promise.allSettled(
    repos.map(async (repo) => {
      if (!(await hasRemote(repo.path))) return false;
      await fetchRepo(repo.path);
      return true;
    }),
  );

  let fetched = 0;
  for (let i = 0; i < results.length; i++) {
    const r = results[i];
    if (r.status === "fulfilled") {
      if (r.value) fetched++;
    } else {
      console.warn(`auto-fetch: ${repos[i]?.name ?? "repo"} failed:`, r.reason);
    }
  }
  return fetched;
}

/**
 * Run one fetch cycle now, then every FETCH_INTERVAL_MS. `onFetched` fires after
 * any cycle that fetched at least one repo, so the renderer can refresh
 * ahead/behind and unpushed counts against the newly updated remote refs.
 * Overlapping cycles are skipped (a slow network can't stack fetches).
 */
export function startAutoFetch(onFetched: () => void): void {
  if (timer) return;

  const cycle = async (): Promise<void> => {
    if (running) return;
    running = true;
    try {
      if ((await fetchAllRepos()) > 0) onFetched();
    } finally {
      running = false;
    }
  };

  void cycle();
  timer = setInterval(() => void cycle(), FETCH_INTERVAL_MS);
}

/** Stop the background fetch loop (idempotent). */
export function stopAutoFetch(): void {
  if (timer) clearInterval(timer);
  timer = null;
}
