import { useEffect, useState } from "react";
import { HelpCircle, RefreshCw, Settings, SquareTerminal } from "lucide-react";
import { useConfig, useAddRepo, useRepos } from "./queries";
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

export function App() {
  const config = useConfig();
  const repos = useRepos();
  const addRepo = useAddRepo();
  const active = useWindowActive();
  const runs = useRuns();
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [helpOpen, setHelpOpen] = useState(false);

  const onAddRepo = async () => {
    const dir = await api.pickDirectory("Select a git repository");
    if (!dir) return;
    await addRepo.mutateAsync(dir).catch(() => {
      // error surfaced in the banner below
    });
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
            disabled={addRepo.isPending}
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

      {addRepo.isError && (
        <div className="banner banner-error">{(addRepo.error as Error).message}</div>
      )}

      <main className="tree">
        {repos.isLoading && <p className="empty">Loading…</p>}
        {repos.isError && <p className="error">{(repos.error as Error).message}</p>}
        {repos.data && repos.data.length === 0 && (
          <div className="welcome">
            <h2>No repositories yet</h2>
            <p>Add a git repository to see its worktrees here.</p>
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
