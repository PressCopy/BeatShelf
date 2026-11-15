import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowUpDown, Pause, Play, Search, Star } from "lucide-react";
import { api, errorText, fileUrl } from "../api";
import { useStore, type PlayTarget } from "../store";
import { useDebounced, useReloadOn } from "../lib/useEvents";
import { beatColor, beatTint } from "../lib/camelot";
import { formatBpm, formatDuration, isTyping } from "../lib/format";
import { dragOut } from "../lib/dragOut";
import { playNow, seekTo, togglePlay } from "../components/Player";
import { MiniWave } from "../components/MiniWave";
import { DawMark, Empty, KeyChip, KIND_ICON } from "../components/ui";
import { STATUSES, type BeatPatch, type BeatRow, type BeatStatus } from "../types";

const ROW_H = 60;
const HEADER_H = 32;
const MIN_W = 1080;
// cover | name + tags | waveform | length | key | bpm | status | rating | daw
const GRID =
  "grid grid-cols-[40px_minmax(220px,2.4fr)_minmax(150px,1.5fr)_48px_108px_56px_140px_88px_52px] items-center gap-x-4 px-6";
const COLUMNS = ["", "name", "", "length", "key", "bpm", "status", "rating", "daw"] as const;

type TextField = "name" | "bpm" | "key" | "genre" | "mood" | "typeBeat";
type Sort = "recent" | "name" | "bpm";

function toPatch(field: TextField, raw: string): BeatPatch | { error: "bpm" } {
  const v = raw.trim();
  switch (field) {
    case "name":
      return v ? { name: v } : {};
    case "bpm": {
      if (!v) return { bpm: null };
      const n = Number(v.replace(",", "."));
      if (!Number.isFinite(n) || n < 40 || n > 300) return { error: "bpm" };
      return { bpm: n };
    }
    case "key":
      return { key: v || null };
    default:
      return { [field]: v || null } as BeatPatch;
  }
}

function playTarget(b: BeatRow): PlayTarget {
  return { type: "asset", id: b.previewAssetId!, title: b.name, bpm: b.bpm, key: b.key, camelot: b.camelot, color: b.color };
}

/** A value that turns into an input when clicked on the selected row
 *  (the first click only selects, like renaming in Finder). */
function EditableCell({
  beat,
  field,
  display,
  className = "w-full",
  chip = false,
  armed,
  onSaved,
}: {
  beat: BeatRow;
  field: TextField;
  display: ReactNode;
  className?: string;
  /** Small tag under the name; hidden when empty unless the row is selected. */
  chip?: boolean;
  armed: boolean;
  onSaved: (row: BeatRow) => void;
}) {
  const { t } = useTranslation();
  const notify = useStore((s) => s.notify);
  const [editing, setEditing] = useState(false);
  const initial = field === "bpm" ? formatBpm(beat.bpm) : ((beat[field] as string | null) ?? "");
  const [value, setValue] = useState(initial);
  const done = useRef(false);
  const label = t(`library.col.${field}`);

  const commit = async () => {
    if (done.current) return;
    done.current = true;
    setEditing(false);
    if (value.trim() === initial.trim()) return;
    const patch = toPatch(field, value);
    if ("error" in patch) {
      notify(t("library.badBpm"), "error");
      return;
    }
    try {
      onSaved(await api.updateBeat(beat.id, patch));
    } catch (e) {
      const msg = errorText(e);
      notify(msg.startsWith("bad_key:") ? t("library.badKey", { value: msg.slice(8) }) : msg, "error");
    }
  };

  const onKey = (e: ReactKeyboardEvent<HTMLInputElement>) => {
    e.stopPropagation();
    if (e.key === "Enter") void commit();
    if (e.key === "Escape") {
      done.current = true;
      setEditing(false);
    }
  };

  if (editing) {
    return (
      <input
        autoFocus
        value={value}
        onChange={(e) => setValue(e.target.value)}
        onBlur={() => void commit()}
        onKeyDown={onKey}
        onClick={(e) => e.stopPropagation()}
        onMouseDown={(e) => e.stopPropagation()}
        onDoubleClick={(e) => e.stopPropagation()}
        inputMode={field === "bpm" ? "decimal" : undefined}
        aria-label={label}
        placeholder={chip ? label : undefined}
        className={
          chip
            ? "h-5 w-32 rounded-full border border-focus bg-bg px-2 text-cap outline-none"
            : `h-7 rounded border border-focus bg-bg px-1.5 outline-none ${className}`
        }
      />
    );
  }
  if (chip && !display && !armed) return null;
  return (
    <button
      type="button"
      tabIndex={armed ? 0 : -1}
      onClick={(e) => {
        if (!armed) return; // first click selects the row
        e.stopPropagation();
        done.current = false;
        setValue(initial);
        setEditing(true);
      }}
      title={label}
      className={
        chip
          ? `h-5 max-w-[160px] shrink-0 truncate rounded-full px-2 text-cap ${
              display ? "bg-raised text-muted hover:text-text" : "border border-dashed border-line text-faint hover:text-text"
            }`
          : `h-7 truncate rounded px-1.5 text-left hover:bg-raised ${className}`
      }
    >
      {display || (chip ? `+ ${label.toLowerCase()}` : <span className="text-faint">{t("common.none")}</span>)}
    </button>
  );
}

