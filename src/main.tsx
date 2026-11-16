import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./i18n";
import "./styles.css";
import App from "./App";

// Focus rings show only while the keyboard is driving (see styles.css).
const html = document.documentElement;
window.addEventListener("keydown", (e) => e.key === "Tab" && (html.dataset.nav = "keyboard"), true);
window.addEventListener("pointerdown", () => delete html.dataset.nav, true);

// The webview's own right-click menu (Reload, Print…) has no place in the
// app; text fields keep theirs for copy and paste.
window.addEventListener("contextmenu", (e) => {
  const el = e.target as HTMLElement | null;
  if (!el?.closest("input, textarea")) e.preventDefault();
});

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
