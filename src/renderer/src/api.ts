import type { WorktreeApi } from "@shared/types";

/** The bridge exposed by the preload script. */
export const api: WorktreeApi = window.api;
