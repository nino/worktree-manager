import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import type { Backend } from "./backend";
import type { Boot } from "./wire/Boot";
import type { Model } from "./wire/Model";

// Commands answer nothing the page waits on: their results come back as a
// new model. A failure here is a bug in the bridge, not something to show.
function send(cmd: string, args?: Record<string, unknown>) {
  invoke(cmd, args).catch((e) => console.error(`${cmd} failed`, e));
}

export function tauriBackend(boot: Boot): Backend {
  return {
    boot,
    ready: () => invoke<Model>("ready"),
    onModel: (f) => {
      listen<Model>("model", (e) => f(e.payload));
    },
    refresh: () => send("refresh"),
    refreshRepo: (repoId) => send("refresh_repo", { repoId }),
    clearNotice: () => send("clear_notice"),
    openInEditor: (path) => send("open_in_editor", { path }),
    openInTerminal: (path) => send("open_in_terminal", { path }),
    reveal: (path) => send("reveal", { path }),
    push: (repoId, path) => send("push", { repoId, path }),
    pull: (repoId, path) => send("pull", { repoId, path }),
    pullMain: (repoId, path) => send("pull_main", { repoId, path }),
  };
}
