import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { api, errorText, onEvent } from "./api";
import { useStore } from "./store";
import { isTyping } from "./lib/format";
import { Sidebar } from "./components/Sidebar";
import { Player } from "./components/Player";
import { PlanDialog } from "./components/PlanDialog";
import { AddFilesDialog } from "./components/AddFilesDialog";
import { DropLayer } from "./components/DropLayer";
import { Library } from "./screens/Library";
import { BeatDrawer } from "./screens/BeatDrawer";
import { Inbox } from "./screens/Inbox";
import { History } from "./screens/History";
import { Settings } from "./screens/Settings";
import { Onboarding } from "./screens/Onboarding";
import type { Screen } from "./store";

const SCREENS: Screen[] = ["library", "inbox", "history", "settings"];

function Toast() {
  const toast = useStore((s) => s.toast);
  if (!toast) return null;
  return (
    <div
      role={toast.tone === "error" ? "alert" : "status"}
      className={`pop-in fixed bottom-20 left-1/2 z-50 max-w-[60ch] -translate-x-1/2 rounded-lg border px-4 py-2 shadow-xl ${
        toast.tone === "error" ? "border-danger bg-surface text-danger" : "border-line bg-surface"
      }`}
    >
      {toast.text}
    </div>
  );
}

export default function App() {
  const { t } = useTranslation();
  const app = useStore((s) => s.app);
  const screen = useStore((s) => s.screen);
  const refreshApp = useStore((s) => s.refreshApp);

  useEffect(() => {
    void refreshApp();
    const offs = [
      onEvent("inbox-changed", () => void refreshApp()),
      onEvent("scan-progress", () => useStore.getState().setScanning(true)),
      onEvent("library-changed", () => useStore.getState().setScanning(false)),
    ];
    return () => offs.forEach((p) => p.then((off) => off()));
  }, [refreshApp]);

  // App-wide shortcuts.
  useEffect(() => {
    const onKey = async (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey;
      if (!mod || !useStore.getState().app?.onboarded) return;
      const n = Number(e.key);
      if (n >= 1 && n <= SCREENS.length) {
        e.preventDefault();
        useStore.getState().go(SCREENS[n - 1]);
      } else if (e.key.toLowerCase() === "f") {
        e.preventDefault();
        useStore.getState().go("library");
        requestAnimationFrame(() => window.dispatchEvent(new Event("beatshelf:focus-search")));
      } else if (e.key.toLowerCase() === "z" && !isTyping(e) && !document.querySelector('[role="dialog"]')) {
        e.preventDefault();
        try {
          const plan = await api.planUndo(null, t("history.undoLabel"));
          if (plan) useStore.getState().showPlan(plan);
        } catch (err) {
          useStore.getState().notify(errorText(err), "error");
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [t]);

  if (!app) return <div className="flex h-full items-center justify-center text-muted">{t("common.loading")}</div>;

  if (!app.onboarded) {
    return (
      <>
        <Onboarding />
        <PlanDialog />
        <Toast />
      </>
    );
  }

  return (
    <div className="flex h-full flex-col">
      <div className="flex min-h-0 flex-1">
        <Sidebar />
        <main className="relative flex min-w-0 flex-1 flex-col bg-bg">
          {screen === "library" && <Library />}
          {screen === "inbox" && <Inbox />}
          {screen === "history" && <History />}
          {screen === "settings" && <Settings />}
          <BeatDrawer />
        </main>
      </div>
      <Player />
      <AddFilesDialog />
      <PlanDialog />
      <DropLayer />
      <Toast />
    </div>
  );
}
