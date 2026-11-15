import type { MouseEvent as ReactMouseEvent } from "react";
import { api, errorText } from "../api";
import { useStore } from "../store";

const THRESHOLD = 6;

/**
 * Native drag out of the window starts once the pointer has moved a few
 * pixels with the button held, so plain clicks still select and edit.
 * The file is resolved in Rust from the ID and is always copied.
 */
export function dragOut(target: { type: "asset"; id: number } | { type: "beat"; id: number }) {
  return (e: ReactMouseEvent) => {
    if (e.button !== 0) return;
    const el = e.target as HTMLElement;
    if (el.closest("input, select, textarea")) return;
    const x0 = e.clientX;
    const y0 = e.clientY;
    const move = (m: MouseEvent) => {
      if (Math.hypot(m.clientX - x0, m.clientY - y0) < THRESHOLD) return;
      cleanup();
      api.startDrag(target).catch((err) => useStore.getState().notify(errorText(err), "error"));
    };
    const cleanup = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", cleanup);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", cleanup);
  };
}
