// Mirrors of the Rust DTOs (src-tauri/src/commands.rs, crates/beatcore/src/db.rs).
// Keep field names in sync with `#[serde(rename_all = "camelCase")]`.

export type Lang = "ru" | "en";
export type ThemePref = "dark" | "light" | "system";

export type BeatStatus = "idea" | "in_progress" | "ready" | "sent" | "leased" | "sold_exclusive";
export const STATUSES: BeatStatus[] = ["idea", "in_progress", "ready", "sent", "leased", "sold_exclusive"];

export type Daw = "fl" | "ableton";
export type AssetKind =
  | "fl_project"
  | "fl_zip"
  | "ableton_project"
  | "ableton_set"
  | "audio"
  | "midi"
  | "image"
  | "document"
  | "folder"
  | "other";
export type Source = "inbox" | "fl" | "ableton";
export type Slot = "project_fl" | "project_ableton" | "bounces" | "stems" | "contracts" | "artwork" | "notes";
export const SLOTS: Slot[] = ["project_fl", "project_ableton", "bounces", "stems", "contracts", "artwork", "notes"];

/** Beats, loops and starters share everything except their section of the library. */
export type ItemKind = "beat" | "loop" | "starter" | "voicetag";
export const KINDS: ItemKind[] = ["beat", "loop", "starter", "voicetag"];

export interface WatchFolder {
  path: string;
  since: number;
}

export interface BeatSubdirs {
  projectFl: string;
  projectAbleton: string;
  bounces: string;
  stems: string;
  artwork: string;
  notes: string;
  contracts: string;
}

export interface LibraryTemplate {
  id: string;
  name: string;
  inbox: string;
  archive: string;
  staticDirs: string[];
  beatDir: string;
  beatSubdirs: BeatSubdirs;
  fileName: string;
  loopDir: string;
  starterDir: string;
  voicetagDir: string;
}

export interface Config {
  root: string | null;
  template: LibraryTemplate | null;
  flFolders: WatchFolder[];
  abletonFolders: WatchFolder[];
  lang: Lang;
  theme: ThemePref;
  showBackups: boolean;
  numberFrom: number;
  numberBeats: boolean;
  producerName: string | null;
}

export interface AppStateDto {
  onboarded: boolean;
  config: Config;
  inboxCount: number;
  platform: string;
}

export interface TreeLine {
  depth: number;
  name: string;
  role: "root" | "dir" | "example" | "beat" | "sub";
}

export interface BeatRow {
  id: number;
  name: string;
  folder: string | null;
  bpm: number | null;
  key: string | null;
  camelot: string | null;
  genre: string | null;
  mood: string | null;
  typeBeat: string | null;
  status: BeatStatus;
  rating: number;
  color: string | null;
  createdAt: number;
  updatedAt: number;
  hasFl: boolean;
  hasAbleton: boolean;
  latestDaw: Daw | null;
  previewAssetId: number | null;
  durationMs: number | null;
  kind: ItemKind;
  authors: string[];
  coauthors: string[];
  /** Absolute path of the cover image, if the beat has one. */
  cover: string | null;
  /** Changes with the cover file; appended to its URL to refresh the picture. */
  coverV: number | null;
}

/** Absent field = unchanged, `null` = clear. */
export interface BeatPatch {
  name?: string;
  bpm?: number | null;
  key?: string | null;
  genre?: string | null;
  mood?: string | null;
  typeBeat?: string | null;
  status?: BeatStatus;
  rating?: number;
  color?: string | null;
  kind?: ItemKind;
  authors?: string[];
  coauthors?: string[];
}

export interface AssetRow {
  id: number;
  beatId: number | null;
  path: string;
  name: string;
  kind: AssetKind;
  daw: Daw | null;
  isDir: boolean;
  isBackup: boolean;
  size: number | null;
  mtime: number | null;
  durationMs: number | null;
  sampleRate: number | null;
  channels: number | null;
  codec: string | null;
}

export interface BeatDetail {
  beat: BeatRow;
  assets: AssetRow[];
  wrongSection: boolean;
  expectedFolder: string;
}

