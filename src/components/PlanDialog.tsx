import { useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowRight, Copy, FolderPlus, FolderMinus, OctagonX, PencilLine, Trash, TriangleExclamationPoint } from "lucide-react";
import { api, errorText } from "../api";
import { useStore } from "../store";
import type { Issue, StepView } from "../types";
import { Button, Dialog } from "./ui";

const STEP_ICON = {
  create_dir: FolderPlus,
  move: ArrowRight,
  rename: PencilLine,
  remove_dir: FolderMinus,
  copy: Copy,
  trash: Trash,
} as const;

function IssueList({ issues, tone }: { issues: Issue[]; tone: "warn" | "danger" }) {
  const { t } = useTranslation();
  const Icon = tone === "warn" ? TriangleExclamationPoint : OctagonX;
  return (
    <ul className="flex flex-col gap-2">
      {issues.map((i, idx) => (
        <li
          key={idx}
          className={`rounded-md px-3 py-2 ${tone === "warn" ? "bg-warn-bg" : "bg-danger-bg"}`}
        >
          <div className="flex gap-2">
            <Icon size={16} className={`mt-0.5 shrink-0 ${tone === "warn" ? "text-warn" : "text-danger"}`} />
            <div className="min-w-0">
              <div>{t(`issue.${i.code}`, { detail: i.detail ?? "" })}</div>
              {i.path && <div className="truncate text-cap text-muted" title={i.path}>{i.path}</div>}
              {i.items.length > 0 && (
                <ul className="mt-1 text-cap text-muted">
                  {i.items.map((it) => (
                    <li key={it} className="truncate" title={it}>{it}</li>
                  ))}
                </ul>
              )}
            </div>
          </div>
        </li>
      ))}
    </ul>
  );
}

function StepRow({ step }: { step: StepView }) {
  const { t } = useTranslation();
  const Icon = STEP_ICON[step.kind];
  return (
    <li className="grid grid-cols-[20px_1fr] gap-x-2 py-1.5">
      <Icon size={16} className="mt-0.5 text-muted" aria-label={t(`plan.${step.kind}`)} />
      <div className="min-w-0">
        {step.from ? (
          <>
            <div className="truncate text-muted" title={step.from}>{step.from}</div>
            <div className="truncate" title={step.to}>{step.to}</div>
          </>
        ) : (
          <div className="truncate" title={step.to}>
            <span className="text-muted">{t(`plan.${step.kind}`)} </span>
            {step.to}
          </div>
        )}
      </div>
    </li>
  );
}

const MAX_STEPS_SHOWN = 300;

export function PlanDialog() {
  const { t } = useTranslation();
  const req = useStore((s) => s.planRequest);
  const close = useStore((s) => s.closePlan);
  const notify = useStore((s) => s.notify);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (!req) return null;
  const { plan } = req;
  const blocked = plan.blockers.length > 0;
  const empty = plan.steps.length === 0;

  const cancel = () => {
    if (busy) return;
    void api.discardPlan(plan.id);
    setError(null);
    close();
  };

  const run = async () => {
    setBusy(true);
    setError(null);
    try {
      const r = await api.executePlan(plan.id);
      notify(`${t("plan.done")}. ${t("plan.doneDetail", { moved: r.moved, created: r.created })}`);
      req.onDone?.();
      close();
    } catch (e) {
      const msg = errorText(e);
      setError(msg === "stale" ? t("plan.stale") : t("plan.failed", { message: msg }));
    } finally {
      setBusy(false);
    }
  };

  const shown = plan.steps.slice(0, MAX_STEPS_SHOWN);

  return (
    <Dialog
      title={plan.label || t("plan.title")}
      onClose={cancel}
      width={720}
      footer={
        <>
          <span className="mr-auto text-muted">{t("plan.steps", { count: plan.steps.length })}</span>
          <Button onClick={cancel} disabled={busy}>{t("common.cancel")}</Button>
          <Button tone="primary" onClick={run} disabled={busy || blocked || empty || error !== null}>
            {plan.isUndo ? t("plan.runUndo") : t("plan.run")}
          </Button>
        </>
      }
    >
      {blocked && (
        <section className="mb-4">
          <h3 className="mb-2 font-semibold text-danger">{t("plan.blockers")}</h3>
          <IssueList issues={plan.blockers} tone="danger" />
          <p className="mt-2 text-muted">{t("plan.blockedHint")}</p>
        </section>
      )}
      {plan.warnings.length > 0 && (
        <section className="mb-4">
          <h3 className="mb-2 font-semibold">{t("plan.warnings")}</h3>
          <IssueList issues={plan.warnings} tone="warn" />
        </section>
      )}
      {error && <p className="mb-4 rounded-md bg-danger-bg px-3 py-2 text-danger" role="alert">{error}</p>}
      {empty ? (
        <p className="text-muted">{t("plan.nothing")}</p>
      ) : (
        <ul className="divide-y divide-line">
          {shown.map((s, i) => (
            <StepRow key={i} step={s} />
          ))}
          {plan.steps.length > shown.length && (
            <li className="py-2 text-muted">{t("plan.more", { count: plan.steps.length - shown.length })}</li>
          )}
        </ul>
      )}
    </Dialog>
  );
}
