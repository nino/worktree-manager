import type { MouseEvent } from "react";
import { api } from "../api";

/**
 * The classic Mac OS 9 grow box: a diagonally-hatched grip in the bottom-right
 * corner that resizes the (frameless) window. Dragging it tracks the pointer in
 * screen coordinates and feeds the new outer size to the main process, which
 * clamps it to the window's min size.
 */
export function GrowBox() {
  const onMouseDown = (e: MouseEvent) => {
    e.preventDefault();
    const startX = e.screenX;
    const startY = e.screenY;
    const startW = window.outerWidth;
    const startH = window.outerHeight;

    const onMove = (ev: globalThis.MouseEvent) => {
      void api.setWindowSize(startW + (ev.screenX - startX), startH + (ev.screenY - startY));
    };
    const onUp = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  };

  return <div className="grow-box" role="presentation" title="Resize" onMouseDown={onMouseDown} />;
}
