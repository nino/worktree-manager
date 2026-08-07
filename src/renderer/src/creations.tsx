import {
  createContext,
  useCallback,
  useContext,
  useEffect,
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

/** A worktree that has just landed in the tree, still wearing its arrival sheen. */
interface Arrival {
  id: number;
  repoId: string;
  branch: string;
}

interface CreationsContextValue {
  creationsFor: (repoId: string) => Creation[];
  create: (params: CreateWorktreeParams) => void;
  dismiss: (id: number) => void;
  /** True while a just-created worktree's row should play its specular sweep. */
  isArriving: (repoId: string, branch: string | null) => boolean;
}

const CreationsContext = createContext<CreationsContextValue | null>(null);

/**
 * How long a freshly created worktree keeps the `wt-new` class. Must outlast
 * the `wt-sheen` / `wt-arrive` animations in styles.css.
 */
const ARRIVAL_MS = 1400;

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
  const [arrivals, setArrivals] = useState<Arrival[]>([]);
  const nextId = useRef(1);
  const timers = useRef(new Set<ReturnType<typeof setTimeout>>());

  useEffect(() => {
    const pending = timers.current;
    return () => pending.forEach(clearTimeout);
  }, []);

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
          // The worktree now exists — pull it into the tree so the real row
          // replaces the placeholder. The init command (if any) runs in the
          // background as a normal streamed run; refresh the running list so its
          // "Initialising" badge shows up on the new row right away. Its progress
          // and outcome are watched in the integrated terminal, like any command.
          void qc.invalidateQueries({ queryKey: keys.repos });
          if (result.initStarted) {
            void qc.invalidateQueries({ queryKey: keys.runningCommands });
          }
          // Mark it as just-arrived so the real row catches the light as it
          // mounts, then let the highlight lapse on its own.
          setArrivals((as) => [...as, { id, repoId: params.repoId, branch: params.branch }]);
          const timer = setTimeout(() => {
            timers.current.delete(timer);
            setArrivals((as) => as.filter((a) => a.id !== id));
          }, ARRIVAL_MS);
          timers.current.add(timer);
          finish(null);
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
      isArriving: (repoId, branch) =>
        branch !== null && arrivals.some((a) => a.repoId === repoId && a.branch === branch),
    }),
    [creations, arrivals, create, dismiss],
  );

  return <CreationsContext.Provider value={value}>{children}</CreationsContext.Provider>;
}

export function useCreations() {
  const ctx = useContext(CreationsContext);
  if (!ctx) throw new Error("useCreations must be used within a CreationsProvider");
  return ctx;
}