function Rating({ beat, onSaved }: { beat: BeatRow; onSaved: (r: BeatRow) => void }) {
  const { t } = useTranslation();
  const set = async (n: number) => onSaved(await api.updateBeat(beat.id, { rating: beat.rating === n ? 0 : n }));
  return (
    <div className="flex" role="radiogroup" aria-label={t("library.col.rating")}>
      {[1, 2, 3, 4, 5].map((n) => (
        <button
          key={n}
          type="button"
          role="radio"
          tabIndex={-1}
          aria-checked={beat.rating === n}
          aria-label={String(n)}
          onClick={() => void set(n)}
          className="p-0.5 text-faint hover:text-text"
        >
          <Star size={14} fill={n <= beat.rating ? "currentColor" : "none"} className={n <= beat.rating ? "text-text" : ""} />
        </button>
      ))}
    </div>
  );
}

export function Library() {
  const { t } = useTranslation();
  const kind = useStore((s) => s.libraryKind);
  const selectedId = useStore((s) => s.selectedBeatId);
  const selectBeat = useStore((s) => s.selectBeat);
  const openDetail = useStore((s) => s.openDetail);
  const setPlay = useStore((s) => s.setPlay);
  const playingId = useStore((s) => (s.play?.type === "asset" ? s.play.id : null));
  const playing = useStore((s) => s.playing);
  const dropBeatId = useStore((s) => s.dropHover?.beatId ?? null);
  const [query, setQuery] = useState("");
  const [status, setStatus] = useState<BeatStatus | "">("");
  const [sort, setSort] = useState<Sort>("recent");
  const [rows, setRows] = useState<BeatRow[] | null>(null);
  const debounced = useDebounced(query, 150);
  const reload = useReloadOn(["library-changed", "beat-updated"]);
  const searchRef = useRef<HTMLInputElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let alive = true;
    api.beats(debounced).then((r) => alive && setRows(r)).catch(() => alive && setRows([]));
    return () => {
      alive = false;
    };
  }, [debounced, reload]);

  useEffect(() => {
    const focus = () => searchRef.current?.focus();
    window.addEventListener("beatshelf:focus-search", focus);
    return () => window.removeEventListener("beatshelf:focus-search", focus);
  }, []);

  // Jump back to the top when the section changes.
  useEffect(() => {
    scrollRef.current?.scrollTo({ top: 0 });
  }, [kind]);

  const inKind = useMemo(() => (rows ?? []).filter((r) => kind === "all" || r.kind === kind), [rows, kind]);
  const visible = useMemo(() => {
    const list = inKind.filter((r) => !status || r.status === status);
    if (sort === "name") return [...list].sort((a, b) => a.name.localeCompare(b.name));
    if (sort === "bpm") return [...list].sort((a, b) => (a.bpm ?? Infinity) - (b.bpm ?? Infinity));
    return list; // the backend returns the most recently changed first
  }, [inKind, status, sort]);

  const title = kind === "all" ? t("nav.library") : t(`item.plural.${kind}`);
  const countLabel =
    kind === "all"
      ? t("library.countAll", { count: visible.length })
      : kind === "beat"
        ? t("library.count", { count: visible.length })
        : kind === "loop"
          ? t("library.countLoop", { count: visible.length })
          : kind === "starter"
            ? t("library.countStarter", { count: visible.length })
            : t("library.countVoicetag", { count: visible.length });

  const replaceRow = useCallback((row: BeatRow) => {
    setRows((prev) => prev?.map((r) => (r.id === row.id ? row : r)) ?? prev);
  }, []);

  const virt = useVirtualizer({
    count: visible.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_H,
    overscan: 10,
    scrollMargin: HEADER_H,
  });

  const select = useCallback(
    (row: BeatRow) => {
      selectBeat(row.id);
      if (row.previewAssetId != null) {
        const p = useStore.getState().play;
        if (!(p?.type === "asset" && p.id === row.previewAssetId)) setPlay(playTarget(row));
      }
    },
    [selectBeat, setPlay],
  );

  // ↑ ↓ move the selection, Enter opens the card.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (isTyping(e) || document.querySelector('[role="dialog"]')) return;
      if (e.key !== "ArrowDown" && e.key !== "ArrowUp" && e.key !== "Enter") return;
      if (!visible.length) return;
      e.preventDefault();
      const idx = visible.findIndex((r) => r.id === selectedId);
      if (e.key === "Enter") {
        if (idx >= 0) openDetail(visible[idx].id);
        return;
      }
      // Drop focus left on a cell of the previous row so its ring doesn't linger.
      const active = document.activeElement as HTMLElement | null;
      if (active && scrollRef.current?.contains(active)) active.blur();
      const next = e.key === "ArrowDown" ? Math.min(visible.length - 1, idx + 1) : Math.max(0, idx - 1);
      select(visible[next]);
      virt.scrollToIndex(next);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [visible, selectedId, select, openDetail, virt]);

  const emptyText = !rows
    ? ""
    : inKind.length === 0 && !query
      ? kind === "all"
        ? t("library.empty")
        : t(`library.emptyKind.${kind}`)
      : t("library.noResults");

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="px-6 pb-4 pt-6">
        <div className="mb-4 flex items-baseline gap-3">
          <h1 className="text-hero font-semibold tracking-tight">{title}</h1>
          <span className="num text-muted">{rows ? countLabel : ""}</span>
        </div>
        <div className="flex items-center gap-2">
          <label className="relative flex-1">
            <Search size={16} className="pointer-events-none absolute left-4 top-3 text-faint" />
            <input
              ref={searchRef}
              type="search"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Escape") {
                  setQuery("");
                  (e.target as HTMLInputElement).blur();
                }
              }}
              placeholder={t("library.search")}
              aria-label={t("library.search")}
              className="h-10 w-full rounded-full border border-line bg-surface pl-11 pr-4 outline-none placeholder:text-faint focus:border-focus"
            />
          </label>
          <select
            value={status}
            onChange={(e) => setStatus(e.target.value as BeatStatus | "")}
            aria-label={t("library.col.status")}
            className={`h-10 rounded-full border bg-surface px-4 outline-none focus:border-focus ${
              status ? "border-accent-hi text-text" : "border-line text-muted"
            }`}
          >
            <option value="">{t("library.allStatuses")}</option>
            {STATUSES.map((s) => (
              <option key={s} value={s}>{t(`status.${s}`)}</option>
            ))}
          </select>
          <label className="relative">
            <ArrowUpDown size={14} className="pointer-events-none absolute left-4 top-[13px] text-faint" />
            <select
              value={sort}
              onChange={(e) => setSort(e.target.value as Sort)}
              aria-label={t("library.sort.label")}
              className="h-10 rounded-full border border-line bg-surface pl-10 pr-4 text-muted outline-none focus:border-focus"
            >
              <option value="recent">{t("library.sort.recent")}</option>
              <option value="name">{t("library.sort.name")}</option>
              <option value="bpm">{t("library.sort.bpm")}</option>
            </select>
          </label>
        </div>
      </header>

      <div ref={scrollRef} className="min-h-0 flex-1 overflow-auto" role="grid" aria-rowcount={visible.length}>
        <div style={{ minWidth: MIN_W }} className="h-full">
          <div
            className={`${GRID} sticky top-0 z-10 border-b border-line bg-bg text-cap text-faint`}
            style={{ height: HEADER_H }}
            role="row"
          >
            {COLUMNS.map((c, i) => (
              <div key={i} role="columnheader" className={c === "name" || c === "key" || c === "bpm" ? "pl-1.5" : ""}>
                {c ? t(`library.col.${c}`) : ""}
              </div>
            ))}
          </div>

          {rows && visible.length === 0 ? (
            <Empty>{emptyText}</Empty>
          ) : (
            <div style={{ height: virt.getTotalSize() - HEADER_H, position: "relative" }}>
              {virt.getVirtualItems().map((vi) => {
                const b = visible[vi.index];
                const selected = b.id === selectedId;
                const hasAudio = b.previewAssetId != null;
                const isCurrent = hasAudio && playingId === b.previewAssetId;
                const kc = beatColor(b.color, b.camelot);
                const KindIcon = KIND_ICON[b.kind];
                const tagged = b.kind !== "voicetag";
                const people = [...b.authors, ...b.coauthors].join(" x ");
                const onCover = () => {
                  select(b);
                  if (!hasAudio) return openDetail(b.id);
                  if (isCurrent) togglePlay();
                  else playNow(playTarget(b));
                };
                return (
                  <div
                    key={b.id}
                    role="row"
                    aria-selected={selected}
                    data-beat-id={b.id}
                    data-beat-name={b.name}
                    onMouseDown={hasAudio ? dragOut({ type: "beat", id: b.id }) : undefined}
                    onClick={() => select(b)}
                    onDoubleClick={() => {
                      select(b);
                      if (hasAudio) playNow(playTarget(b));
                      else openDetail(b.id);
                    }}
                    className={`${GRID} group absolute left-0 right-0 border-b border-line/40 ${
                      dropBeatId === b.id
                        ? "bg-[var(--row-selected)] outline outline-2 -outline-offset-2 outline-focus"
                        : selected
                          ? "bg-[var(--row-selected)]"
                          : "hover:bg-[var(--row-hover)]"
                    }`}
                    style={{ height: ROW_H, transform: `translateY(${vi.start - HEADER_H}px)` }}
                  >
                    <button
                      type="button"
                      tabIndex={-1}
                      aria-label={isCurrent && playing ? t("player.pause") : t("player.play")}
                      onMouseDown={(e) => e.stopPropagation()}
                      onClick={(e) => {
                        e.stopPropagation();
                        onCover();
                      }}
                      className="relative flex h-10 w-10 items-center justify-center overflow-hidden rounded-lg"
                      style={{ background: beatTint(b.color, b.camelot, 0.24) ?? "var(--raised)" }}
                    >
                      {b.cover && (
                        <>
                          <img
                            src={`${fileUrl(b.cover)}?v=${b.coverV ?? 0}`}
                            alt=""
                            draggable={false}
                            loading="lazy"
                            className="absolute inset-0 h-full w-full object-cover"
                          />
                          <span
                            className={`absolute inset-0 bg-black/50 ${
                              (isCurrent && playing) || selected ? "" : "hidden group-hover:block"
                            }`}
                          />
                        </>
                      )}
                      <span className={`relative flex items-center justify-center ${b.cover ? "text-white" : ""}`}>
                        {isCurrent && playing ? (
                          <>
                            <span className="eq group-hover:hidden" style={{ color: b.cover ? "#fff" : (kc ?? "var(--accent-hi)") }}>
                              <i />
                              <i />
                              <i />
                            </span>
                            <Pause size={16} className="hidden group-hover:block" />
                          </>
                        ) : (
                          <>
                            {!b.cover && (
                              <KindIcon
                                size={16}
                                className={selected ? "hidden" : "group-hover:hidden"}
                                style={{ color: kc ?? "var(--faint)" }}
                              />
                            )}
                            <Play size={16} className={`translate-x-[1px] ${selected ? "" : "hidden group-hover:block"}`} />
                          </>
                        )}
                      </span>
                    </button>

                    <div className="min-w-0">
                      <div className="flex min-w-0 items-center gap-1.5">
                        <EditableCell
                          beat={b}
                          field="name"
                          display={<span className="font-medium">{b.name}</span>}
                          className="min-w-0 shrink"
                          armed={selected}
                          onSaved={replaceRow}
                        />
                        {kind === "all" && b.kind !== "beat" && (
                          <span className="shrink-0 rounded-full border border-line px-1.5 text-[11px] text-muted">
                            {t(`item.${b.kind}`)}
                          </span>
                        )}
                        {people && (
                          <span className="min-w-0 shrink-[2] truncate text-cap text-muted" title={people}>
                            {people}
                          </span>
                        )}
                      </div>
                      {tagged && (
                        <div className="mt-0.5 flex min-w-0 items-center gap-1.5 overflow-hidden pl-1.5">
                          <EditableCell beat={b} field="genre" display={b.genre} chip armed={selected} onSaved={replaceRow} />
                          <EditableCell beat={b} field="mood" display={b.mood} chip armed={selected} onSaved={replaceRow} />
                          <EditableCell beat={b} field="typeBeat" display={b.typeBeat} chip armed={selected} onSaved={replaceRow} />
                        </div>
                      )}
                    </div>

                    <MiniWave
                      assetId={b.previewAssetId}
                      color={kc ?? "var(--accent-hi)"}
                      active={isCurrent}
                      onPick={(f) => {
                        if (!hasAudio) return;
                        if (isCurrent) seekTo(f);
                        else playNow(playTarget(b));
                      }}
                    />

                    <div className="num text-muted">{formatDuration(b.durationMs)}</div>

                    {tagged ? (
                      <>
                        <EditableCell
                          beat={b}
                          field="key"
                          display={b.key ? <KeyChip keyName={b.key} camelot={b.camelot} /> : null}
                          armed={selected}
                          onSaved={replaceRow}
                        />
                        <EditableCell beat={b} field="bpm" display={formatBpm(b.bpm)} className="num w-full" armed={selected} onSaved={replaceRow} />
                        <select
                          value={b.status}
                          tabIndex={selected ? 0 : -1}
                          onMouseDown={(e) => e.stopPropagation()}
                          onChange={async (e) => replaceRow(await api.updateBeat(b.id, { status: e.target.value as BeatStatus }))}
                          aria-label={t("library.col.status")}
                          className="h-7 w-full rounded-full border border-line bg-transparent px-2.5 text-cap outline-none hover:border-muted focus:border-focus"
                        >
                          {STATUSES.map((s) => (
                            <option key={s} value={s}>{t(`status.${s}`)}</option>
                          ))}
                        </select>
                      </>
                    ) : (
                      <>
                        <div />
                        <div />
                        <div />
                      </>
                    )}

                    <Rating beat={b} onSaved={replaceRow} />
                    <div className="flex gap-1">
                      {b.hasFl && (
                        <DawMark
                          daw="fl"
                          latest={b.latestDaw === "fl" && b.hasAbleton}
                          title={b.latestDaw === "fl" ? `FL Studio, ${t("library.latest")}` : "FL Studio"}
                        />
                      )}
                      {b.hasAbleton && (
                        <DawMark
                          daw="ableton"
                          latest={b.latestDaw === "ableton" && b.hasFl}
                          title={b.latestDaw === "ableton" ? `Ableton Live, ${t("library.latest")}` : "Ableton Live"}
                        />
                      )}
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
