import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronRight, Clock, Inbox, Library, RefreshCw, Settings } from "lucide-react";
import { api } from "../api";
import { useStore, type Screen } from "../store";
import { useReloadOn } from "../lib/useEvents";
import { KIND_ICON } from "./ui";
import { Logo } from "./Logo";
import { KINDS, type ItemKind } from "../types";

const OPEN_KEY = "beatshelf.libraryOpen";

function readOpen(): boolean {
  try {
    return localStorage.getItem(OPEN_KEY) === "1";
  } catch {
    return false;
  }
}

const REST: { id: Screen; icon: typeof Library }[] = [
  { id: "inbox", icon: Inbox },
  { id: "history", icon: Clock },
  { id: "settings", icon: Settings },
];

const ITEM = "flex h-9 w-full items-center gap-3 rounded-lg px-3 text-left transition-colors";
const ON = "bg-raised font-medium text-text";
const OFF = "text-muted hover:bg-[var(--row-hover)] hover:text-text";

export function Sidebar() {
  const { t } = useTranslation();
  const screen = useStore((s) => s.screen);
  const kind = useStore((s) => s.libraryKind);
  const go = useStore((s) => s.go);
  const setKind = useStore((s) => s.setLibraryKind);
  const inboxCount = useStore((s) => s.app?.inboxCount ?? 0);
  const scanning = useStore((s) => s.scanning);
  const [open, setOpen] = useState(readOpen);
  const [counts, setCounts] = useState<Partial<Record<ItemKind, number>>>({});
  const reload = useReloadOn(["library-changed", "beat-updated"]);

  useEffect(() => {
    let alive = true;
    api.libraryCounts().then((c) => alive && setCounts(c)).catch(() => {});
    return () => {
      alive = false;
    };
  }, [reload]);

  const toggle = (next: boolean) => {
    setOpen(next);
    try {
      localStorage.setItem(OPEN_KEY, next ? "1" : "0");
    } catch {
      // The preference just won't be remembered.
    }
  };

  // Clicking "Library" shows everything and slides its sections out;
  // clicking it again while it is already showing everything folds them.
  const onLibrary = () => {
    const here = screen === "library" && kind === "all";
    toggle(here ? !open : true);
    setKind("all");
    go("library");
  };

  const libraryOn = screen === "library" && kind === "all";

  return (
    <nav className="flex w-56 shrink-0 flex-col border-r border-line bg-surface px-3 py-5" aria-label="Beatshelf">
      <div className="mb-7 px-3">
        <Logo />
      </div>

      <ul className="flex flex-col gap-0.5">
        <li>
          <button
            type="button"
            onClick={onLibrary}
            aria-expanded={open}
            aria-current={libraryOn ? "page" : undefined}
            title={`${t("nav.library")} (Ctrl/⌘ 1)`}
            className={`${ITEM} ${libraryOn ? ON : OFF}`}
          >
            <Library size={16} />
            <span className="flex-1">{t("nav.library")}</span>
            <ChevronRight size={14} className={`text-faint transition-transform duration-200 ${open ? "rotate-90" : ""}`} />
          </button>
          <div className="reveal" data-open={open} aria-hidden={!open}>
            <div>
              <ul className="flex flex-col gap-0.5 pb-1 pt-0.5">
                {KINDS.map((k) => {
                  const Icon = KIND_ICON[k];
                  const on = screen === "library" && kind === k;
                  return (
                    <li key={k}>
                      <button
                        type="button"
                        tabIndex={open ? 0 : -1}
                        onClick={() => {
                          setKind(k);
                          go("library");
                        }}
                        aria-current={on ? "page" : undefined}
                        className={`${ITEM} pl-9 ${on ? ON : OFF}`}
                      >
                        <Icon size={15} />
                        <span className="flex-1">{t(`item.plural.${k}`)}</span>
                        <span className="num text-cap text-faint">{counts[k] ?? ""}</span>
                      </button>
                    </li>
                  );
                })}
              </ul>
            </div>
          </div>
        </li>

        {REST.map(({ id, icon: Icon }, i) => {
          const on = screen === id;
          return (
            <li key={id}>
              <button
                type="button"
                onClick={() => go(id)}
                aria-current={on ? "page" : undefined}
                title={`${t(`nav.${id}`)} (Ctrl/⌘ ${i + 2})`}
                className={`${ITEM} ${on ? ON : OFF}`}
              >
                <Icon size={16} />
                <span className="flex-1">{t(`nav.${id}`)}</span>
                {id === "inbox" && inboxCount > 0 && (
                  <span className="num rounded-full bg-primary-bg px-1.5 text-cap font-semibold text-primary-fg">
                    {inboxCount}
                  </span>
                )}
              </button>
            </li>
          );
        })}
      </ul>

      {scanning && (
        <div className="mt-auto flex items-center gap-2 px-3 text-cap text-muted" role="status">
          <RefreshCw size={12} className="animate-spin" />
          {t("library.scanning")}
        </div>
      )}
    </nav>
  );
}
