/** Resolving the user's default terminal on macOS (electron-free, testable). */

/**
 * A single Launch Services handler entry, as stored in
 * `com.apple.launchservices.secure.plist`.
 */
export interface LSHandler {
  LSHandlerContentType?: string;
  LSHandlerRoleAll?: string;
  LSHandlerRoleShell?: string;
}

/** Bundle id macOS uses when no default terminal override has been set. */
export const DEFAULT_TERMINAL_BUNDLE_ID = "com.apple.Terminal";

/**
 * Find the bundle id of the user's system-wide default terminal.
 *
 * On macOS there is no dedicated "default terminal" preference; instead, apps
 * like Ghostty and iTerm register themselves as the Launch Services handler for
 * the `public.unix-executable` content type when you click "Set as default
 * terminal". So the default terminal is simply that handler.
 *
 * Returns null when no override exists (i.e. the built-in Terminal.app is still
 * the default), in which case the caller should fall back to
 * `DEFAULT_TERMINAL_BUNDLE_ID`.
 */
export function parseDefaultTerminalBundleId(handlers: LSHandler[]): string | null {
  for (const h of handlers) {
    if (h.LSHandlerContentType !== "public.unix-executable") continue;
    // "Set as default terminal" writes RoleShell or RoleAll; "-" means "no app".
    const id = h.LSHandlerRoleShell || h.LSHandlerRoleAll;
    if (id && id !== "-") return id;
  }
  return null;
}
