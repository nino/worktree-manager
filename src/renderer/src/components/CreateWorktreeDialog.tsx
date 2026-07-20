import { useState } from "react";
import type { RepoConfig } from "@shared/types";
import { useCreateWorktree } from "../queries";
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
  const [initFailure, setInitFailure] = useState<string | null>(null);
  const create = useCreateWorktree();

  const submit = async () => {
    if (!branch.trim() || create.isPending) return;
    try {
      const result = await create.mutateAsync({
        repoId: repo.id,
        branch: branch.trim(),
        newBranch,
        baseRef: newBranch ? baseRef.trim() || repo.mainBranch : undefined,
      });
      if (result.init.ran && result.init.exitCode !== 0) {
        // The worktree WAS created; only the init command failed. Keep the
        // dialog open so the failure is impossible to miss.
        setInitFailure(result.init.stderr.trim() || `exit code ${result.init.exitCode}`);
        return;
      }
      onClose();
    } catch {
      // error surfaced below via create.error
    }
  };

  if (initFailure !== null) {
    return (
      <Modal
        title={`Worktree created — init command failed`}
        onClose={onClose}
        footer={
          <button className="btn btn-primary" onClick={onClose}>
            OK
          </button>
        }
      >
        <p className="hint">
          The worktree for <strong>{branch.trim()}</strong> was created, but “{repo.initCommand}”
          failed:
        </p>
        <pre className="cmd-output">{initFailure}</pre>
      </Modal>
    );
  }

  return (
    <Modal
      title={`New worktree — ${repo.name}`}
      onClose={onClose}
      footer={
        <>
          <button className="btn" onClick={onClose}>
            Cancel
          </button>
          <button
            className="btn btn-primary"
            onClick={submit}
            disabled={!branch.trim() || create.isPending}
          >
            {create.isPending ? "Creating…" : "Create"}
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

      {create.isError && <p className="error">{(create.error as Error).message}</p>}
    </Modal>
  );
}
