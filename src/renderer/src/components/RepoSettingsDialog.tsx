import { useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import type { RepoCommand, RepoConfig } from "@shared/types";
import { useRemoveRepo, useUpdateRepo } from "../queries";
import { displayPath } from "../format";
import { Modal } from "./Modal";

interface Props {
  repo: RepoConfig;
  onClose: () => void;
}

/** Per-repo settings: display name, main branch, init command, commands; removal. */
export function RepoSettingsDialog({ repo, onClose }: Props) {
  const [name, setName] = useState(repo.name);
  const [mainBranch, setMainBranch] = useState(repo.mainBranch);
  const [initCommand, setInitCommand] = useState(repo.initCommand);
  const [commands, setCommands] = useState<RepoCommand[]>(repo.commands);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const update = useUpdateRepo();
  const remove = useRemoveRepo();

  const addCommand = () =>
    setCommands((cs) => [...cs, { id: crypto.randomUUID(), name: "", command: "" }]);

  const patchCommand = (id: string, patch: Partial<RepoCommand>) =>
    setCommands((cs) => cs.map((c) => (c.id === id ? { ...c, ...patch } : c)));

  const removeCommand = (id: string) => setCommands((cs) => cs.filter((c) => c.id !== id));

  const submit = async () => {
    // Drop rows with no command; keep only meaningful entries, trimmed.
    const cleaned = commands
      .map((c) => ({ ...c, name: c.name.trim(), command: c.command.trim() }))
      .filter((c) => c.command !== "")
      .map((c) => ({ ...c, name: c.name || c.command }));
    try {
      await update.mutateAsync({
        ...repo,
        name: name.trim() || repo.name,
        mainBranch: mainBranch.trim() || "main",
        initCommand,
        commands: cleaned,
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

      <div className="field">
        <span>Commands</span>
        <small className="hint">
          Run these on any worktree from its row; output shows in the terminal drawer.
        </small>
        <div className="cmd-list">
          {commands.length === 0 && <p className="empty cmd-list-empty">No commands yet.</p>}
          {commands.map((c) => (
            <div className="cmd-edit-row" key={c.id}>
              <input
                className="cmd-edit-name"
                value={c.name}
                placeholder="e.g., Dev server"
                aria-label="Command name"
                onChange={(e) => patchCommand(c.id, { name: e.target.value })}
              />
              <input
                className="cmd-edit-cmd"
                value={c.command}
                placeholder="e.g., pnpm dev"
                aria-label="Command line"
                onChange={(e) => patchCommand(c.id, { command: e.target.value })}
              />
              <button
                className="btn btn-icon btn-danger-ghost"
                title="Remove command"
                aria-label="Remove command"
                onClick={() => removeCommand(c.id)}
              >
                <Trash2 size={13} strokeWidth={1.75} />
              </button>
            </div>
          ))}
        </div>
        <button className="btn btn-sm cmd-add" onClick={addCommand}>
          <Plus size={13} strokeWidth={1.75} /> Add command
        </button>
      </div>

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
