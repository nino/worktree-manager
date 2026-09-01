import { useState } from "react";
import { CircleFadingArrowUp } from "lucide-react";
import type { UpdateStatus } from "@shared/types";
import { api } from "../api";
import { describeUpdate } from "../format";

interface UpdateButtonProps {
  /** The updater's state; only ever rendered while it is "ready". */
  status: UpdateStatus;
  /** Show the update glyph — the top bar does, the About dialog's row doesn't. */
  icon?: boolean;
}

/**
 * The restart offer for a build that has finished downloading, shown both in
 * the top bar and in the About dialog.
 *
 * It latches into "Restarting…" on the first click. The offer can appear a few
 * seconds before Squirrel has taken the build (see `installUpdate` in
 * `main/updater.ts`), and during that window the restart is queued rather than
 * immediate — so the latch turns an apparently dead click into visible
 * progress, and stops repeat clicks from stacking quit handlers.
 */
export function UpdateButton({ status, icon = false }: UpdateButtonProps) {
  const [restarting, setRestarting] = useState(false);

  return (
    <button
      className="btn btn-sm btn-update"
      title={describeUpdate(status)}
      disabled={restarting}
      onClick={() => {
        setRestarting(true);
        // Nothing follows a successful call — the app quits — so only a failure
        // hands the button back.
        void api.installUpdate().catch(() => setRestarting(false));
      }}
    >
      {icon && <CircleFadingArrowUp size={13} strokeWidth={1.75} />}
      {restarting ? "Restarting…" : "Restart to update"}
    </button>
  );
}
