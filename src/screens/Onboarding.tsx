import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Folder, FolderOpen, Plus, X } from "lucide-react";
import { api, errorText, pickFolder } from "../api";
import { useStore } from "../store";
import { baseName } from "../lib/format";
import { Button, IconButton, Label, Segmented } from "../components/ui";
import { Logo } from "../components/Logo";
import type { Lang, LibraryTemplate, TreeLine } from "../types";

function Paths({ title, paths, onChange }: { title: string; paths: string[]; onChange: (p: string[]) => void }) {
  const { t } = useTranslation();
  return (
    <div className="mb-3">
      <div className="mb-1 text-muted">{title}</div>
      {paths.map((p) => (
        <div key={p} className="mb-1 flex items-center gap-2 rounded-md bg-raised px-2.5 py-1">
          <code className="min-w-0 flex-1 truncate" title={p}>{p}</code>
          <IconButton label={t("common.remove")} onClick={() => onChange(paths.filter((x) => x !== p))}>
            <X size={14} />
          </IconButton>
        </div>
      ))}
      <Button
        onClick={async () => {
          const p = await pickFolder(title);
          if (p && !paths.includes(p)) onChange([...paths, p]);
        }}
      >
        <Plus size={14} />
        {t("common.add")}
      </Button>
    </div>
  );
}

export function Onboarding() {
  const { t, i18n } = useTranslation();
  const showPlan = useStore((s) => s.showPlan);
  const refreshApp = useStore((s) => s.refreshApp);
  const notify = useStore((s) => s.notify);
  const [step, setStep] = useState(1);
  const [lang, setLang] = useState<Lang>((i18n.language as Lang) || "ru");
  const [root, setRoot] = useState<string | null>(null);
  const [fl, setFl] = useState<string[]>([]);
  const [ab, setAb] = useState<string[]>([]);
  const [templates, setTemplates] = useState<LibraryTemplate[]>([]);
  const [templateId, setTemplateId] = useState("standard");
  const [tree, setTree] = useState<TreeLine[]>([]);

  useEffect(() => {
    void api.templates().then(setTemplates);
  }, []);
  useEffect(() => {
    void i18n.changeLanguage(lang);
  }, [lang, i18n]);
  useEffect(() => {
    if (step === 3 && root) void api.previewTemplate(templateId, baseName(root)).then(setTree);
  }, [step, root, templateId]);

  const create = async () => {
    if (!root) return;
    try {
      const plan = await api.planCreateLibrary({
        root,
        templateId,
        flFolders: fl,
        abletonFolders: ab,
        lang,
        label: t("onboarding.createLabel"),
      });
      showPlan(plan, () => void refreshApp());
    } catch (e) {
      notify(errorText(e), "error");
    }
  };

  const titles = [t("onboarding.s1title"), t("onboarding.s2title"), t("onboarding.s3title")];
  const texts = [t("onboarding.s1text"), t("onboarding.s2text"), t("onboarding.s3text")];

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center justify-between px-8 py-5">
        <Logo />
        <Segmented
          value={lang}
          onChange={setLang}
          options={[
            { value: "ru", label: "RU" },
            { value: "en", label: "EN" },
          ]}
        />
      </header>

      <main className="mx-auto flex w-full max-w-[640px] min-h-0 flex-1 flex-col px-8 pb-8">
        <ol className="mb-8 flex gap-2" aria-label={t("onboarding.step", { n: step })}>
          {[1, 2, 3].map((n) => (
            <li key={n} className={`h-1 flex-1 rounded-full ${n <= step ? "bg-text" : "bg-line"}`} aria-current={n === step ? "step" : undefined} />
          ))}
        </ol>
        <p className="mb-1 text-muted">{t("onboarding.step", { n: step })}</p>
        <h1 className="mb-2 text-hero font-semibold tracking-tight">{titles[step - 1]}</h1>
        <p className="mb-8 max-w-[56ch] text-sub text-muted">{texts[step - 1]}</p>

        <div className="min-h-0 flex-1 overflow-auto">
          {step === 1 && (
            <>
              <Label>{t("onboarding.root")}</Label>
              <div className="mb-8 flex items-center gap-2">
                <div className="flex h-10 min-w-0 flex-1 items-center gap-2 rounded-md border border-line bg-surface px-3">
                  <Folder size={16} className="shrink-0 text-muted" />
                  <span className={`truncate ${root ? "" : "text-faint"}`}>{root ?? "…"}</span>
                </div>
                <Button
                  tone={root ? "quiet" : "primary"}
                  className="h-10"
                  onClick={async () => setRoot((await pickFolder(t("onboarding.root"))) ?? root)}
                >
                  <FolderOpen size={16} />
                  {t("common.choose")}
                </Button>
              </div>
              <Label hint={t("onboarding.dawsHint")}>{t("onboarding.daws")}</Label>
              <Paths title="FL Studio" paths={fl} onChange={setFl} />
              <Paths title="Ableton Live" paths={ab} onChange={setAb} />
            </>
          )}

          {step === 2 && (
            <div role="radiogroup" className="flex flex-col gap-3">
              {templates.map((tpl) => {
                const on = tpl.id === templateId;
                return (
                  <button
                    key={tpl.id}
                    type="button"
                    role="radio"
                    aria-checked={on}
                    onClick={() => setTemplateId(tpl.id)}
                    className={`rounded-lg border px-4 py-3 text-left transition-colors ${on ? "border-text bg-raised" : "border-line hover:border-muted"}`}
                  >
                    <div className="mb-2 text-sub font-semibold">{tpl.name}</div>
                    <div className="text-muted">{[tpl.inbox, tpl.staticDirs[0]?.split("/")[0], tpl.archive].filter(Boolean).join(" · ")}</div>
                  </button>
                );
              })}
            </div>
          )}

          {step === 3 && (
            <ul className="rounded-lg border border-line bg-surface px-4 py-3 font-mono text-body" aria-label={t("onboarding.s3title")}>
              {tree.map((l, i) => {
                const example = l.role === "example" || l.role === "beat" || l.role === "sub";
                return (
                  <li key={i} className={`flex items-center gap-2 ${example ? "text-muted" : ""}`} style={{ paddingLeft: l.depth * 16 }}>
                    <Folder size={14} className="shrink-0" />
                    <span className="truncate">{l.name}</span>
                    {l.role === "beat" && <span className="text-cap text-faint">({t("onboarding.example")})</span>}
                  </li>
                );
              })}
            </ul>
          )}
        </div>

        <footer className="mt-6 flex justify-between">
          <Button onClick={() => setStep(step - 1)} disabled={step === 1}>{t("common.back")}</Button>
          {step < 3 ? (
            <Button tone="primary" onClick={() => setStep(step + 1)} disabled={step === 1 && !root}>{t("common.next")}</Button>
          ) : (
            <Button tone="primary" onClick={() => void create()}>{t("onboarding.create")}</Button>
          )}
        </footer>
      </main>
    </div>
  );
}
