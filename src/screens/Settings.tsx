import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { FolderOpen, Plus, RefreshCw, X } from "lucide-react";
import { api, errorText, pickFolder } from "../api";
import { useStore } from "../store";
import { Button, IconButton, Label, Segmented, TextInput } from "../components/ui";
import { useDebounced } from "../lib/useEvents";
import type { Config, Lang, LibraryTemplate, TemplatePatch, ThemePref } from "../types";

const TOKENS = ["name", "bpm", "key", "camelot", "year", "date", "authors", "coauthors", "producers"] as const;
type PatternField = "fileName" | "beatDir" | "loopDir" | "starterDir" | "voicetagDir";
const FIELDS: { field: PatternField; label: string }[] = [
  { field: "fileName", label: "settings.fileName" },
  { field: "beatDir", label: "settings.beatFolder" },
  { field: "loopDir", label: "settings.loopFolder" },
  { field: "starterDir", label: "settings.starterFolder" },
  { field: "voicetagDir", label: "settings.voicetagFolder" },
];

function Preview({ pattern }: { pattern: string }) {
  const { t } = useTranslation();
  const debounced = useDebounced(pattern, 200);
  const [out, setOut] = useState("");
  useEffect(() => {
    let alive = true;
    api.renderPreview(debounced).then((r) => alive && setOut(r)).catch(() => alive && setOut(""));
    return () => {
      alive = false;
    };
  }, [debounced]);
  return (
    <div className="mt-1 truncate text-cap text-muted" title={out}>
      {t("settings.example")}: <span className="text-text">{out}</span>
    </div>
  );
}

/** Patterns for file and folder names, with token buttons and a live example. */
function TemplateEditor({ template, onSaved }: { template: LibraryTemplate; onSaved: (c: Config) => void }) {
  const { t } = useTranslation();
  const notify = useStore((s) => s.notify);
  const [values, setValues] = useState<Record<PatternField, string>>({
    fileName: template.fileName,
    beatDir: template.beatDir,
    loopDir: template.loopDir,
    starterDir: template.starterDir,
    voicetagDir: template.voicetagDir,
  });
  const [error, setError] = useState<string | null>(null);
  const focused = useRef<{ field: PatternField; el: HTMLInputElement } | null>(null);

  const insert = (text: string) => {
    const f = focused.current;
    const field = f?.field ?? "fileName";
    const current = values[field];
    const start = f?.el.selectionStart ?? current.length;
    const end = f?.el.selectionEnd ?? current.length;
    const next = current.slice(0, start) + text + current.slice(end);
    setValues({ ...values, [field]: next });
    requestAnimationFrame(() => {
      if (!f) return;
      f.el.focus();
      f.el.setSelectionRange(start + text.length, start + text.length);
    });
  };

  const changed = (Object.keys(values) as PatternField[]).filter((k) => values[k] !== template[k]);
  const save = async () => {
    setError(null);
    const patch: TemplatePatch = {};
    for (const k of changed) patch[k] = values[k];
    try {
      onSaved(await api.updateTemplate(patch));
      notify(t("settings.templateSaved"));
    } catch (e) {
      const code = errorText(e);
      setError(code === "bad_pattern" || code === "structure_locked" ? t(`settings.errors.${code}`) : code);
    }
  };

  return (
    <section className="mb-8">
      <Label hint={t("settings.namesHint")}>{t("settings.names")}</Label>
      <div className="mb-3 flex flex-wrap gap-1.5">
        {TOKENS.map((tok) => (
          <button
            key={tok}
            type="button"
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => insert(`{${tok}}`)}
            className="h-7 rounded-md border border-line px-2 text-cap hover:bg-raised"
          >
            {t(`settings.tokens.${tok}`)}
          </button>
        ))}
        <button
          type="button"
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => insert("[ (prod. {producers})]")}
          className="h-7 rounded-md border border-dashed border-line px-2 text-cap hover:bg-raised"
        >
          [ (prod. …)]
        </button>
      </div>
      <div className="flex flex-col gap-4">
        {FIELDS.map(({ field, label }) => (
          <label key={field} className="block">
            <span className="mb-1 block font-medium">{t(label)}</span>
            <TextInput
              value={values[field]}
              spellCheck={false}
              className="font-mono"
              onFocus={(e) => (focused.current = { field, el: e.currentTarget })}
              onChange={(e) => setValues({ ...values, [field]: e.target.value })}
            />
            <Preview pattern={values[field]} />
          </label>
        ))}
      </div>
      {error && <p className="mt-3 text-danger" role="alert">{error}</p>}
      <Button tone="primary" className="mt-4" disabled={changed.length === 0} onClick={() => void save()}>
        {t("settings.saveTemplate")}
      </Button>
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

        {config.template && <TemplateEditor template={config.template} onSaved={setConfig} />}

        <section className="mb-8">
          <Label hint={t("settings.watchHint")}>{t("onboarding.daws")}</Label>
          <FolderList title={t("settings.flFolders")} folders={fl} onChange={(n) => void watch(n, ab)} />
          <FolderList title={t("settings.abletonFolders")} folders={ab} onChange={(n) => void watch(fl, n)} />
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
