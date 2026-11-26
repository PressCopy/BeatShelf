import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Download, FilePlus, FolderOpen, FolderPlus, GripVertical, ImagePlus, Play, Plus, Trash, X } from "lucide-react";
import { api, errorText, fileUrl } from "../api";
import { BEAT_COLORS, beatTint } from "../lib/camelot";
import { useStore } from "../store";
import { useReloadOn } from "../lib/useEvents";
import { formatBpm, formatDuration, formatSize } from "../lib/format";
import { dragOut } from "../lib/dragOut";
import { MetadataField } from "../components/MetadataField";
import { playNow } from "../components/Player";
import { Button, DawMark, IconButton, KeyChip, Segmented } from "../components/ui";
import { KINDS, type AssetKind, type AssetRow, type BeatDetail, type BeatRow, type ItemKind } from "../types";

const ORDER: AssetKind[] = ["fl_project", "fl_zip", "ableton_project", "ableton_set", "audio", "midi", "document", "image", "folder", "other"];

const splitPeople = (s: string) =>
  s
    .split(",")
    .map((x) => x.trim())
    .filter(Boolean);

/** Folder inside the beat folder ("Contracts", "Bounces"), from the stored path. */
function subfolder(asset: AssetRow, beatFolder: string | null): string | null {
  if (!beatFolder) return null;
  const rest = asset.path.slice(beatFolder.length + 1).split("/");
  return rest.length > 1 ? rest[0] : null;
}

function PeopleField({ beat, field, label }: { beat: BeatRow; field: "authors" | "coauthors"; label: string }) {
  const { t } = useTranslation();
  const notify = useStore((s) => s.notify);
  const initial = beat[field].join(", ");
  const [value, setValue] = useState(initial);
  useEffect(() => setValue(initial), [initial]);
  const save = async () => {
    if (value.trim() === initial) return;
    try {
      await api.updateBeat(beat.id, { [field]: splitPeople(value) });
    } catch (e) {
      notify(errorText(e), "error");
    }
  };
  return (
    <label className="block">
      <span className="mb-1 block text-cap text-muted">{label}</span>
      <input
        value={value}
        onChange={(e) => setValue(e.target.value)}
        onBlur={() => void save()}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === "Enter") (e.target as HTMLInputElement).blur();
          if (e.key === "Escape") setValue(initial);
        }}
        placeholder={t("detail.peopleHint")}
        className="h-8 w-full rounded-md border border-line bg-bg px-2.5 outline-none placeholder:text-faint focus:border-muted"
      />
    </label>
  );
}

