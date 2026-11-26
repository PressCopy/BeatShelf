import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { FileMusic, FileText, Folder, Image } from "lucide-react";
import { api, errorText } from "../api";
import { useStore } from "../store";
import { useDebounced } from "../lib/useEvents";
import { formatBpm, formatSize } from "../lib/format";
import { Button, DawMark, Dialog, KeyChip, Label, Segmented, TextInput } from "./ui";
import { KINDS, SLOTS, voicetagAccepts, type AddTarget, type BeatRow, type DropItem, type ItemKind, type Slot } from "../types";

type Mode = "new" | "beat" | "inbox";

function ItemIcon({ item }: { item: DropItem }) {
  if (item.kind === "fl_project" || item.kind === "fl_zip") return <DawMark daw="fl" />;
  if (item.kind === "ableton_project" || item.kind === "ableton_set") return <DawMark daw="ableton" />;
  if (item.isDir) return <Folder size={16} className="text-muted" />;
  if (item.kind === "audio") return <FileMusic size={16} className="text-muted" />;
  if (item.kind === "image") return <Image size={16} className="text-muted" />;
  return <FileText size={16} className="text-muted" />;
}

const people = (v: string) =>
  v
    .split(",")
    .map((x) => x.trim())
    .filter(Boolean);

/** "Where do these files go": a new item, an existing one, or the Inbox.
 *  Opened by the "Add folder" buttons and by dropping files on the window. */