export interface InboxItem {
  folderName: string | null;
  authors: string[];
  id: number;
  path: string;
  name: string;
  source: Source;
  kind: AssetKind;
  daw: Daw | null;
  isDir: boolean;
  size: number | null;
  mtime: number | null;
  title: string | null;
  bpm: number | null;
  key: string | null;
  camelot: string | null;
  tagged: boolean | null;
  state: "pending" | "ignored";
  detectedAt: number;
}

export type IssueCode =
  | "source_missing"
  | "destination_exists"
  | "destination_duplicate"
  | "outside_library"
  | "into_itself"
  | "ableton_tear"
  | "cloud_placeholder"
  | "parent_missing"
  | "not_a_directory"
  | "long_path"
  | "ableton_external_samples"
  | "ableton_loose_set"
  | "fl_sample_paths"
  | "maybe_open_in_daw"
  | "dir_not_empty"
  | "in_trash"
  | "deletes_files"
  | "voicetag_file_type";

export interface Issue {
  code: IssueCode;
  path: string | null;
  detail: string | null;
  items: string[];
}

export interface StepView {
  kind: "create_dir" | "move" | "rename" | "remove_dir" | "copy" | "trash";
  from: string | null;
  to: string;
}

export interface PlanView {
  id: string;
  label: string;
  steps: StepView[];
  warnings: Issue[];
  blockers: Issue[];
  isUndo: boolean;
}

export interface ExecResult {
  batchId: number | null;
  moved: number;
  created: number;
  skipped: Issue[];
}

export interface BatchRow {
  id: number;
  label: string;
  createdAt: number;
  status: "running" | "done" | "rolled_back" | "failed";
  undoOf: number | null;
  undoneBy: number | null;
  opCount: number;
  canUndo: boolean;
}

export interface Media {
  path: string;
  peaks: number[];
  durationMs: number | null;
}

export type SortTarget =
  | { type: "existing"; beatId: number }
  | {
      type: "new";
      name: string;
      bpm: number | null;
      key: string | null;
      kind: ItemKind;
      authors: string[];
      coauthors: string[];
    };

export interface SortRequest {
  inboxIds: number[];
  target: SortTarget;
  slots: Record<string, Slot>;
  label: string;
}

export type RevealTarget =
  | { type: "asset"; id: number }
  | { type: "beat"; id: number }
  | { type: "inbox"; id: number }
  | { type: "root" };

/** Files dropped on the window or picked in a dialog. Paths stay in Rust. */
export interface DropItem {
  index: number;
  name: string;
  kind: AssetKind;
  isDir: boolean;
  size: number | null;
  slot: Slot;
}

export interface DropDto {
  folderSuggestion: ImportSuggest | null;
  dropId: string;
  items: DropItem[];
}

export type DragFilesEvent =
  | { type: "over"; x: number; y: number }
  | { type: "leave" }
  | { type: "drop"; x: number; y: number; drop: DropDto };

export type AddTarget =
  | { type: "beat"; beatId: number }
  | { type: "inbox" }
  | {
      type: "new";
      name: string;
      bpm: number | null;
      key: string | null;
      kind: ItemKind;
      authors: string[];
      coauthors: string[];
    };

/** What the name of a picked folder or file says about the item. */
export interface ImportSuggest {
  userFolder: string;
  name: string;
  bpm: number | null;
  key: string | null;
  authors: string[];
}

export interface ImportDto extends ImportSuggest {
  drop: DropDto;
}

export interface TemplatePatch {
  fileName?: string;
  beatDir?: string;
  loopDir?: string;
  starterDir?: string;
  voicetagDir?: string;
}

/** What a voice tag may hold: WAV, MP3 and DAW projects. Mirrors `ItemKind::accepts` in Rust. */
export function voicetagAccepts(kind: AssetKind, name: string): boolean {
  if (kind === "audio") return /\.(wav|mp3)$/i.test(name);
  return kind === "fl_project" || kind === "fl_zip" || kind === "ableton_project" || kind === "ableton_set";
}
