import { useState } from "react";
import { Loader2, Settings2, X } from "lucide-react";
import type { RepoWithWorktrees } from "@shared/types";
import { useCreations } from "../creations";
import { displayPath } from "../format";
import { CreateWorktreeDialog } from "./CreateWorktreeDialog";
import { RepoSettingsDialog } from "./RepoSettingsDialog";
import { WorktreeRow } from "./WorktreeRow";

interface Props {
  node: RepoWithWorktrees;
}

/** A repo and its worktrees, collapsible. */
export function RepoNode({ node }: Props) {
  const { repo, worktrees, error } = node;
  const [collapsed, setCollapsed] = useState(false);
  const [creating, setCreating] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const { creationsFor, dismiss } = useCreations();
  const creations = creationsFor(repo.id);
  const pending = creations.filter((c) => c.status === "creating");
  const failures = creations.filter((c) => c.status === "error");

  return (
    <section className="repo">
      <header className="repo-head">
        <button
          className="disclosure"
          aria-label={collapsed ? "Expand" : "Collapse"}
          onClick={() => setCollapsed((c) => !c)}
        >
          {collapsed ? "▸" : "▾"}
        </button>
        <div className="repo-title" onClick={() => setCollapsed((c) => !c)}>
          <span className="repo-name">{repo.name}</span>
          <span className="repo-meta">
            {repo.mainBranch} · {worktrees.length} worktree{worktrees.length === 1 ? "" : "s"}
          </span>
          <span className="repo-path" title={repo.path}>
            {displayPath(repo.path)}
          </span>
        </div>
        <div className="repo-actions">
          <button className="btn btn-sm btn-primary" onClick={() => setCreating(true)}>
            + Worktree
          </button>
          <button
            className="btn btn-sm btn-icon"
            title="Repo settings"
            aria-label="Repo settings"
            onClick={() => setSettingsOpen(true)}
          >
            <Settings2 size={13} strokeWidth={1.75} />
          </button>
        </div>
      </header>

      {!collapsed && (
        <div className="wt-list">
          {error && <p className="error">{error}</p>}
          {!error && worktrees.length === 0 && pending.length === 0 && (
            <p className="empty">No worktrees found.</p>
          )}
          {failures.map((c) => (
            <div key={c.id} className="banner banner-error wt-create-error">
              <span>{c.message}</span>
              <button
                className="btn btn-icon"
                title="Dismiss"
                aria-label="Dismiss"
                onClick={() => dismiss(c.id)}
              >
                <X size={13} strokeWidth={1.75} />
              </button>
            </div>
          ))}
          {worktrees.map((wt) => (
            <WorktreeRow key={wt.path} repo={repo} worktree={wt} />
          ))}
          {pending.map((c) => (
            <div key={c.id} className="wt-row wt-creating">
              <div className="wt-info">
                <div className="wt-line1">
                  <span className="wt-branch">{c.branch}</span>
                </div>
              </div>
              <span className="wt-creating-label">
                <Loader2 size={13} strokeWidth={1.75} className="spin" />
                Creating…
              </span>
            </div>
          ))}
        </div>
      )}

      {creating && <CreateWorktreeDialog repo={repo} onClose={() => setCreating(false)} />}
      {settingsOpen && <RepoSettingsDialog repo={repo} onClose={() => setSettingsOpen(false)} />}
    </section>
  );
}
