import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import type {
  AppStateDto,
  BatchRow,
  BeatDetail,
  BeatPatch,
  BeatRow,
  AddTarget,
  Config,
  DropDto,
  ExecResult,
  ImportDto,
  InboxItem,
  ItemKind,
  LibraryTemplate,
  Media,
  PlanView,
  RevealTarget,
  SortRequest,
  TemplatePatch,
  TreeLine,
} from "./types";

// The UI never sends file paths to commands that touch the disk: it sends IDs,
// and Rust resolves and checks the paths. Paths only travel the other way
// (for display and for the audio element).

export const api = {
  appState: () => invoke<AppStateDto>("get_app_state"),
  templates: () => invoke<LibraryTemplate[]>("list_templates"),
  previewTemplate: (templateId: string, rootName: string) =>
    invoke<TreeLine[]>("preview_template", { templateId, rootName }),
  planCreateLibrary: (req: {
    root: string;
    templateId: string;
    flFolders: string[];
    abletonFolders: string[];
    lang: string;
    label: string;
  }) => invoke<PlanView>("plan_create_library", { req }),
  executePlan: (planId: string) => invoke<ExecResult>("execute_plan", { planId }),
  discardPlan: (planId: string) => invoke<void>("discard_plan", { planId }),

  beats: (query: string) => invoke<BeatRow[]>("list_beats", { query }),
  updateBeat: (id: number, patch: BeatPatch) => invoke<BeatRow>("update_beat", { id, patch }),
  beatDetail: (id: number) => invoke<BeatDetail>("beat_detail", { id }),
  planAlignFolder: (id: number, label: string) => invoke<PlanView>("plan_align_folder", { id, label }),

  inbox: (ignored: boolean) => invoke<InboxItem[]>("list_inbox", { ignored }),
  setInboxIgnored: (ids: number[], ignored: boolean) => invoke<void>("set_inbox_ignored", { ids, ignored }),
  planSort: (req: SortRequest) => invoke<PlanView>("plan_sort", { req }),

  history: () => invoke<BatchRow[]>("list_history"),
  planUndo: (batchId: number | null, label: string) =>
    invoke<PlanView | null>("plan_undo", { batchId, label }),

  assetMedia: (id: number) => invoke<Media>("asset_media", { id }),
  inboxMedia: (id: number) => invoke<Media>("inbox_media", { id }),
  reveal: (target: RevealTarget) => invoke<void>("reveal", { target }),

  setPrefs: (patch: Partial<Pick<Config, "lang" | "theme" | "showBackups" | "producerName">>) =>
    invoke<Config>("set_prefs", { patch }),
  updateTemplate: (patch: TemplatePatch) => invoke<Config>("update_template", { patch }),
  renderPreview: (pattern: string) => invoke<string>("render_preview", { pattern }),

  /** Opens the system picker in Rust; the UI only gets an ID and file names. */
  pickFiles: (folders: boolean, title: string) => invoke<DropDto | null>("pick_files", { folders, title }),
  /** "New folder": an empty item; its folder and name come with the first files. */
  createItem: (kind: ItemKind, name: string) => invoke<BeatRow>("create_item", { kind, name }),
  /** Removes an empty item (one that has no files yet). */
  deleteDraft: (id: number) => invoke<void>("delete_draft", { id }),
  /** Plans sending an item's whole folder to the OS trash. */
  planDeleteItem: (id: number, label: string) => invoke<PlanView>("plan_delete_item", { id, label }),
  /** "Add folder" / "Add files": a folder's contents (or loose files) for one new item. */
  pickImport: (folder: boolean, title: string) => invoke<ImportDto | null>("pick_import", { folder, title }),
  planAddFiles: (req: { dropId: string; target: AddTarget; slots: Record<string, string>; label: string }) =>
    invoke<PlanView>("plan_add_files", { req }),
  /** Native drag of a library file out of the window (always a copy). */
  startDrag: (target: { type: "asset"; id: number } | { type: "beat"; id: number }) =>
    invoke<void>("start_drag", { target }),

  /** Opens an image picker in Rust and plans putting the picture in as the beat's cover. */
  planSetCover: (id: number, title: string, label: string) =>
    invoke<PlanView | null>("plan_set_cover", { id, title, label }),
  planRemoveCover: (id: number, label: string) => invoke<PlanView>("plan_remove_cover", { id, label }),
  /** Saves a copy of an MP3 with cover, title and credits in its tags.
   *  Returns the saved file's name, or null if the dialog was cancelled. */
  exportMp3: (id: number, title: string) => invoke<string | null>("export_mp3", { id, title }),

  /** Small waveforms for list rows, by audio file ID. */
  rowPeaks: (ids: number[]) => invoke<{ id: number; peaks: number[] }[]>("row_peaks", { ids }),
  libraryCounts: () => invoke<Record<ItemKind, number>>("library_counts"),
  setWatchFolders: (fl: string[], ableton: string[]) =>
    invoke<Config>("set_watch_folders", { fl, ableton }),
  rescan: () => invoke<void>("rescan"),
};

export async function pickFolder(title: string): Promise<string | null> {
  const picked = await open({ directory: true, multiple: false, title });
  return typeof picked === "string" ? picked : null;
}

export const fileUrl = (path: string) => convertFileSrc(path);

export type AppEvent = "library-changed" | "inbox-changed" | "beat-updated" | "scan-progress" | "drag-files";

export function onEvent<T = unknown>(name: AppEvent, cb: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(name, (e) => cb(e.payload));
}

/** Command errors arrive as plain strings from Rust. */
export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}
