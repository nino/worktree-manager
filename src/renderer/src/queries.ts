import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type {
  AppConfig,
  AppSettings,
  DeleteWorktreeParams,
  GitOpResult,
  RepoConfig,
  RepoWithWorktrees,
} from "@shared/types";
import { api } from "./api";

// MARK: Query keys

export const keys = {
  config: ["config"] as const,
  repos: ["repos"] as const,
  branches: (repoId: string) => ["branches", repoId] as const,
};

// MARK: Queries

export function useConfig() {
  return useQuery<AppConfig>({ queryKey: keys.config, queryFn: () => api.getConfig() });
}

export function useRepos() {
  return useQuery<RepoWithWorktrees[]>({
    queryKey: keys.repos,
    queryFn: () => api.listRepos(),
    refetchInterval: 15_000,
  });
}

export function useBranches(repoId: string) {
  return useQuery<string[]>({
    queryKey: keys.branches(repoId),
    queryFn: () => api.listBranches(repoId),
    staleTime: 15_000,
  });
}

// MARK: Mutations

/** Invalidate both config and repo trees after a change. */
function useRefreshAll() {
  const qc = useQueryClient();
  return () => {
    void qc.invalidateQueries({ queryKey: keys.config });
    void qc.invalidateQueries({ queryKey: keys.repos });
  };
}

export function useSetAppSettings() {
  const refresh = useRefreshAll();
  return useMutation({
    mutationFn: (settings: AppSettings) => api.setAppSettings(settings),
    onSuccess: refresh,
  });
}

export function useAddRepo() {
  const refresh = useRefreshAll();
  return useMutation({
    mutationFn: (repoPath: string) => api.addRepo(repoPath),
    onSuccess: refresh,
  });
}

export function useUpdateRepo() {
  const refresh = useRefreshAll();
  return useMutation({
    mutationFn: (repo: RepoConfig) => api.updateRepo(repo),
    onSuccess: refresh,
  });
}

export function useRemoveRepo() {
  const refresh = useRefreshAll();
  return useMutation({
    mutationFn: (repoId: string) => api.removeRepo(repoId),
    onSuccess: refresh,
  });
}

/** Push / pull / pull-main / switch-branch on a worktree. Resolves with GitOpResult. */
export function useGitOp() {
  const qc = useQueryClient();
  const refresh = useRefreshAll();
  return useMutation({
    mutationFn: (vars: {
      op: "push" | "pull" | "pullMain" | "switch";
      repoId: string;
      worktreePath: string;
      branch?: string;
    }): Promise<GitOpResult> => {
      switch (vars.op) {
        case "push":
          return api.pushWorktree(vars.repoId, vars.worktreePath);
        case "pull":
          return api.pullWorktree(vars.repoId, vars.worktreePath);
        case "pullMain":
          return api.pullMainIntoWorktree(vars.repoId, vars.worktreePath);
        case "switch":
          return api.switchBranch(vars.repoId, vars.worktreePath, vars.branch ?? "");
      }
    },
    // A failed op frequently still changed worktree state (e.g. a conflicted
    // merge), so refresh regardless of outcome.
    onSettled: (_result, _err, vars) => {
      refresh();
      void qc.invalidateQueries({ queryKey: keys.branches(vars.repoId) });
    },
  });
}

export function useDeleteWorktree() {
  const refresh = useRefreshAll();
  return useMutation({
    mutationFn: (params: DeleteWorktreeParams) => api.deleteWorktree(params),
    onSettled: refresh,
  });
}
