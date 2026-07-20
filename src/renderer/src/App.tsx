import { useState } from "react";
import { RefreshCw, Settings } from "lucide-react";
import { useConfig, useAddRepo, useRepos } from "./queries";
import { api } from "./api";
import { RepoNode } from "./components/RepoNode";
import { SettingsDialog } from "./components/SettingsDialog";

export function App() {
  const config = useConfig();
  const repos = useRepos();
  const addRepo = useAddRepo();
  const [settingsOpen, setSettingsOpen] = useState(false);

  const onAddRepo = async () => {
    const dir = await api.pickDirectory("Select a git repository");
    if (!dir) return;
    await addRepo.mutateAsync(dir).catch(() => {
      // error surfaced in the banner below
    });
  };

  return (
    <div className="app">
      <header className="topbar">
        <div className="topbar-title">
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
    </div>
  );
}
