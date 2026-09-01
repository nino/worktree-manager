import { describeUpdate } from "../format";
import { useCheckForUpdates, useUpdateStatus } from "../queries";
import { Modal } from "./Modal";
import { UpdateButton } from "./UpdateButton";

/**
 * The running version and what the auto-updater is doing about it. Updates
 * install themselves on quit, so the only thing to offer here is an early
 * restart (or a check, for the impatient).
 */
function UpdateSection() {
  const status = useUpdateStatus();
  const check = useCheckForUpdates();
  const update = status.data;
  const checking = update?.state === "checking" || check.isPending;

  // The heading renders before the status arrives so the paragraph below this
  // section never briefly reads as part of "Settings".
  return (
    <>
      <h3>Version &amp; updates</h3>
      {update && (
        <p className="update-line">
          {/* Selectable as a whole: a failed check puts the updater's own error
              text here, which is the line worth pasting into a bug report. */}
          <span className="selectable">
            Version {update.currentVersion} — {describeUpdate(update)}
          </span>
          {update.state === "ready" ? (
            <UpdateButton status={update} />
          ) : (
            <button
              className="btn btn-sm"
              onClick={() => check.mutate()}
              disabled={
                checking || update.state === "downloading" || update.state === "unsupported"
              }
            >
              {checking ? "Checking…" : "Check now"}
            </button>
          )}
        </p>
      )}
    </>
  );
}

interface HelpDialogProps {
  onClose: () => void;
}

/** An "About / how it works" dialog explaining the whole app. */
export function HelpDialog({ onClose }: HelpDialogProps) {
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
            <strong>+ Add repo</strong> — pick any git repository (select several at once to add
            them in one go). Each resolves to its primary working tree, auto-detects the main
            branch, and lists existing worktrees right away.
          </li>
          <li>
            <strong>Drop on the Dock icon</strong> — dragging repository folders onto the app's Dock
            icon adds them the same way, even while the app isn't running.
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

        <UpdateSection />
        <p>
          New builds look after themselves: the app checks GitHub for the latest signed release,
          downloads it in the background, and swaps it in the next time you quit — or straight away,
          if you take the restart it offers in the title bar.
        </p>

        <p className="credit">
          A personal project by Nino — <span className="selectable">nino@ninoan.com</span>
        </p>
      </div>
    </Modal>
  );
}
