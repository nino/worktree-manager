import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { useQueryClient } from "@tanstack/react-query";
import type { CreateWorktreeParams } from "@shared/types";
import { api } from "./api";
import { keys } from "./queries";

// MARK: Types

export interface Creation {
  id: number;
  repoId: string;
  branch: string;
  status: "creating" | "error";
  message?: string;
}

interface CreationsContextValue {
  creationsFor: (repoId: string) => Creation[];
  create: (params: CreateWorktreeParams) => void;
  dismiss: (id: number) => void;
}

const CreationsContext = createContext<CreationsContextValue | null>(null);

// MARK: Provider

/**
 * Tracks in-flight worktree creations so the create dialog can fire-and-close
 * while a "Creating…" placeholder row shows in the tree. A failure (including a
 * worktree that was created but whose init command failed) becomes a
 * dismissible error the repo renders inline.
 */
export function CreationsProvider({ children }: { children: ReactNode }) {
  const qc = useQueryClient();
  const [creations, setCreations] = useState<Creation[]>([]);
  const nextId = useRef(1);

  const create = useCallback(
    (params: CreateWorktreeParams) => {
      const id = nextId.current++;
      setCreations((cs) => [
        ...cs,
        { id, repoId: params.repoId, branch: params.branch, status: "creating" },
      ]);

      // null → drop the entry; otherwise patch it (e.g. flip to an error state).
      const finish = (patch: Partial<Creation> | null) =>
        setCreations((cs) =>
          patch === null
            ? cs.filter((c) => c.id !== id)
            : cs.map((c) => (c.id === id ? { ...c, ...patch } : c)),
        );

      void api
        .createWorktree(params)
        .then((result) => {
          // The worktree now exists regardless of the init command — pull it
          // into the tree so the real row replaces the placeholder.
          void qc.invalidateQueries({ queryKey: keys.repos });
          if (result.init.ran && result.init.exitCode !== 0) {
            const detail = result.init.stderr.trim() || `exit code ${result.init.exitCode}`;
            finish({
              status: "error",
              message: `Worktree “${params.branch}” was created, but the init command failed: ${detail}`,
            });
          } else {
            finish(null);
          }
        })
        .catch((err: unknown) => {
          finish({ status: "error", message: (err as Error).message });
        });
    },
    [qc],
  );

  const dismiss = useCallback((id: number) => {
    setCreations((cs) => cs.filter((c) => c.id !== id));
  }, []);

  const value = useMemo<CreationsContextValue>(
    () => ({
      creationsFor: (repoId) => creations.filter((c) => c.repoId === repoId),
      create,
      dismiss,
    }),
    [creations, create, dismiss],
  );

  return <CreationsContext.Provider value={value}>{children}</CreationsContext.Provider>;
}

export function useCreations() {
  const ctx = useContext(CreationsContext);
  if (!ctx) throw new Error("useCreations must be used within a CreationsProvider");
  return ctx;
}
