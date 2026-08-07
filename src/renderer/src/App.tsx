import { useCallback, useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { HelpCircle, RefreshCw, Settings, SquareTerminal } from "lucide-react";
import type { AddReposResult } from "@shared/types";
import { useConfig, useAddRepos, useRepos, keys } from "./queries";
import { summarizeAddResult, type Notice } from "./format";
import { api } from "./api";
import { useRuns } from "./runs";
import { RepoNode } from "./components/RepoNode";
import { SettingsDialog } from "./components/SettingsDialog";
import { HelpDialog } from "./components/HelpDialog";
import { TerminalDrawer } from "./components/TerminalDrawer";
import { GrowBox } from "./components/GrowBox";

/** Track whether the window is frontmost, so the UI can render the brushed-
 * metal "background window" look (grey gems, faded etching) when it isn't. */
function useWindowActive(): boolean {
  const [active, setActive] = useState(true);
  useEffect(() => api.onWindowFocusChange(setActive), []);
  return active;
}

/** Refetch the repo trees whenever the background fetch reports new remote refs. */
function useReposChangedRefresh(): void {
  const qc = useQueryClient();
  useEffect(
    () => api.onReposChanged(() => void qc.invalidateQueries({ queryKey: keys.repos })),
    [qc],
  );
}

/**
 * Report repos added by dropping folders on the Dock icon. The main process
 * handles those adds on its own, so this both drains the outcomes that landed
 * before this window was listening (a drop can launch the app) and subscribes
 * to later ones.
 */
function useDroppedRepos(onResult: (result: AddReposResult) => void): void {
  useEffect(() => {
    const unsubscribe = api.onReposDropped(onResult);
    void api.takeDroppedRepos().then((results) => results.forEach(onResult));
    return unsubscribe;
  }, [onResult]);
}

export function App() {
  const qc = useQueryClient();
  const config = useConfig();
  const repos = useRepos();
  const addRepos = useAddRepos();
  const active = useWindowActive();
  useReposChangedRefresh();
  const runs = useRuns();
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [helpOpen, setHelpOpen] = useState(false);
  const [notice, setNotice] = useState<Notice | null>(null);

  // Dock drops are added by the main process, so the trees have to be refetched
  // here rather than by a mutation's onSuccess.
  const onDropped = useCallback(
    (result: AddReposResult) => {
      setNotice(summarizeAddResult(result));
      void qc.invalidateQueries({ queryKey: keys.config });
      void qc.invalidateQueries({ queryKey: keys.repos });
    },
    [qc],
  );
  useDroppedRepos(onDropped);

  // A successful add is confirmed by the repo appearing in the tree, so let its
  // notice go; failures stay until the next add.
  useEffect(() => {
    if (notice?.tone !== "info") return;
    const timer = setTimeout(() => setNotice(null), 5000);
    return () => clearTimeout(timer);
  }, [notice]);

  const onAddRepo = async () => {
    const dirs = await api.pickDirectories("Select git repositories");
    if (dirs.length === 0) return;
    setNotice(null);
    const result = await addRepos.mutateAsync(dirs).catch((error: Error) => error);
    setNotice(
      result instanceof Error
        ? { tone: "error", text: result.message }
        : summarizeAddResult(result),
    );
  };

  return (
    <div className={active ? "app" : "app inactive"}>
      <header className="topbar">
        <div className="topbar-title">
          <span className="gems" role="group" aria-label="Window controls">
            <button
              className="gem gem-close"
              title="Close"
              aria-label="Close window"
              onClick={() => api.closeWindow()}
            />
            <button
              className="gem gem-min"
              title="Minimize"
              aria-label="Minimize window"
              onClick={() => api.minimizeWindow()}
            />
            <button
              className="gem gem-zoom"
              title="Zoom"
              aria-label="Zoom window"
              onClick={() => api.zoomWindow()}
            />
          </span>
          <span className="app-name">Worktree Manager</span>
        </div>
        <div className="topbar-actions">
          <button
            className="btn btn-sm btn-icon"
            title="Refresh"
            aria-label="Refresh"
            onClick={() => repos.refetch()}
            disabled={repos.isFetching}
          >
            <RefreshCw size={13} strokeWidth={1.75} className={repos.isFetching ? "spin" : ""} />
          </button>
          <button
            className="btn btn-sm btn-primary"
            onClick={onAddRepo}
            disabled={addRepos.isPending}
          >
            + Add repo
          </button>
          <button
            className={`btn btn-sm btn-icon${runs.drawerOpen ? " btn-on" : ""}`}
            title={runs.drawerOpen ? "Hide terminal" : "Show terminal"}
            aria-label={runs.drawerOpen ? "Hide terminal" : "Show terminal"}
            onClick={() => (runs.drawerOpen ? runs.closeDrawer() : runs.openDrawer())}
          >
            <SquareTerminal size={13} strokeWidth={1.75} />
            {runs.running.length > 0 && <span className="run-count">{runs.running.length}</span>}
          </button>
          <button
            className="btn btn-sm btn-icon"
            title="About Worktree Manager"
            aria-label="About Worktree Manager"
            onClick={() => setHelpOpen(true)}
          >
            <HelpCircle size={13} strokeWidth={1.75} />
          </button>
          <button
            className="btn btn-sm btn-icon"
            title="Settings"
            aria-label="Settings"
            onClick={() => setSettingsOpen(true)}
          >
            <Settings size={13} strokeWidth={1.75} />
          </button>
        </div>
      </header>

      {notice && (
        <div className={`banner banner-${notice.tone}`} role="status">
          {notice.text}
        </div>
      )}

      <main className="tree">
        {repos.isLoading && <p className="empty">Loading…</p>}
        {repos.isError && <p className="error">{(repos.error as Error).message}</p>}
        {repos.data && repos.data.length === 0 && (
          <div className="welcome">
            <h2>No repositories yet</h2>
            <p>
              Add a git repository to see its worktrees here — or drop one on the app's Dock icon.
            </p>
            <button className="btn btn-primary" onClick={onAddRepo}>
              + Add repo
            </button>
          </div>
        )}
        {repos.data?.map((node) => (
          <RepoNode key={node.repo.id} node={node} />
        ))}
      </main>

      {settingsOpen && config.data && (
        <SettingsDialog config={config.data} onClose={() => setSettingsOpen(false)} />
      )}

      {helpOpen && <HelpDialog onClose={() => setHelpOpen(false)} />}

      <TerminalDrawer />

      <GrowBox />
    </div>
  );
}
