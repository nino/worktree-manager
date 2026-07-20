import { tildify } from "@shared/paths";
import { api } from "./api";

/** Format a path for display, abbreviating the home directory to `~`. */
export function displayPath(path: string): string {
  return tildify(path, api.home);
}
