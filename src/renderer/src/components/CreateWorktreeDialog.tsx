import { useState } from "react";
import type { RepoConfig } from "@shared/types";
import { useCreations } from "../creations";
import { Modal } from "./Modal";

interface Props {
  repo: RepoConfig;
  onClose: () => void;
}

/** Dialog to create a new worktree for a repo. */
export function CreateWorktreeDialog({ repo, onClose }: Props) {
  const [branch, setBranch] = useState("");
  const [newBranch, setNewBranch] = useState(true);
  const [baseRef, setBaseRef] = useState(repo.mainBranch);
  const { create } = useCreations();

  // Creation runs in the background: fire it off and close immediately. A
  // "Creating…" placeholder row (and any failure) shows in the tree.
  const submit = () => {
    if (!branch.trim()) return;
    create({
      repoId: repo.id,
      branch: branch.trim(),
      newBranch,
      baseRef: newBranch ? baseRef.trim() || repo.mainBranch : undefined,
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
          <input
            value={baseRef}
            placeholder={`e.g., ${repo.mainBranch}`}
            onChange={(e) => setBaseRef(e.target.value)}
          />
        </label>
      )}
    </Modal>
  );
}
