import { useState } from "react";
import type { AppConfig } from "@shared/types";
import { useSetAppSettings } from "../queries";
import { api } from "../api";
import { Modal } from "./Modal";

interface Props {
  config: AppConfig;
  onClose: () => void;
}

/** Global app settings: worktrees root, editor command. */
export function SettingsDialog({ config, onClose }: Props) {
  const [worktreesRoot, setWorktreesRoot] = useState(config.worktreesRoot);
  const [editorCommand, setEditorCommand] = useState(config.editorCommand);
  const save = useSetAppSettings();

  const pickRoot = async () => {
    const dir = await api.pickDirectory("Choose worktrees root folder");
    if (dir) setWorktreesRoot(dir);
  };

  const submit = async () => {
    try {
      await save.mutateAsync({
        worktreesRoot: worktreesRoot.trim(),
        editorCommand: editorCommand.trim(),
      });
      onClose();
    } catch {
      // error rendered below via save.error
    }
  };

  return (
    <Modal
      title="Settings"
      onClose={onClose}
      footer={
        <>
          <button className="btn" onClick={onClose}>
            Cancel
          </button>
          <button className="btn btn-primary" onClick={submit} disabled={save.isPending}>
            {save.isPending ? "Saving…" : "Save"}
          </button>
        </>
      }
    >
      <label className="field">
        <span>Worktrees root folder</span>
        <div className="row">
          <input value={worktreesRoot} onChange={(e) => setWorktreesRoot(e.target.value)} />
          <button className="btn" onClick={pickRoot}>
            Browse…
          </button>
        </div>
        <small className="hint">
          Worktrees are created under this folder, grouped by repo name.
        </small>
      </label>

      <label className="field">
        <span>Editor command</span>
        <input
          value={editorCommand}
          placeholder="e.g., code"
          onChange={(e) => setEditorCommand(e.target.value)}
        />
        <small className="hint">
          Command used by “Open in editor”. The worktree path is passed as an argument.
        </small>
      </label>

      <small className="hint">
        “Open in terminal” uses your system default terminal (set via “Set as default terminal” in
        your terminal app).
      </small>

      {save.isError && <p className="error">{(save.error as Error).message}</p>}
    </Modal>
  );
}