/** Right-hand card for one beat, loop or starter. */
export function BeatDrawer() {
  const { t } = useTranslation();
  const id = useStore((s) => s.detailBeatId);
  const close = useStore((s) => s.openDetail);
  const showPlan = useStore((s) => s.showPlan);
  const notify = useStore((s) => s.notify);
  const openAddFiles = useStore((s) => s.openAddFiles);
  const dropHere = useStore((s) => s.dropHover?.beatId != null && s.dropHover.beatId === s.detailBeatId);
  const [detail, setDetail] = useState<BeatDetail | null>(null);
  const reload = useReloadOn(["library-changed", "beat-updated"]);
  const colorTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  useEffect(() => {
    if (id == null) {
      setDetail(null);
      return;
    }
    let alive = true;
    api.beatDetail(id).then((d) => alive && setDetail(d)).catch(() => alive && close(null));
    return () => {
      alive = false;
    };
  }, [id, reload, close]);

  useEffect(() => {
    if (id == null) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !document.querySelector('[role="dialog"]')) close(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [id, close]);

  if (id == null || !detail) return null;
  const { beat, assets } = detail;
  const projects = assets.filter((a) => a.daw && !a.isBackup);
  const newest = projects.reduce<AssetRow | null>((m, a) => (!m || (a.mtime ?? 0) > (m.mtime ?? 0) ? a : m), null);
  const sorted = [...assets].sort((a, b) => ORDER.indexOf(a.kind) - ORDER.indexOf(b.kind) || a.name.localeCompare(b.name));
  const wrongSection = detail.wrongSection;

  const align = async () => {
    try {
      const label = t("detail.moveLabel", { name: beat.name });
      showPlan(await api.planAlignFolder(beat.id, label));
    } catch (e) {
      notify(errorText(e), "error");
    }
  };

  const pick = async (folders: boolean) => {
    try {
      const drop = await api.pickFiles(folders, t("add.pickTitle", { name: beat.name }));
      if (drop) openAddFiles(drop, beat.id);
    } catch (e) {
      notify(errorText(e), "error");
    }
  };

  // The newest MP3 is the one "Download" sends out.
  const mp3 = assets
    .filter((a) => !a.isBackup && !a.isDir && /\.mp3$/i.test(a.name))
    .sort((x, y) => (y.mtime ?? 0) - (x.mtime ?? 0))[0];

  const pickCover = async () => {
    try {
      const plan = await api.planSetCover(
        beat.id,
        t("cover.pickTitle", { name: beat.name }),
        t("cover.labelSet", { name: beat.name }),
      );
      if (plan) showPlan(plan);
    } catch (e) {
      const m = errorText(e);
      notify(m === "bad_image" ? t("cover.badImage") : m, "error");
    }
  };

  const removeCover = async () => {
    try {
      showPlan(await api.planRemoveCover(beat.id, t("cover.labelRemove", { name: beat.name })));
    } catch (e) {
      notify(errorText(e), "error");
    }
  };

  const download = async (assetId: number) => {
    try {
      const saved = await api.exportMp3(assetId, t("cover.saveTitle"));
      if (saved) notify(t("cover.saved", { name: saved }));
    } catch (e) {
      const m = errorText(e);
      notify(m === "export_inside_library" ? t("cover.insideLibrary") : m, "error");
    }
  };

  // The system colour picker fires while you drag in it, so wait for a pause.
  const setColor = (color: string | null, wait = 0) => {
    clearTimeout(colorTimer.current);
    colorTimer.current = setTimeout(() => {
      api.updateBeat(beat.id, { color }).catch((e) => notify(errorText(e), "error"));
    }, wait);
  };

  const removeDraft = async () => {
    try {
      await api.deleteDraft(beat.id);
      close(null);
    } catch (e) {
      notify(errorText(e), "error");
    }
  };

  const setKind = async (kind: ItemKind) => {
    try {
      await api.updateBeat(beat.id, { kind });
    } catch (e) {
      const m = errorText(e);
      notify(m === "voicetag_file_type" ? t("issue.voicetag_file_type") : m, "error");
    }
  };

  return (
    <aside
      aria-label={beat.name}
      data-beat-id={beat.id}
      data-beat-name={beat.name}
      className={`slide-in absolute bottom-0 right-0 top-0 z-30 flex w-[420px] flex-col border-l bg-surface shadow-2xl ${
        dropHere ? "border-focus outline outline-2 -outline-offset-2 outline-focus" : "border-line"
      }`}
    >
      <header className="flex items-start gap-3 border-b border-line px-5 py-4">
        <div className="min-w-0 flex-1">
          <h2 className="truncate text-title font-semibold">{beat.name}</h2>
          <div className="mt-1 flex flex-wrap items-center gap-2 text-muted">
            {beat.bpm != null && <span className="num">{formatBpm(beat.bpm)} BPM</span>}
            <KeyChip keyName={beat.key} camelot={beat.camelot} />
            <span>{t(`status.${beat.status}`)}</span>
          </div>
        </div>
        <IconButton label={t("common.close")} onClick={() => close(null)}>
          <X size={16} />
        </IconButton>
      </header>

      <div className="min-h-0 flex-1 overflow-auto px-5 py-4">
        <section className="mb-6 flex flex-col gap-3">
          <div>
            <span className="mb-1 block text-cap text-muted">{t("detail.kind")}</span>
            <Segmented<ItemKind>
              value={beat.kind}
              onChange={(k) => void setKind(k)}
              options={KINDS.map((k) => ({ value: k, label: t(`item.${k}`) }))}
            />
          </div>
          <MetadataField key={beat.id} beat={beat} field="name" />
          {beat.kind !== "voicetag" && <div className="grid grid-cols-2 gap-3">
            <MetadataField key={beat.id + "-bpm"} beat={beat} field="bpm" />
            <MetadataField key={beat.id + "-key"} beat={beat} field="key" />
          </div>}
          <div className="grid grid-cols-2 gap-3">
            <PeopleField beat={beat} field="authors" label={t("detail.authors")} />
            <PeopleField beat={beat} field="coauthors" label={t("detail.coauthors")} />
          </div>
        </section>

        {beat.kind !== "voicetag" && beat.folder && (
          <section className="mb-6 flex gap-4">
            <div
              className="flex h-24 w-24 shrink-0 items-center justify-center overflow-hidden rounded-xl text-faint"
              style={{ background: beatTint(beat.color, beat.camelot, 0.24) ?? "var(--raised)" }}
            >
              {beat.cover ? (
                <img
                  src={`${fileUrl(beat.cover)}?v=${beat.coverV ?? 0}`}
                  alt=""
                  draggable={false}
                  className="h-full w-full object-cover"
                />
              ) : (
                <ImagePlus size={24} />
              )}
            </div>
            <div className="min-w-0 flex-1">
              <h3 className="font-semibold">{t("cover.title")}</h3>
              <p className="mb-2 text-cap text-muted">{t("cover.hint")}</p>
              <div className="flex flex-wrap items-center gap-2">
                <Button onClick={() => void pickCover()}>
                  <ImagePlus size={14} />
                  {beat.cover ? t("cover.replace") : t("cover.choose")}
                </Button>
                {beat.cover && (
                  <IconButton label={t("cover.remove")} onClick={() => void removeCover()}>
                    <Trash size={16} />
                  </IconButton>
                )}
                {mp3 && (
                  <Button tone="primary" onClick={() => void download(mp3.id)}>
                    <Download size={14} />
                    {t("cover.download")}
                  </Button>
                )}
              </div>
              {!mp3 && <p className="mt-2 text-cap text-faint">{t("cover.noMp3")}</p>}
            </div>
          </section>
        )}

        <section className="mb-6">
          <span className="mb-1.5 block text-cap text-muted">{t("color.title")}</span>
          <div className="flex flex-wrap items-center gap-2" role="radiogroup" aria-label={t("color.title")}>
            <button
              type="button"
              role="radio"
              aria-checked={!beat.color}
              onClick={() => setColor(null)}
              className={`h-6 rounded-full border px-2.5 text-cap ${
                beat.color ? "border-line text-muted hover:text-text" : "border-text text-text"
              }`}
            >
              {t("color.auto")}
            </button>
            {BEAT_COLORS.map((c) => {
              const on = beat.color?.toLowerCase() === c;
              return (
                <button
                  key={c}
                  type="button"
                  role="radio"
                  aria-checked={on}
                  aria-label={c}
                  onClick={() => setColor(c)}
                  className={`h-6 w-6 rounded-full transition-transform hover:scale-110 ${
                    on ? "ring-2 ring-text ring-offset-2 ring-offset-surface" : ""
                  }`}
                  style={{ background: c }}
                />
              );
            })}
            <label
              className="relative flex h-6 w-6 cursor-pointer items-center justify-center overflow-hidden rounded-full border border-dashed border-muted text-muted hover:text-text"
              title={t("color.custom")}
            >
              <Plus size={12} />
              <input
                type="color"
                value={beat.color ?? "#6f8cff"}
                onChange={(e) => setColor(e.target.value, 250)}
                aria-label={t("color.custom")}
                className="absolute inset-0 h-full w-full cursor-pointer opacity-0"
              />
            </label>
          </div>
        </section>

        {!beat.folder && (
          <section className="mb-6 rounded-xl border border-dashed border-line px-4 py-4">
            <h3 className="font-semibold">{t("draft.boxTitle")}</h3>
            <p className="mb-3 mt-1 text-muted">{t("draft.boxText")}</p>
            <Button onClick={() => void removeDraft()}>
              <Trash size={14} />
              {t("draft.delete")}
            </Button>
          </section>
        )}

        <section className={beat.folder ? "mb-6" : "hidden"}>
          <h3 className="mb-2 font-semibold">{t("detail.folder")}</h3>
          <div className="flex items-center gap-2">
            <code className="min-w-0 flex-1 truncate text-muted" title={beat.folder ?? ""}>{beat.folder?.split("/").at(-1)}</code>
            <IconButton label={t("common.reveal")} onClick={() => void api.reveal({ type: "beat", id: beat.id })}>
              <FolderOpen size={16} />
            </IconButton>
          </div>
          {wrongSection && (
            <div className="mt-2 rounded-md bg-warn-bg px-3 py-2">
              <p>{t("detail.wrongSection", { path: detail.expectedFolder })}</p>
              <Button className="mt-2" onClick={() => void align()}>{t("detail.moveFolder")}</Button>
            </div>
          )}
        </section>

        <section>
          <div className="mb-2 flex items-center gap-2">
            <h3 className="mr-auto font-semibold">{t("detail.files")}</h3>
            <Button onClick={() => void pick(false)}>
              <FilePlus size={14} />
              {t("detail.addFiles")}
            </Button>
            <IconButton label={t("detail.addFolder")} onClick={() => void pick(true)}>
              <FolderPlus size={16} />
            </IconButton>
          </div>
          {sorted.length === 0 ? (
            beat.folder && <p className="text-muted">{t("detail.noFiles")}</p>
          ) : (
            <ul className="flex flex-col">
              {sorted.map((a) => {
                const sub = subfolder(a, beat.folder);
                return (
                  <li
                    key={a.id}
                    onMouseDown={dragOut({ type: "asset", id: a.id })}
                    title={t("detail.dragHint")}
                    className="group flex cursor-grab items-center gap-2 border-b border-line/60 py-2 active:cursor-grabbing"
                  >
                    <GripVertical size={14} className="shrink-0 text-faint opacity-0 group-hover:opacity-100" />
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2">
                        {a.daw && <DawMark daw={a.daw} latest={newest?.id === a.id && projects.length > 1} />}
                        <span className="truncate" title={a.path}>{a.name}</span>
                      </div>
                      <div className="num truncate text-cap text-muted">
                        {sub ? `${sub}, ` : ""}
                        {t(`kind.${a.kind}`)}
                        {a.isBackup && ` (${t("detail.backup")})`}
                        {a.durationMs != null && `, ${formatDuration(a.durationMs)}`}
                        {a.size != null && !a.isDir && `, ${formatSize(a.size)}`}
                        {newest?.id === a.id && projects.length > 1 && `, ${t("library.latest")}`}
                      </div>
                    </div>
                    {beat.kind !== "voicetag" && !a.isDir && /\.mp3$/i.test(a.name) && (
                      <IconButton
                        label={t("cover.download")}
                        onMouseDown={(e) => e.stopPropagation()}
                        onClick={() => void download(a.id)}
                      >
                        <Download size={16} />
                      </IconButton>
                    )}
                    {a.kind === "audio" && (
                      <IconButton
                        label={t("player.play")}
                        onMouseDown={(e) => e.stopPropagation()}
                        onClick={() => playNow({ type: "asset", id: a.id, title: beat.name, bpm: beat.bpm, key: beat.key, camelot: beat.camelot, color: beat.color, beatId: beat.id })}
                      >
                        <Play size={16} />
                      </IconButton>
                    )}
                    <IconButton
                      label={t("common.reveal")}
                      onMouseDown={(e) => e.stopPropagation()}
                      onClick={() => void api.reveal({ type: "asset", id: a.id })}
                    >
                      <FolderOpen size={16} />
                    </IconButton>
                  </li>
                );
              })}
            </ul>
          )}
        </section>
      </div>
    </aside>
  );
}
