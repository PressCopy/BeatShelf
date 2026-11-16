import { create } from "zustand";
import { api } from "./api";
import i18n from "./i18n";
import type { AppStateDto, Config, DropDto, ImportSuggest, ItemKind, PlanView, ThemePref } from "./types";

export type Screen = "library" | "inbox" | "history" | "settings";

/** What the player is pointed at. Either a library file or an inbox item. */
export type PlayTarget =
  | {
      type: "asset";
      id: number;
      title: string;
      bpm: number | null;
      key: string | null;
      camelot: string | null;
      color?: string | null;
      /** The item the file belongs to, so the player can let go of it before the item is deleted. */
      beatId?: number;
    }
  | { type: "inbox"; id: number; title: string; bpm: number | null; key: string | null; camelot: string | null; color?: string | null };

/** How the "where do these files go" dialog should open. */
export interface AddFilesOptions {
  suggest?: ImportSuggest;
  kind?: ItemKind;
  mode?: "new" | "beat" | "inbox";
}

interface PlanRequest {
  plan: PlanView;
  /** Called after a successful run, e.g. to clear a selection. */
  onDone?: () => void;
  /** Runs right before the plan is executed (not when it is cancelled). */
  beforeRun?: () => void | Promise<void>;
}

interface Store {
  app: AppStateDto | null;
  screen: Screen;
  selectedBeatId: number | null;
  detailBeatId: number | null;
  play: PlayTarget | null;
  planRequest: PlanRequest | null;
  toast: { text: string; tone: "ok" | "error" } | null;
  scanning: boolean;
  /** Section of the library chosen in the sidebar. */
  libraryKind: ItemKind | "all";
  /** Player state mirrored for list rows. */
  playing: boolean;
  playProgress: number;
  /** Files waiting for "where to put them". */
  addFiles: ({ drop: DropDto; beatId: number | null } & AddFilesOptions) | null;
  /** Beat under the cursor while files are dragged over the window. */
  dropHover: { beatId: number | null; beatName: string | null } | null;

  refreshApp: () => Promise<void>;
  setConfig: (c: Config) => void;
  go: (s: Screen) => void;
  selectBeat: (id: number | null) => void;
  openDetail: (id: number | null) => void;
  setPlay: (p: PlayTarget | null) => void;
  showPlan: (plan: PlanView, onDone?: () => void, beforeRun?: () => void | Promise<void>) => void;
  closePlan: () => void;
  notify: (text: string, tone?: "ok" | "error") => void;
  setScanning: (v: boolean) => void;
  setLibraryKind: (k: ItemKind | "all") => void;
  openAddFiles: (drop: DropDto, beatId: number | null, opts?: AddFilesOptions) => void;
  closeAddFiles: () => void;
  setDropHover: (h: { beatId: number | null; beatName: string | null } | null) => void;
}

let toastTimer: ReturnType<typeof setTimeout> | undefined;

export const useStore = create<Store>((set, get) => ({
  app: null,
  screen: "library",
  selectedBeatId: null,
  detailBeatId: null,
  play: null,
  planRequest: null,
  toast: null,
  scanning: false,
  libraryKind: "all",
  playing: false,
  playProgress: 0,
  addFiles: null,
  dropHover: null,

  refreshApp: async () => {
    const app = await api.appState();
    set({ app });
    applyPrefs(app.config);
  },
  setConfig: (config) => {
    const app = get().app;
    if (app) set({ app: { ...app, config } });
    applyPrefs(config);
  },
  go: (screen) => set({ screen }),
  selectBeat: (id) => set({ selectedBeatId: id }),
  openDetail: (id) => set({ detailBeatId: id }),
  setPlay: (play) => set({ play }),
  showPlan: (plan, onDone, beforeRun) => set({ planRequest: { plan, onDone, beforeRun } }),
  closePlan: () => set({ planRequest: null }),
  notify: (text, tone = "ok") => {
    clearTimeout(toastTimer);
    set({ toast: { text, tone } });
    toastTimer = setTimeout(() => set({ toast: null }), tone === "error" ? 7000 : 3500);
  },
  setScanning: (scanning) => set({ scanning }),
  setLibraryKind: (libraryKind) => set({ libraryKind, detailBeatId: null }),
  openAddFiles: (drop, beatId, opts) => set({ addFiles: { drop, beatId, ...opts } }),
  closeAddFiles: () => set({ addFiles: null }),
  setDropHover: (dropHover) => set({ dropHover }),
}));

// ── theme & language ──

let systemQuery: MediaQueryList | null = null;
let systemListener: (() => void) | null = null;

function applyTheme(pref: ThemePref) {
  const root = document.documentElement;
  if (systemQuery && systemListener) {
    systemQuery.removeEventListener("change", systemListener);
    systemListener = null;
  }
  if (pref === "system") {
    systemQuery = window.matchMedia("(prefers-color-scheme: dark)");
    const sync = () => root.setAttribute("data-theme", systemQuery!.matches ? "dark" : "light");
    systemListener = sync;
    systemQuery.addEventListener("change", sync);
    sync();
  } else {
    root.setAttribute("data-theme", pref);
  }
}

function applyPrefs(c: Config) {
  applyTheme(c.theme);
  if (i18n.language !== c.lang) void i18n.changeLanguage(c.lang);
  document.documentElement.lang = c.lang;
}
