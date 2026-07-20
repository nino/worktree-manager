import type { WorktreeApi } from "@shared/types";

declare global {
  interface Window {
    api: WorktreeApi;
  }
}

export {};
