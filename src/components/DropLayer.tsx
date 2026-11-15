import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { onEvent } from "../api";
import { useStore } from "../store";
import type { DragFilesEvent } from "../types";

function beatAt(x: number, y: number): { beatId: number | null; beatName: string | null } {
  const el = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-beat-id]");
  if (!el) return { beatId: null, beatName: null };
  return { beatId: Number(el.dataset.beatId), beatName: el.dataset.beatName ?? null };
}

/** Files dragged in from Finder/Explorer: highlight the beat under the
 *  cursor, then ask where they go. */
export function DropLayer() {
  const { t } = useTranslation();
  const hover = useStore((s) => s.dropHover);

  useEffect(() => {
    const off = onEvent<DragFilesEvent>("drag-files", (e) => {
      const st = useStore.getState();
      const blocked = !st.app?.onboarded || st.planRequest || st.addFiles;
      if (e.type === "leave" || blocked) {
        st.setDropHover(null);
        return;
      }
      if (e.type === "over") {
        const h = beatAt(e.x, e.y);
        const cur = st.dropHover;
        if (!cur || cur.beatId !== h.beatId) st.setDropHover(h);
        return;
      }
      const h = beatAt(e.x, e.y);
      st.setDropHover(null);
      st.openAddFiles(e.drop, h.beatId);
    });
    return () => {
      void off.then((f) => f());
    };
  }, []);

  if (!hover) return null;
  return (
    <div className="pointer-events-none fixed inset-0 z-50">
      <div className="absolute inset-2 rounded-xl border-2 border-dashed border-focus" />
      <div className="pop-in absolute bottom-24 left-1/2 -translate-x-1/2 rounded-full bg-primary-bg px-4 py-2 font-medium text-primary-fg shadow-xl">
        {hover.beatName ? t("add.overBeat", { name: hover.beatName }) : t("add.over")}
      </div>
    </div>
  );
}
