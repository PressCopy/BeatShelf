import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { FileMusic, FileText, Folder, FolderOpen, Image, Layers, Play } from "lucide-react";
import { api, errorText } from "../api";
import { useStore } from "../store";
import { useDebounced, useReloadOn } from "../lib/useEvents";
import { formatBpm, formatDate, formatSize } from "../lib/format";
import { playNow } from "../components/Player";
import { Button, DawMark, Dialog, Empty, IconButton, KeyChip, Label, Segmented, TextInput } from "../components/ui";
import { KINDS, SLOTS, voicetagAccepts, type AssetKind, type BeatRow, type InboxItem, type ItemKind, type Slot } from "../types";

const CONTRACT_WORDS = ["contract", "контракт", "договор", "license", "licence", "лиценз", "agreement", "split"];
const TRACKOUT_WORDS = ["stem", "trackout", "track out", "стем", "трекаут", "multitrack"];

/** Same guess as `suggest_slot` in Rust: contracts and trackouts by name. */
export function defaultSlot(kind: AssetKind, name = ""): Slot {
  const n = name.toLowerCase();
  const ext = n.includes(".") ? n.split(".").pop()! : "";
  if (CONTRACT_WORDS.some((w) => n.includes(w)) || ["pdf", "doc", "docx", "odt", "pages"].includes(ext)) return "contracts";
  if (TRACKOUT_WORDS.some((w) => n.includes(w)) || (ext === "zip" && kind !== "fl_zip")) return "stems";
  switch (kind) {
    case "fl_project":
    case "fl_zip":
      return "project_fl";
    case "ableton_project":
    case "ableton_set":
      return "project_ableton";
    case "audio":
      return "bounces";
    case "image":
      return "artwork";
    case "midi":
    case "folder":
      return "stems";
    default:
      return "notes";
  }
}

function KindIcon({ item }: { item: InboxItem }) {
  if (item.daw) return <DawMark daw={item.daw} />;
  const Icon = item.kind === "audio" ? FileMusic : item.kind === "image" ? Image : item.kind === "folder" ? Folder : item.kind === "midi" ? Layers : FileText;
  return <Icon size={16} className="text-muted" />;
}

