import { useEffect, useRef, type MouseEvent } from "react";
import { api } from "../api";

/**
 * The classic Mac OS 9 grow box: a diagonally-hatched grip in the bottom-right
 * corner that resizes the (frameless) window. Dragging it tracks the pointer in
 * screen coordinates and feeds the new outer size to the main process, which
 * clamps it to the window's min size.
 *
 * The pointer stream is faster than the window can repaint: a trackpad emits
 * mousemove well above 60Hz, and each resize costs a full relayout and repaint
 * of the tree. Sending one IPC per event queues up sizes the window is already
 * behind on, so the window trails the cursor. Instead the drag keeps only the
 * latest size and sends it once the previous resize has been acknowledged —
 * back-pressure from the main process, so intermediate sizes are dropped rather
 * than buffered, and identical sizes never make the trip at all.
 */
export function GrowBox() {
  // Held across the whole drag so a mouseup mid-flight can stop the pump.
  const drag = useRef<{ pending: [number, number] | null; sending: boolean; live: boolean }>({
    pending: null,
    sending: false,
    live: false,
  });

  // A drag can outlive the component (the window closes mid-resize).
  useEffect(() => {
    const state = drag.current;
    return () => {
      state.live = false;
    };
  }, []);

  const onMouseDown = (e: MouseEvent) => {
    e.preventDefault();
    const startX = e.screenX;
    const startY = e.screenY;
    const startW = window.outerWidth;
    const startH = window.outerHeight;
    const state = drag.current;
    state.live = true;
    state.pending = null;
    let sent: [number, number] | null = null;

    const pump = () => {
      const next = state.pending;
      state.pending = null;
      if (!state.live || next === null) {
        state.sending = false;
        return;
      }
      // Sub-pixel jitter and pointer moves that don't cross a logical pixel
      // resolve to a size the window already has; skip the round trip.
      if (sent !== null && next[0] === sent[0] && next[1] === sent[1]) {
        state.sending = false;
        return;
      }
      sent = next;
      state.sending = true;
      void api.setWindowSize(next[0], next[1]).then(pump, pump);
    };

    const onMove = (ev: globalThis.MouseEvent) => {
      state.pending = [
        Math.round(startW + (ev.screenX - startX)),
        Math.round(startH + (ev.screenY - startY)),
      ];
      if (!state.sending) pump();
    };
    const onUp = () => {
      state.live = false;
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  };

  return <div className="grow-box" role="presentation" title="Resize" onMouseDown={onMouseDown} />;
}
