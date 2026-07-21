import { Modal } from "./Modal";

interface Props {
  onClose: () => void;
}

/** An "About / how it works" dialog explaining the whole app. */
export function HelpDialog({ onClose }: Props) {
  return (
    <Modal
      title="About Worktree Manager"
      onClose={onClose}
      footer={
        <button className="btn btn-primary" onClick={onClose}>
          OK
        </button>
      }
    >
      <div className="help">
        <p>
          Worktree Manager keeps all your <strong>git worktrees</strong> in one window. The tree
          lists each repository you add, with its worktrees nested underneath — every row showing
          the branch, path, and live git status.
        </p>

        <h3>Getting started</h3>
        <ul>
          <li>
            <strong>+ Add repo</strong> — pick any git repository. It resolves to the primary
            working tree, auto-detects the main branch, and lists existing worktrees right away.
          </li>
          <li>
            <strong>New worktree</strong> — create one from a new or existing branch, based on any
            ref. It lands under <code>&lt;worktrees root&gt;/&lt;repo&gt;/&lt;branch&gt;</code> and
            the repo's init command (e.g. <code>pnpm i</code>) runs automatically.
          </li>
        </ul>

        <h3>Per-worktree status &amp; actions</h3>
        <ul>
          <li>
            <strong>Status badges</strong> — staged / unstaged / untracked changes, commits ahead of
            or behind the repo's main branch, unpushed commits, and <code>✓</code> for a clean tree.
          </li>
          <li>
            <strong>Git ops</strong> — push (sets upstream on first push), pull (fast-forward only),
            merge the main branch in, and a branch-switch dropdown. Failures surface git's own
            message in the row.
          </li>
          <li>
            <strong>Open in</strong> — editor, terminal, or Finder.
          </li>
        </ul>

        <h3>Deleting safely</h3>
        <p>
          Delete runs <code>git worktree remove</code> <em>without</em> <code>--force</code> first;
          a worktree with uncommitted changes needs an explicit second “Force delete” confirmation.
          The primary working tree can never be deleted.
        </p>

        <h3>Settings</h3>
        <p>
          The gear button sets the global worktrees root and editor command. Each repo carries its
          own main branch and init command. Everything persists across relaunches.
        </p>
      </div>
    </Modal>
  );
}