function SortDialog({ items, onClose, onPlanned }: { items: InboxItem[]; onClose: () => void; onPlanned: () => void }) {
  const { t } = useTranslation();
  const showPlan = useStore((s) => s.showPlan);
  const first = items.find((i) => i.title) ?? items[0];
  const [mode, setMode] = useState<"new" | "existing">("new");
  const [name, setName] = useState(first?.title ?? "");
  const [bpm, setBpm] = useState(formatBpm(items.find((i) => i.bpm != null)?.bpm));
  const [key, setKey] = useState(items.find((i) => i.key)?.key ?? "");
  const [rename, setRename] = useState(true);
  const [kind, setKind] = useState<ItemKind>("beat");
  const [authors, setAuthors] = useState(useStore.getState().app?.config.producerName ?? "");
  const [coauthors, setCoauthors] = useState("");
  const [slots, setSlots] = useState<Record<string, Slot>>(() =>
    Object.fromEntries(items.map((i) => [String(i.id), defaultSlot(i.kind, i.name)])),
  );
  const [query, setQuery] = useState("");
  const debounced = useDebounced(query, 150);
  const [found, setFound] = useState<BeatRow[]>([]);
  const [beatId, setBeatId] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (mode !== "existing") return;
    let alive = true;
    api.beats(debounced).then((r) => alive && setFound(r.slice(0, 50)));
    return () => {
      alive = false;
    };
  }, [debounced, mode]);

  const submit = async () => {
    setError(null);
    let target;
    let label;
    if (mode === "new") {
      if (!name.trim()) return setError(t("sort.needName"));
      const n = bpm.trim() ? Number(bpm.replace(",", ".")) : null;
      if (n != null && (!Number.isFinite(n) || n < 40 || n > 300)) return setError(t("library.badBpm"));
      const people = (v: string) => v.split(",").map((x) => x.trim()).filter(Boolean);
      target = {
        type: "new" as const,
        name: name.trim(),
        bpm: kind === "voicetag" ? null : n,
        key: kind === "voicetag" ? null : key.trim() || null,
        kind,
        authors: kind === "voicetag" ? [] : people(authors),
        coauthors: kind === "voicetag" ? [] : people(coauthors),
      };
      label = t("sort.label", { name: name.trim() });
    } else {
      const beat = found.find((b) => b.id === beatId);
      if (!beat) return setError(t("sort.needBeat"));
      target = { type: "existing" as const, beatId: beat.id };
      label = t("sort.label", { name: beat.name });
    }
    setBusy(true);
    try {
      const plan = await api.planSort({ inboxIds: items.map((i) => i.id), target, rename, slots, label });
      onClose();
      showPlan(plan, onPlanned);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  // Voice tags are flat (no subfolders) and take only WAV and MP3.
  const flat = mode === "new" ? kind === "voicetag" : found.find((b) => b.id === beatId)?.kind === "voicetag";

  return (
    <Dialog
      title={t("sort.title")}
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>{t("common.cancel")}</Button>
          <Button tone="primary" onClick={() => void submit()} disabled={busy}>{t("sort.makePlan")}</Button>
        </>
      }
    >
      <div className="mb-4">
        <Segmented
          value={mode}
          onChange={setMode}
          options={[
            { value: "new", label: t("sort.newBeat") },
            { value: "existing", label: t("sort.existing") },
          ]}
        />
      </div>

      {mode === "new" ? (
        <>
        <div className="mb-3">
          <Segmented<ItemKind>
            value={kind}
            onChange={(k) => {
              setKind(k);
              // "Maks Tag.wav" is parsed as title "Maks" + a "tagged" marker;
              // for a voice tag the whole file name is the name.
              if (k === "voicetag" && first && name === (first.title ?? "")) setName(first.name.replace(/\.[^.]+$/, ""));
            }}
            options={KINDS.map((k) => ({ value: k, label: t(`item.${k}`) }))}
          />
        </div>
        <div className={`mb-3 grid gap-3 ${kind === "voicetag" ? "grid-cols-1" : "grid-cols-[1fr_96px_120px]"}`}>
          <label>
            <Label>{t("sort.name")}</Label>
            <TextInput value={name} onChange={(e) => setName(e.target.value)} />
          </label>
          <label className={kind === "voicetag" ? "hidden" : ""}>
            <Label>{t("sort.bpm")}</Label>
            <TextInput className="num" inputMode="decimal" value={bpm} onChange={(e) => setBpm(e.target.value)} />
          </label>
          <label className={kind === "voicetag" ? "hidden" : ""}>
            <Label>{t("sort.key")}</Label>
            <TextInput value={key} placeholder="Am" onChange={(e) => setKey(e.target.value)} />
          </label>
        </div>
        <div className={`mb-6 grid grid-cols-2 gap-3 ${kind === "voicetag" ? "hidden" : ""}`}>
          <label>
            <Label>{t("sort.authors")}</Label>
            <TextInput value={authors} placeholder={t("sort.peopleHint")} onChange={(e) => setAuthors(e.target.value)} />
          </label>
          <label>
            <Label>{t("sort.coauthors")}</Label>
            <TextInput value={coauthors} placeholder={t("sort.peopleHint")} onChange={(e) => setCoauthors(e.target.value)} />
          </label>
        </div>
        </>
      ) : (
        <div className="mb-6">
          <TextInput placeholder={t("sort.pickBeat")} value={query} onChange={(e) => setQuery(e.target.value)} />
          <ul className="mt-2 max-h-48 overflow-auto rounded-md border border-line" role="listbox">
            {found.map((b) => (
              <li key={b.id}>
                <button
                  type="button"
                  role="option"
                  aria-selected={beatId === b.id}
                  onClick={() => setBeatId(b.id)}
                  className={`flex w-full items-center gap-3 px-3 py-1.5 text-left ${beatId === b.id ? "bg-[var(--row-selected)]" : "hover:bg-[var(--row-hover)]"}`}
                >
                  <span className="flex-1 truncate">{b.name}</span>
                  <span className="num text-muted">{formatBpm(b.bpm)}</span>
                  <KeyChip keyName={b.key} camelot={b.camelot} />
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}

      <label className="mb-6 flex items-start gap-2">
        <input type="checkbox" className="mt-1" checked={rename} onChange={(e) => setRename(e.target.checked)} />
        <span>
          <span className="block">{t("sort.rename")}</span>
          <span className="block text-cap text-muted">{t("sort.renameHint")}</span>
        </span>
      </label>

      {flat ? <p className="mb-2 text-muted">{t("sort.voicetagHint")}</p> : <Label>{t("sort.whereEach")}</Label>}
      <ul className="flex flex-col gap-1">
        {items.map((i) => (
          <li key={i.id} className="flex items-center gap-3">
            <KindIcon item={i} />
            <span className="min-w-0 flex-1 truncate" title={i.path}>{i.name}</span>
            {flat && !voicetagAccepts(i.kind, i.name) && (
              <span className="shrink-0 text-cap text-danger">{t("sort.notAllowed")}</span>
            )}
            {!flat && <select
              value={slots[String(i.id)]}
              onChange={(e) => setSlots({ ...slots, [String(i.id)]: e.target.value as Slot })}
              aria-label={i.name}
              className="h-7 rounded border border-line bg-surface px-1 outline-none focus:border-focus"
            >
              {SLOTS.map((s) => (
                <option key={s} value={s}>{t(`slot.${s}`)}</option>
              ))}
            </select>}
          </li>
        ))}
      </ul>

      {error && <p className="mt-4 text-danger" role="alert">{error}</p>}
    </Dialog>
  );
}

export function Inbox() {
  const { t, i18n } = useTranslation();
  const notify = useStore((s) => s.notify);
  const refreshApp = useStore((s) => s.refreshApp);
  const [ignored, setIgnored] = useState(false);
  const [items, setItems] = useState<InboxItem[] | null>(null);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [sorting, setSorting] = useState(false);
  const reload = useReloadOn(["inbox-changed", "library-changed"]);

  useEffect(() => {
    let alive = true;
    api.inbox(ignored).then((r) => {
      if (!alive) return;
      setItems(r);
      setSelected((prev) => new Set([...prev].filter((id) => r.some((i) => i.id === id))));
    });
    return () => {
      alive = false;
    };
  }, [ignored, reload]);

  const chosen = useMemo(() => (items ?? []).filter((i) => selected.has(i.id)), [items, selected]);
  const toggle = (id: number) => {
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setSelected(next);
  };

  const setHidden = async (hide: boolean) => {
    try {
      await api.setInboxIgnored([...selected], hide);
      setSelected(new Set());
      void refreshApp();
    } catch (e) {
      notify(errorText(e), "error");
    }
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex items-center gap-4 border-b border-line px-4 py-3">
        <div className="min-w-0 flex-1">
          <h1 className="text-title font-semibold">{t("inbox.title")}</h1>
          <p className="max-w-[72ch] text-muted">{t("inbox.hint")}</p>
        </div>
        <Segmented
          value={ignored ? "ignored" : "pending"}
          onChange={(v) => {
            setIgnored(v === "ignored");
            setSelected(new Set());
          }}
          options={[
            { value: "pending", label: t("inbox.showPending") },
            { value: "ignored", label: t("inbox.showIgnored") },
          ]}
        />
      </div>

      <div className="min-h-0 flex-1 overflow-auto">
        {items && items.length === 0 ? (
          <Empty>{t("inbox.empty")}</Empty>
        ) : (
          <ul>
            {(items ?? []).map((i) => {
              const on = selected.has(i.id);
              return (
                <li
                  key={i.id}
                  className={`flex items-center gap-3 border-b border-line/60 px-4 py-2 ${on ? "bg-[var(--row-selected)]" : "hover:bg-[var(--row-hover)]"}`}
                  onClick={() => toggle(i.id)}
                >
                  <input type="checkbox" checked={on} onChange={() => toggle(i.id)} onClick={(e) => e.stopPropagation()} aria-label={i.name} />
                  <span className="flex w-7 justify-center"><KindIcon item={i} /></span>
                  <div className="min-w-0 flex-1">
                    <div className="truncate font-medium" title={i.path}>{i.name}</div>
                    <div className="text-cap text-muted">
                      {t(`kind.${i.kind}`)}, {t(`source.${i.source}`)}
                      {i.size != null && !i.isDir && `, ${formatSize(i.size)}`}
                      {`, ${formatDate(i.detectedAt, i18n.language)}`}
                    </div>
                  </div>
                  {(i.bpm != null || i.key) && (
                    <div className="flex items-center gap-2" title={t("inbox.parsed")}>
                      {i.bpm != null && <span className="num text-muted">{formatBpm(i.bpm)} BPM</span>}
                      <KeyChip keyName={i.key} camelot={i.camelot} />
                    </div>
                  )}
                  {i.kind === "audio" && (
                    <IconButton
                      label={t("player.play")}
                      onClick={(e) => {
                        e.stopPropagation();
                        playNow({ type: "inbox", id: i.id, title: i.title ?? i.name, bpm: i.bpm, key: i.key, camelot: i.camelot });
                      }}
                    >
                      <Play size={16} />
                    </IconButton>
                  )}
                  <IconButton
                    label={t("common.reveal")}
                    onClick={(e) => {
                      e.stopPropagation();
                      void api.reveal({ type: "inbox", id: i.id });
                    }}
                  >
                    <FolderOpen size={16} />
                  </IconButton>
                </li>
              );
            })}
          </ul>
        )}
      </div>

      {selected.size > 0 && (
        <div className="flex items-center gap-3 border-t border-line bg-surface px-4 py-3">
          <span className="mr-auto text-muted">{t("inbox.selected", { count: selected.size })}</span>
          {ignored ? (
            <Button onClick={() => void setHidden(false)}>{t("inbox.unignore")}</Button>
          ) : (
            <>
              <Button onClick={() => void setHidden(true)}>{t("inbox.ignore")}</Button>
              <Button tone="primary" onClick={() => setSorting(true)}>{t("inbox.sort")}</Button>
            </>
          )}
        </div>
      )}

      {sorting && (
        <SortDialog
          items={chosen}
          onClose={() => setSorting(false)}
          onPlanned={() => {
            setSelected(new Set());
            void refreshApp();
          }}
        />
      )}
    </div>
  );
}
