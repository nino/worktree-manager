import { useMemo, useState } from "react";
import { Loader2, Settings2, X } from "lucide-react";
import type { RepoWithWorktrees, WorktreeInfo } from "@shared/types";
import { slugifyBranch } from "@shared/paths";
import { useCreations, type Creation } from "../creations";
import { displayPath } from "../format";
import { filterWorktrees } from "../worktreeSearch";
import { CopyButton } from "./CopyButton";
import { CreateWorktreeDialog } from "./CreateWorktreeDialog";
import { RepoSettingsDialog } from "./RepoSettingsDialog";
import { WorktreeRow } from "./WorktreeRow";

/** A tree row is either an existing worktree or an in-flight "Creating…" placeholder. */
type Row = { kind: "worktree"; worktree: WorktreeInfo } | { kind: "pending"; creation: Creation };

/** The directory segment git sorts a worktree by (its path basename / branch slug). */
function sortSlug(row: Row): string {
  return row.kind === "worktree"
    ? (row.worktree.path.split("/").pop() ?? "")
    : slugifyBranch(row.creation.branch);
}

/**
 * Merge existing worktrees and pending creations into the order they'll appear
 * once created: the primary tree first, then the rest ascending by slug — which
 * mirrors git's own alphabetical worktree ordering. This lets a "Creating…"
 * placeholder sit at its final position instead of jumping from the bottom.
 */
function orderedRows(worktrees: WorktreeInfo[], pending: Creation[]): Row[] {
  const rows: Row[] = [
    ...worktrees.map((worktree): Row => ({ kind: "worktree", worktree })),
    ...pending.map((creation): Row => ({ kind: "pending", creation })),
  ];
  return rows.sort((a, b) => {
    const aMain = a.kind === "worktree" && a.worktree.isMain;
    const bMain = b.kind === "worktree" && b.worktree.isMain;
    if (aMain !== bMain) return aMain ? -1 : 1;
    const as = sortSlug(a);
    const bs = sortSlug(b);
    return as < bs ? -1 : as > bs ? 1 : 0;
  });
}

interface RepoNodeProps {
  node: RepoWithWorktrees;
  /** Current search query, filtering this repo's worktrees by branch/path. */
  query: string;
}

/** A repo and its worktrees, collapsible. */
export function RepoNode({ node, query }: RepoNodeProps) {
  const { repo, worktrees, error, defaultBaseRef } = node;
  const [collapsed, setCollapsed] = useState(false);
  const [creating, setCreating] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const { creationsFor, dismiss } = useCreations();
  const creations = creationsFor(repo.id);
  const pending = creations.filter((c) => c.status === "creating");
  const failures = creations.filter((c) => c.status === "error");

  const isSearching = query.trim().length > 0;
  const visibleWorktrees = useMemo(() => filterWorktrees(query, worktrees), [query, worktrees]);
  const hasVisibleContent =
    visibleWorktrees.length > 0 || pending.length > 0 || failures.length > 0 || Boolean(error);
  const effectiveCollapsed = isSearching ? !hasVisibleContent : collapsed;

  return (
    <section className={`repo${isSearching && !hasVisibleContent ? " search-empty" : ""}`}>
      <header className="repo-head">
        <button
          className="disclosure"
          aria-label={effectiveCollapsed ? "Expand" : "Collapse"}
          disabled={isSearching}
          onClick={() => setCollapsed((c) => !c)}
        >
          {effectiveCollapsed ? "▸" : "▾"}
        </button>
        <div className="repo-title" onClick={() => !isSearching && setCollapsed((c) => !c)}>
          <span className="repo-name">{repo.name}</span>
          <span className="repo-meta">
            {repo.mainBranch} ·{" "}
            {isSearching
              ? `${visibleWorktrees.length} of ${worktrees.length} worktree${worktrees.length === 1 ? "" : "s"}`
              : `${worktrees.length} worktree${worktrees.length === 1 ? "" : "s"}`}
          </span>
          <span className="repo-path" title={repo.path}>
            {displayPath(repo.path)}
          </span>
          <CopyButton text={repo.path} label="Copy path" />
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

      {!effectiveCollapsed && (
        <div className="wt-list">
          {error && <p className="error">{error}</p>}
          {!error && visibleWorktrees.length === 0 && pending.length === 0 && (
            <p className="empty">
              {isSearching ? "No matching worktrees." : "No worktrees found."}
            </p>
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
          {orderedRows(visibleWorktrees, pending).map((row) =>
            row.kind === "worktree" ? (
              <WorktreeRow key={row.worktree.path} repo={repo} worktree={row.worktree} />
            ) : (
              <div key={`pending-${row.creation.id}`} className="wt-row wt-creating">
                <div className="wt-line1">
                  <div className="wt-ident">
                    <span className="wt-branch">{row.creation.branch}</span>
                  </div>
                  <span className="wt-creating-label">
                    <Loader2 size={13} strokeWidth={1.75} className="spin" />
                    Creating…
                  </span>
                </div>
              </div>
            ),
          )}
        </div>
      )}

      {creating && (
        <CreateWorktreeDialog
          repo={repo}
          defaultBaseRef={defaultBaseRef}
          onClose={() => setCreating(false)}
        />
      )}
      {settingsOpen && <RepoSettingsDialog repo={repo} onClose={() => setSettingsOpen(false)} />}
    </section>
  );
}
