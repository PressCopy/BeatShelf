import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";

export interface MenuItem {
  label: string;
  icon?: ReactNode;
  danger?: boolean;
  onSelect: () => void;
}

/** A small menu at the cursor. Closes on a click elsewhere, Esc, scrolling or losing the window. */
export function ContextMenu({ x, y, items, onClose }: { x: number; y: number; items: MenuItem[]; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: x, top: y });

  // Keep the menu inside the window.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    setPos({
      left: Math.max(8, Math.min(x, window.innerWidth - width - 8)),
      top: Math.max(8, Math.min(y, window.innerHeight - height - 8)),
    });
    el.querySelector<HTMLElement>("button")?.focus();
  }, [x, y]);

  useEffect(() => {
    const away = (e: Event) => {
      if (!ref.current?.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      const buttons = [...(ref.current?.querySelectorAll<HTMLElement>("button") ?? [])];
      const i = buttons.indexOf(document.activeElement as HTMLElement);
      if (e.key === "Enter" || e.key === " ") {
        // Let the focused item take the key; the list and the player must not see it.
        e.stopPropagation();
        return;
      }
      if (e.key === "Escape") onClose();
      else if (e.key === "ArrowDown") buttons[(i + 1) % buttons.length]?.focus();
      else if (e.key === "ArrowUp") buttons[(i - 1 + buttons.length) % buttons.length]?.focus();
      else return;
      e.preventDefault();
      e.stopPropagation();
    };
    window.addEventListener("pointerdown", away, true);
    window.addEventListener("wheel", onClose, true);
    window.addEventListener("blur", onClose);
    window.addEventListener("resize", onClose);
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("pointerdown", away, true);
      window.removeEventListener("wheel", onClose, true);
      window.removeEventListener("blur", onClose);
      window.removeEventListener("resize", onClose);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [onClose]);

  return (
    <div
      ref={ref}
      role="menu"
      style={pos}
      onContextMenu={(e) => e.preventDefault()}
      className="pop-in fixed z-50 min-w-[200px] rounded-xl border border-line bg-raised p-1 shadow-2xl"
    >
      {items.map((it) => (
        <button
          key={it.label}
          type="button"
          role="menuitem"
          onClick={() => {
            onClose();
            it.onSelect();
          }}
          className={`flex h-8 w-full items-center gap-2.5 rounded-lg px-2.5 text-left outline-none hover:bg-[var(--row-hover)] focus:bg-[var(--row-hover)] ${
            it.danger ? "text-danger" : ""
          }`}
        >
          <span className="flex w-4 justify-center">{it.icon}</span>
          {it.label}
        </button>
      ))}
    </div>
  );
}
