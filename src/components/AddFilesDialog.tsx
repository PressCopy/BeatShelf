import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { FileMusic, FileText, Folder, Image } from "lucide-react";
import { api, errorText } from "../api";
import { useStore } from "../store";
import { useDebounced } from "../lib/useEvents";
import { formatBpm, formatSize } from "../lib/format";
import { Button, Dialog, KeyChip, Segmented, TextInput } from "./ui";
import { SLOTS, voicetagAccepts, type BeatRow, type DropItem, type Slot } from "../types";

function ItemIcon({ item }: { item: DropItem }) {
  if (item.isDir) return <Folder size={16} className="text-muted" />;
  if (item.kind === "audio") return <FileMusic size={16} className="text-muted" />;
  if (item.kind === "image") return <Image size={16} className="text-muted" />;
  return <FileText size={16} className="text-muted" />;
}

/** "Where do these files go": a beat (with a subfolder per file) or the Inbox. */
export function AddFilesDialog() {
  const { t } = useTranslation();
  const req = useStore((s) => s.addFiles);
  const close = useStore((s) => s.closeAddFiles);
  const showPlan = useStore((s) => s.showPlan);
  const [mode, setMode] = useState<"beat" | "inbox">("beat");
  const [beat, setBeat] = useState<BeatRow | null>(null);
  const [query, setQuery] = useState("");
  const debounced = useDebounced(query, 150);
  const [found, setFound] = useState<BeatRow[]>([]);
  const [slots, setSlots] = useState<Record<string, Slot>>({});
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!req) return;
    setError(null);
    setQuery("");
    setSlots(Object.fromEntries(req.drop.items.map((i) => [String(i.index), i.slot])));
    setMode(req.beatId != null || useStore.getState().screen !== "inbox" ? "beat" : "inbox");
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

  const submit = async () => {
    setError(null);
    if (mode === "beat" && !beat) return setError(t("sort.needBeat"));
    setBusy(true);
    try {
      const plan = await api.planAddFiles({
        dropId: req.drop.dropId,
        target: mode === "beat" && beat ? { type: "beat", beatId: beat.id } : { type: "inbox" },
        slots: mode === "beat" ? slots : {},
        label: mode === "beat" && beat ? t("add.label", { name: beat.name }) : t("add.labelInbox"),
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

  const flat = mode === "beat" && beat?.kind === "voicetag";

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
        <Segmented
          value={mode}
          onChange={setMode}
          options={[
            { value: "beat", label: t("add.toBeat") },
            { value: "inbox", label: t("add.toInbox") },
          ]}
        />
      </div>

      {mode === "beat" ? (
        <>
          {beat ? (
            <div className="mb-4 flex items-center gap-3 rounded-md bg-raised px-3 py-2">
              <span className="flex-1 truncate font-medium">{beat.name}</span>
              <span className="num text-muted">{formatBpm(beat.bpm)}</span>
              <KeyChip keyName={beat.key} camelot={beat.camelot} />
              <Button onClick={() => setBeat(null)}>{t("add.change")}</Button>
            </div>
          ) : (
            <div className="mb-4">
              <TextInput placeholder={t("sort.pickBeat")} value={query} onChange={(e) => setQuery(e.target.value)} />
              <ul className="mt-2 max-h-40 overflow-auto rounded-md border border-line" role="listbox">
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
          )}
          {flat && <p className="mb-2 text-muted">{t("sort.voicetagHint")}</p>}
          <ul className="flex flex-col gap-1">
            {req.drop.items.map((i) => (
              <li key={i.index} className="flex items-center gap-3">
                <ItemIcon item={i} />
                <span className="min-w-0 flex-1 truncate">{i.name}</span>
                {i.size != null && <span className="num text-cap text-muted">{formatSize(i.size)}</span>}
                {flat && !voicetagAccepts(i.kind, i.name) && (
                  <span className="shrink-0 text-cap text-danger">{t("sort.notAllowed")}</span>
                )}
                {!flat && <select
                  value={slots[String(i.index)]}
                  onChange={(e) => setSlots({ ...slots, [String(i.index)]: e.target.value as Slot })}
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
        </>
      ) : (
        <>
          <p className="mb-3 text-muted">{t("add.inboxHint")}</p>
          <ul className="flex flex-col gap-1">
            {req.drop.items.map((i) => (
              <li key={i.index} className="flex items-center gap-3">
                <ItemIcon item={i} />
                <span className="min-w-0 flex-1 truncate">{i.name}</span>
              </li>
            ))}
          </ul>
        </>
      )}
      {error && <p className="mt-4 text-danger" role="alert">{error}</p>}
    </Dialog>
  );
}
