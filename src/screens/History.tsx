import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Undo2 } from "lucide-react";
import { api, errorText } from "../api";
import { useStore } from "../store";
import { useReloadOn } from "../lib/useEvents";
import { formatDate } from "../lib/format";
import { Button, Empty } from "../components/ui";
import type { BatchRow } from "../types";

function statusOf(b: BatchRow): keyof typeof TONE {
  return b.undoneBy != null ? "undone" : b.status;
}

const TONE = {
  done: "text-muted",
  undone: "text-faint",
  rolled_back: "text-warn",
  running: "text-warn",
  failed: "text-danger",
} as const;

export function History() {
  const { t, i18n } = useTranslation();
  const showPlan = useStore((s) => s.showPlan);
  const notify = useStore((s) => s.notify);
  const [rows, setRows] = useState<BatchRow[] | null>(null);
  const reload = useReloadOn(["library-changed", "inbox-changed"]);

  useEffect(() => {
    let alive = true;
    api.history().then((r) => alive && setRows(r));
    return () => {
      alive = false;
    };
  }, [reload]);

  const undo = async (b: BatchRow) => {
    try {
      const plan = await api.planUndo(b.id, t("history.undoLabel"));
      if (plan) showPlan(plan);
    } catch (e) {
      notify(errorText(e), "error");
    }
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="border-b border-line px-4 py-3">
        <h1 className="text-title font-semibold">{t("history.title")}</h1>
        <p className="max-w-[72ch] text-muted">{t("history.hint")}</p>
      </div>
      <div className="min-h-0 flex-1 overflow-auto">
        {rows && rows.length === 0 ? (
          <Empty>{t("history.empty")}</Empty>
        ) : (
          <ul>
            {(rows ?? []).map((b) => {
              const st = statusOf(b);
              return (
                <li key={b.id} className="flex items-center gap-4 border-b border-line/60 px-4 py-2.5">
                  <div className="min-w-0 flex-1">
                    <div className={`truncate ${st === "undone" ? "text-muted line-through decoration-faint" : "font-medium"}`}>{b.label}</div>
                    <div className="num text-cap text-muted">
                      {formatDate(b.createdAt, i18n.language)}, {t("history.ops", { count: b.opCount })}
                    </div>
                  </div>
                  <span className={`text-cap ${TONE[st]}`}>{t(`history.st.${st}`)}</span>
                  <Button onClick={() => void undo(b)} disabled={!b.canUndo}>
                    <Undo2 size={14} />
                    {t("history.undo")}
                  </Button>
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </div>
  );
}
