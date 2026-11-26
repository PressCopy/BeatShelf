import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { FolderOpen, Plus, RefreshCw, X } from "lucide-react";
import { api, errorText, pickFolder } from "../api";
import { useStore } from "../store";
import { Button, IconButton, Label, Segmented, TextInput } from "../components/ui";
import type { Config, Lang, ThemePref } from "../types";

function Numbering({ config, save }: { config: Config; save: (p: Parameters<typeof api.setPrefs>[0]) => Promise<void> }) {
  const { t } = useTranslation();
  const [number, setNumber] = useState(String(config.numberFrom));
  useEffect(() => setNumber(String(config.numberFrom)), [config.numberFrom]);
  return (
    <section className="mb-8 flex flex-col items-start gap-3">
      <label className="flex items-center gap-2">
        <input type="checkbox" role="switch" checked={config.numberBeats}
          className="peer sr-only" onChange={(e) => void save({ numberBeats: e.target.checked })} />
        <span aria-hidden="true" className="flex h-5 w-9 items-center rounded-full bg-raised px-0.5 ring-1 ring-line peer-checked:bg-primary-bg peer-focus-visible:outline-2">
          <span className={`h-4 w-4 rounded-full transition-transform ${config.numberBeats ? "translate-x-4 bg-primary-fg" : "bg-muted"}`} />
        </span>
        {t("settings.numberBeats")}
      </label>
      <label className="flex items-center gap-3 whitespace-nowrap">{t("settings.numberFrom")}
        <span className="w-24">
        <TextInput type="number" min={0} step={1} className="num" value={number}
          onChange={(e) => setNumber(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter") e.currentTarget.blur(); }}
          onBlur={() => {
            const n = Number(number);
            if (!number || !Number.isSafeInteger(n) || n < 0) setNumber(String(config.numberFrom));
            else if (n !== config.numberFrom) void save({ numberFrom: n });
          }}
        />
        </span>
      </label>
    </section>
  );
}

function ProducerName({ value, onSaved }: { value: string | null; onSaved: (c: Config) => void }) {
  const { t } = useTranslation();
  const [v, setV] = useState(value ?? "");
  useEffect(() => setV(value ?? ""), [value]);
  const save = async () => {
    if ((value ?? "") === v.trim()) return;
    onSaved(await api.setPrefs({ producerName: v.trim() || null }));
  };
  return (
    <label className="mb-8 block max-w-[360px]">
      <Label hint={t("settings.producerHint")}>{t("settings.producerName")}</Label>
      <TextInput
        value={v}
        placeholder="maks"
        onChange={(e) => setV(e.target.value)}
        onBlur={() => void save()}
        onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
      />
    </label>
  );
}

function FolderList({
  title,
  folders,
  onChange,
}: {
  title: string;
  folders: string[];
  onChange: (next: string[]) => void;
}) {
  const { t } = useTranslation();
  const add = async () => {
    const p = await pickFolder(title);
    if (p && !folders.includes(p)) onChange([...folders, p]);
  };
  return (
    <div className="mb-4">
      <div className="mb-1 font-medium">{title}</div>
      <ul className="mb-2 flex flex-col gap-1">
        {folders.map((f) => (
          <li key={f} className="flex items-center gap-2 rounded-md bg-raised px-2.5 py-1">
            <code className="min-w-0 flex-1 truncate" title={f}>{f}</code>
            <IconButton label={t("common.remove")} onClick={() => onChange(folders.filter((x) => x !== f))}>
              <X size={14} />
            </IconButton>
          </li>
        ))}
      </ul>
      <Button onClick={() => void add()}>
        <Plus size={14} />
        {t("common.add")}
      </Button>
    </div>
  );
}

export function Settings() {
  const { t } = useTranslation();
  const config = useStore((s) => s.app?.config);
  const setConfig = useStore((s) => s.setConfig);
  const notify = useStore((s) => s.notify);
  const [scanningDaw, setScanningDaw] = useState(false);
  const scanDaw = async () => {
    setScanningDaw(true);
    try {
      await api.scanDaw();
      notify(t("settings.dawDone"));
    } catch (e) {
      notify(t("settings.dawFailed", { message: errorText(e) }), "error");
    } finally {
      setScanningDaw(false);
    }
  };
  if (!config) return null;

  const prefs = async (patch: Parameters<typeof api.setPrefs>[0]) => {
    try {
      setConfig(await api.setPrefs(patch));
    } catch (e) {
      notify(errorText(e), "error");
    }
  };
  const fl = config.flFolders.map((f) => f.path);
  const ab = config.abletonFolders.map((f) => f.path);
  const watch = async (nextFl: string[], nextAb: string[]) => {
    try {
      setConfig(await api.setWatchFolders(nextFl, nextAb));
    } catch (e) {
      notify(errorText(e), "error");
    }
  };

  return (
    <div className="min-h-0 flex-1 overflow-auto">
      <div className="max-w-[720px] px-6 py-5">
        <h1 className="mb-6 text-title font-semibold">{t("settings.title")}</h1>

        <section className="mb-8 grid grid-cols-[160px_1fr] items-center gap-x-6 gap-y-4">
          <div className="font-medium">{t("settings.language")}</div>
          <Segmented<Lang>
            value={config.lang}
            onChange={(lang) => void prefs({ lang })}
            options={[
              { value: "ru", label: "Русский" },
              { value: "en", label: "English" },
            ]}
          />
          <div className="font-medium">{t("settings.theme")}</div>
          <Segmented<ThemePref>
            value={config.theme}
            onChange={(theme) => void prefs({ theme })}
            options={(["dark", "light", "system"] as const).map((v) => ({ value: v, label: t(`settings.themes.${v}`) }))}
          />
        </section>

        <ProducerName value={config.producerName} onSaved={setConfig} />

        <label className="mb-8 flex items-start gap-2">
          <input type="checkbox" className="mt-1" checked={config.showBackups} onChange={(e) => void prefs({ showBackups: e.target.checked })} />
          <span>
            <span className="block">{t("settings.backups")}</span>
            <span className="block text-cap text-muted">{t("settings.backupsHint")}</span>
          </span>
        </label>

        <section className="mb-8">
          <h2 className="mb-3 text-sub font-semibold">{t("settings.library")}</h2>
          <div className="mb-3 flex items-center gap-2">
            <code className="min-w-0 flex-1 truncate rounded-md bg-raised px-2.5 py-1.5" title={config.root ?? ""}>{config.root}</code>
            <IconButton label={t("common.reveal")} onClick={() => void api.reveal({ type: "root" })}>
              <FolderOpen size={16} />
            </IconButton>
          </div>
          <Button onClick={() => void api.rescan()}>
            <RefreshCw size={14} />
            {t("settings.rescan")}
          </Button>
        </section>

        <Numbering config={config} save={prefs} />

        <section className="mb-8">
          <Label hint={t("settings.watchHint")}>{t("onboarding.daws")}</Label>
          <FolderList title={t("settings.flFolders")} folders={fl} onChange={(n) => void watch(n, ab)} />
          <FolderList title={t("settings.abletonFolders")} folders={ab} onChange={(n) => void watch(fl, n)} />
          <Button onClick={() => void scanDaw()} disabled={scanningDaw || !(fl.length || ab.length)}>
            <RefreshCw size={14} className={scanningDaw ? "animate-spin" : ""} />
            {t("settings.scanDaw")}
          </Button>
        </section>

        <section>
          <h2 className="mb-3 text-sub font-semibold">{t("settings.shortcuts")}</h2>
          <ul className="flex flex-col gap-1 text-muted">
            {(["play", "seek", "jkl", "move", "open", "search", "undo"] as const).map((k) => (
              <li key={k}>{t(`settings.keys.${k}`)}</li>
            ))}
          </ul>
        </section>
      </div>
    </div>
  );
}
