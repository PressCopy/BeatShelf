import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, errorText } from "../api";
import { formatBpm } from "../lib/format";
import { useStore } from "../store";
import type { BeatPatch, BeatRow } from "../types";
import { Button, TextInput } from "./ui";

export type MainField = "name" | "bpm" | "key";
export type TextField = MainField | "genre" | "mood" | "typeBeat";

export function toPatch(field: TextField, raw: string): BeatPatch | { error: "bpm" } {
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
      return { key: raw || null };
    default:
      return { [field]: v || null } as BeatPatch;
  }
}

/** The drawer saves on blur or Enter. A context-menu dialog saves explicitly. */
export function MetadataField({ beat, field, onClose }: { beat: BeatRow; field: MainField; onClose?: () => void }) {
  const { t } = useTranslation();
  const notify = useStore((s) => s.notify);
  const initial = field === "bpm" ? formatBpm(beat.bpm) : beat[field] ?? "";
  const [value, setValue] = useState(initial);
  const [busy, setBusy] = useState(false);
  useEffect(() => setValue(initial), [beat.id, field, initial]);
  const save = async () => {
    if (busy) return;
    if (value === initial) { onClose?.(); return; }
    const patch = toPatch(field, value);
    if ("error" in patch || !Object.keys(patch).length) {
      notify(t("error" in patch ? "library.badBpm" : "sort.needName"), "error");
      if (!onClose) setValue(initial);
      return;
    }
    setBusy(true);
    try {
      await api.updateBeat(beat.id, patch);
      onClose?.();
    } catch (e) {
      notify(errorText(e), "error");
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="min-w-0">
      <label>
        <span className="mb-1 block text-cap text-muted">{t(`library.col.${field}`)}</span>
        <TextInput value={value} inputMode={field === "bpm" ? "decimal" : undefined}
          onChange={(e) => setValue(e.target.value)}
          onBlur={onClose ? undefined : () => void save()}
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === "Escape") { setValue(initial); onClose?.(); }
            if (e.key === "Enter") {
              e.preventDefault();
              if (onClose) void save();
              else e.currentTarget.blur();
            }
          }}
        />
      </label>
      {onClose && <div className="mt-4 flex justify-end gap-2">
        <Button onClick={onClose} disabled={busy}>{t("common.cancel")}</Button>
        <Button tone="primary" onClick={() => void save()} disabled={busy}>{t("common.save")}</Button>
      </div>}
    </div>
  );
}
