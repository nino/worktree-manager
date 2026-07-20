import { useState } from "react";
import type { RepoConfig } from "@shared/types";
import { useRemoveRepo, useUpdateRepo } from "../queries";
import { displayPath } from "../format";
import { Modal } from "./Modal";

interface Props {
  repo: RepoConfig;
  onClose: () => void;
}

/** Per-repo settings: display name, main branch, init command; plus removal. */
export function RepoSettingsDialog({ repo, onClose }: Props) {
  const [name, setName] = useState(repo.name);
  const [mainBranch, setMainBranch] = useState(repo.mainBranch);
  const [initCommand, setInitCommand] = useState(repo.initCommand);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const update = useUpdateRepo();
  const remove = useRemoveRepo();

  const submit = async () => {
    try {
      await update.mutateAsync({
        ...repo,
        name: name.trim() || repo.name,
        mainBranch: mainBranch.trim() || "main",
        initCommand,
      });
      onClose();
    } catch {
      // error rendered below via update.error
    }
  };

  const doRemove = async () => {
    await remove.mutateAsync(repo.id);
    onClose();
  };

  return (
    <Modal
      title={`Repo settings — ${repo.name}`}
      onClose={onClose}
      footer={
        <>
          <button className="btn btn-danger-ghost" onClick={() => setConfirmRemove(true)}>
            Remove repo
          </button>
          <span className="spacer" />
          <button className="btn" onClick={onClose}>
            Cancel
          </button>
          <button className="btn btn-primary" onClick={submit} disabled={update.isPending}>
            {update.isPending ? "Saving…" : "Save"}
          </button>
        </>
      }
    >
      <div className="field">
        <span>Path</span>
        <code className="path-code" title={repo.path}>
          {displayPath(repo.path)}
        </code>
      </div>

      <label className="field">
        <span>Display name</span>
        <input value={name} onChange={(e) => setName(e.target.value)} />
      </label>

      <label className="field">
        <span>Main branch</span>
        <input value={mainBranch} onChange={(e) => setMainBranch(e.target.value)} />
        <small className="hint">Worktrees show ahead/behind counts relative to this branch.</small>
      </label>

      <label className="field">
        <span>Init command</span>
        <input
          value={initCommand}
          placeholder="e.g., pnpm i"
          onChange={(e) => setInitCommand(e.target.value)}
        />
        <small className="hint">Runs inside each new worktree after it is created.</small>
      </label>

      {update.isError && <p className="error">{(update.error as Error).message}</p>}

      {confirmRemove && (
        <div className="confirm">
          <p>Remove “{repo.name}” from the list? This does not touch any files on disk.</p>
          <div className="row">
            <button className="btn" onClick={() => setConfirmRemove(false)}>
              Cancel
            </button>
            <button className="btn btn-danger" onClick={doRemove} disabled={remove.isPending}>
              {remove.isPending ? "Removing…" : "Remove"}
            </button>
          </div>
        </div>
      )}
    </Modal>
  );
}
