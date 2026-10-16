import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import type { Backend, MenuId } from "./backend";
import type { Boot } from "./wire/Boot";
import type { DeleteResult } from "./wire/DeleteResult";
import type { NewWorktreeCheck } from "./wire/NewWorktreeCheck";
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
    switchBranch: (repoId, path, branch) => send("switch_branch", { repoId, path, branch }),
    // Waited on: the answer decides whether there is a second question.
    deleteWorktree: (request) => invoke<DeleteResult>("delete_worktree", { request }),
    storeView: (view) => send("store_view", { view }),
    // Only macOS has a menu bar to update.
    setMenuState: boot.menuBar ? (state) => send("set_menu_state", { state }) : () => {},
    onMenu: (f) => {
      listen<MenuId>("menu", (e) => f(e.payload));
    },
    onFilesOver: (f) => {
      listen<boolean>("files-over", (e) => f(e.payload));
    },
    checkNewWorktree: (fields) => invoke<NewWorktreeCheck>("check_new_worktree", { fields }),
    createWorktree: (request) => send("create_worktree", { request }),
    fetchRepo: (repoId) => send("fetch_repo", { repoId }),
    dismissCreation: (id) => send("dismiss_creation", { id }),
    addRepos: () => send("add_repos"),
    pickFolder: () => invoke<string | null>("pick_folder"),
    updateRepo: (edit) => send("update_repo", { edit }),
    removeRepo: (repoId) => send("remove_repo", { repoId }),
    moveRepo: (repoId, shown, gap) => send("move_repo", { repoId, shown, gap }),
    setSettings: (settings) => send("set_settings", { settings }),
  };
}
