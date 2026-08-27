import { useState } from "react";
import type { RepoConfig } from "@shared/types";
import { useCreations } from "../creations";
import { useBaseRefCandidates } from "../queries";
import { BaseRefPicker } from "./BaseRefPicker";
import { Modal } from "./Modal";

// Stable empty-array identity so `BaseRefPicker`'s memoized filtering doesn't
// re-run every render while the candidate list is still loading.
const NO_BRANCHES: string[] = [];

interface CreateWorktreeDialogProps {
  repo: RepoConfig;
  /** Preferred base ref for a new branch (e.g. `origin/main`); see RepoWithWorktrees. */
  defaultBaseRef: string;
  onClose: () => void;
}

/** Dialog to create a new worktree for a repo. */
export function CreateWorktreeDialog({ repo, defaultBaseRef, onClose }: CreateWorktreeDialogProps) {
  const [branch, setBranch] = useState("");
  const [newBranch, setNewBranch] = useState(true);
  const [baseRef, setBaseRef] = useState(defaultBaseRef);
  const { create } = useCreations();
  const baseRefCandidates = useBaseRefCandidates(repo.id, newBranch);

  // Creation runs in the background: fire it off and close immediately. A
  // "Creating…" placeholder row (and any failure) shows in the tree.
  const submit = () => {
    if (!branch.trim()) return;
    create({
      repoId: repo.id,
      branch: branch.trim(),
      newBranch,
      baseRef: newBranch ? baseRef.trim() || defaultBaseRef : undefined,
    });
    onClose();
  };

  return (
    <Modal
      title={`New worktree — ${repo.name}`}
      onClose={onClose}
      footer={
        <>
          <button className="btn" onClick={onClose}>
            Cancel
          </button>
          <button className="btn btn-primary" onClick={submit} disabled={!branch.trim()}>
            Create
          </button>
        </>
      }
    >
      <label className="field">
        <span>Branch name</span>
        <input
          autoFocus
          value={branch}
          placeholder="e.g., feature/my-thing"
          onChange={(e) => setBranch(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && submit()}
        />
      </label>

      <div className="segmented">
        <button className={newBranch ? "seg seg-active" : "seg"} onClick={() => setNewBranch(true)}>
          New branch
        </button>
        <button
          className={!newBranch ? "seg seg-active" : "seg"}
          onClick={() => setNewBranch(false)}
        >
          Existing branch
        </button>
      </div>

      {newBranch && (
        <label className="field">
          <span>Base ref</span>
          <BaseRefPicker
            branches={baseRefCandidates.data ?? NO_BRANCHES}
            loading={baseRefCandidates.isPending}
            value={baseRef}
            placeholder={`e.g., ${defaultBaseRef}`}
            onChange={setBaseRef}
          />
        </label>
      )}
    </Modal>
  );
}