export function AddFilesDialog() {
  const { t } = useTranslation();
  const req = useStore((s) => s.addFiles);
  const close = useStore((s) => s.closeAddFiles);
  const showPlan = useStore((s) => s.showPlan);
  const [mode, setMode] = useState<Mode>("new");
  // New item
  const [kind, setKind] = useState<ItemKind>("beat");
  const [name, setName] = useState("");
  const [bpm, setBpm] = useState("");
  const [key, setKey] = useState("");
  const [authors, setAuthors] = useState("");
  const [coauthors, setCoauthors] = useState("");
  // Existing item
  const [beat, setBeat] = useState<BeatRow | null>(null);
  const [query, setQuery] = useState("");
  const debounced = useDebounced(query, 150);
  const [found, setFound] = useState<BeatRow[]>([]);

  const [slots, setSlots] = useState<Record<string, Slot>>({});
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!req) return;
    const st = useStore.getState();
    const s = req.suggest ?? req.drop.folderSuggestion;
    setError(null);
    setQuery("");
    setSlots(Object.fromEntries(req.drop.items.map((i) => [String(i.index), i.slot])));
    setMode(req.mode ?? (req.beatId != null ? "beat" : "new"));
    setKind(req.kind ?? (st.libraryKind === "all" ? "beat" : st.libraryKind));
    // Without a suggestion (files dropped on the window) start from the first file's name.
    const first = req.drop.items[0];
    setName(s?.name ?? (first?.isDir ? first.name : (first?.name ?? "").replace(/\.[^.]+$/, "")));
    setBpm(formatBpm(s?.bpm ?? null));
    setKey(s?.key ?? "");
    setAuthors(s?.authors.length ? s.authors.join(", ") : (st.app?.config.producerName ?? ""));
    setCoauthors("");
    setBeat(null);
    if (req.beatId != null) void api.beatDetail(req.beatId).then((d) => setBeat(d.beat));
  }, [req]);

  useEffect(() => {
    if (!req || mode !== "beat") return;
    let alive = true;
    api.beats(debounced).then((r) => alive && setFound(r.slice(0, 50)));
    return () => {
      alive = false;
    };
  }, [debounced, mode, req]);

  if (!req) return null;

  const targetKind = mode === "new" ? kind : mode === "beat" ? beat?.kind : undefined;
  const flat = targetKind === "voicetag";
  const tag = mode === "new" && kind === "voicetag";

  const submit = async () => {
    setError(null);
    const suggestion = req.suggest ?? req.drop.folderSuggestion;
    let target: AddTarget;
    let label: string;
    if (mode === "new") {
      if (!name.trim()) return setError(t("sort.needName"));
      const n = !tag && bpm.trim() ? Number(bpm.replace(",", ".")) : null;
      if (n != null && (!Number.isFinite(n) || n < 40 || n > 300)) return setError(t("library.badBpm"));
      target = {
        type: "new",
        name: name.trim(),
        bpm: n,
        key: tag ? null : key || null,
        kind,
        authors: tag ? [] : people(authors),
        coauthors: tag ? [] : people(coauthors),
      };
      label = t("add.labelNew", { name: name.trim() });
    } else if (mode === "beat") {
      if (!beat) return setError(t("sort.needBeat"));
      target = { type: "beat", beatId: beat.id };
      // An empty item is still unnamed: its plan is labelled by the first file.
      label = beat.folder
        ? t("add.label", { name: beat.name })
        : t("add.labelNew", { name: (req.drop.items[0]?.name ?? beat.name).replace(/\.[^.]+$/, "") });
    } else {
      target = { type: "inbox" };
      label = t("add.labelInbox");
    }
    setBusy(true);
    try {
      const plan = await api.planAddFiles({
        dropId: req.drop.dropId,
        userFolder: mode === "new" && name !== suggestion?.name ? name : suggestion?.userFolder,
        target,
        slots: mode === "inbox" || flat ? {} : slots,
        label,
      });
      close();
      showPlan(plan);
    } catch (e) {
      const m = errorText(e);
      setError(m === "stale" ? t("plan.stale") : m);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      title={t("add.title")}
      onClose={close}
      footer={
        <>
          <span className="mr-auto text-cap text-muted">{t("add.copyHint")}</span>
          <Button onClick={close}>{t("common.cancel")}</Button>
          <Button tone="primary" onClick={() => void submit()} disabled={busy}>{t("sort.makePlan")}</Button>
        </>
      }
    >
      <div className="mb-4">
        <Segmented<Mode>
          value={mode}
          onChange={setMode}
          options={[
            { value: "new", label: t("add.toNew") },
            { value: "beat", label: t("add.toBeat") },
            { value: "inbox", label: t("add.toInbox") },
          ]}
        />
      </div>

      {mode === "new" && (
        <>
          <div className="mb-3">
            <Segmented<ItemKind> value={kind} onChange={setKind} options={KINDS.map((k) => ({ value: k, label: t(`item.${k}`) }))} />
          </div>
          <div className={`mb-3 grid gap-3 ${tag ? "grid-cols-1" : "grid-cols-[1fr_96px_120px]"}`}>
            <label>
              <Label>{t("sort.name")}</Label>
              <TextInput value={name} onChange={(e) => setName(e.target.value)} />
            </label>
            {!tag && (
              <>
                <label>
                  <Label>{t("sort.bpm")}</Label>
                  <TextInput className="num" inputMode="decimal" value={bpm} onChange={(e) => setBpm(e.target.value)} />
                </label>
                <label>
                  <Label>{t("sort.key")}</Label>
                  <TextInput value={key} placeholder="Am" onChange={(e) => setKey(e.target.value)} />
                </label>
              </>
            )}
          </div>
          {!tag && (
            <div className="mb-5 grid grid-cols-2 gap-3">
              <label>
                <Label>{t("sort.authors")}</Label>
                <TextInput value={authors} placeholder={t("sort.peopleHint")} onChange={(e) => setAuthors(e.target.value)} />
              </label>
              <label>
                <Label>{t("sort.coauthors")}</Label>
                <TextInput value={coauthors} placeholder={t("sort.peopleHint")} onChange={(e) => setCoauthors(e.target.value)} />
              </label>
            </div>
          )}
        </>
      )}

      {mode === "beat" &&
        (beat ? (
          <div className="mb-4 flex items-center gap-3 rounded-lg bg-raised px-3 py-2">
            <span className="flex-1 truncate font-medium">{beat.name}</span>
            <span className="text-cap text-muted">{t(`item.${beat.kind}`)}</span>
            <span className="num text-muted">{formatBpm(beat.bpm)}</span>
            <KeyChip keyName={beat.key} camelot={beat.camelot} />
            <Button onClick={() => setBeat(null)}>{t("add.change")}</Button>
          </div>
        ) : (
          <div className="mb-4">
            <TextInput placeholder={t("sort.pickBeat")} value={query} onChange={(e) => setQuery(e.target.value)} />
            <ul className="mt-2 max-h-40 overflow-auto rounded-lg border border-line" role="listbox">
              {found.map((b) => (
                <li key={b.id}>
                  <button
                    type="button"
                    role="option"
                    aria-selected={false}
                    onClick={() => setBeat(b)}
                    className="flex w-full items-center gap-3 px-3 py-1.5 text-left hover:bg-[var(--row-hover)]"
                  >
                    <span className="flex-1 truncate">{b.name}</span>
                    <span className="text-cap text-muted">{t(`item.${b.kind}`)}</span>
                    <span className="num text-muted">{formatBpm(b.bpm)}</span>
                  </button>
                </li>
              ))}
            </ul>
          </div>
        ))}

      {mode === "beat" && beat && !beat.folder && <p className="mb-3 text-muted">{t("add.draftHint")}</p>}

      {mode === "inbox" ? (
        <p className="mb-3 text-muted">{t("add.inboxHint")}</p>
      ) : flat ? (
        <p className="mb-2 text-muted">{t("sort.voicetagHint")}</p>
      ) : (
        <Label>{t("sort.whereEach")}</Label>
      )}

      <ul className="flex max-h-56 flex-col gap-1 overflow-auto pr-1">
        {req.drop.items.map((i) => (
          <li key={i.index} className="flex min-h-7 items-center gap-3">
            <span className="flex w-7 shrink-0 justify-center">
              <ItemIcon item={i} />
            </span>
            <span className="min-w-0 flex-1 truncate">{i.name}</span>
            {i.size != null && <span className="num shrink-0 text-cap text-muted">{formatSize(i.size)}</span>}
            {flat && !voicetagAccepts(i.kind, i.name) && (
              <span className="shrink-0 text-cap text-danger">{t("sort.notAllowed")}</span>
            )}
            {mode !== "inbox" && !flat && (
              <select
                value={slots[String(i.index)]}
                onChange={(e) => setSlots({ ...slots, [String(i.index)]: e.target.value as Slot })}
                aria-label={i.name}
                className="h-7 shrink-0 rounded-lg border border-line bg-surface px-1 outline-none focus:border-muted"
              >
                {SLOTS.map((s) => (
                  <option key={s} value={s}>{t(`slot.${s}`)}</option>
                ))}
              </select>
            )}
          </li>
        ))}
      </ul>
      {error && <p className="mt-4 text-danger" role="alert">{error}</p>}
    </Dialog>
  );
}
