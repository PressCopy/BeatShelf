import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import WaveSurfer from "wavesurfer.js";
import { Pause, Play } from "lucide-react";
import { api, errorText, fileUrl } from "../api";
import { useStore, type PlayTarget } from "../store";
import { beatColor } from "../lib/camelot";
import { formatBpm, formatTime, isTyping } from "../lib/format";
import { KeyChip } from "./ui";

/** Canvas can't read CSS variables, so resolve a colour through the DOM. */
function resolveColor(css: string): string {
  const probe = document.createElement("span");
  probe.style.color = css;
  probe.style.display = "none";
  document.body.appendChild(probe);
  const out = getComputedStyle(probe).color;
  probe.remove();
  return out;
}

function waveColors(camelot: string | null, color?: string | null) {
  const accent = resolveColor(beatColor(color, camelot) ?? "var(--accent-hi)");
  const rest = resolveColor("var(--wave)");
  return { progress: accent, wave: rest, cursor: accent };
}

export function Player() {
  const { t } = useTranslation();
  const target = useStore((s) => s.play);
  const container = useRef<HTMLDivElement>(null);
  const ws = useRef<WaveSurfer | null>(null);
  const [playing, setPlaying] = useState(false);
  const [time, setTime] = useState(0);
  const [duration, setDuration] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const autoplay = useRef(false);
  const loadedKey = useRef<string | null>(null);

  useEffect(() => {
    if (!container.current) return;
    const w = WaveSurfer.create({
      container: container.current,
      height: 40,
      barWidth: 2,
      barGap: 1,
      barRadius: 1,
      cursorWidth: 1,
      normalize: true,
      dragToSeek: true,
      ...waveColors(null),
    });
    // Mirror the transport into the store so list rows can show it.
    const mirror = (on: boolean) => {
      setPlaying(on);
      useStore.setState({ playing: on });
    };
    w.on("play", () => mirror(true));
    w.on("pause", () => mirror(false));
    w.on("finish", () => mirror(false));
    let lastTick = 0;
    w.on("timeupdate", (s) => {
      setTime(s);
      const now = performance.now();
      if (now - lastTick < 100) return;
      lastTick = now;
      const d = w.getDuration();
      useStore.setState({ playProgress: d > 0 ? s / d : 0 });
    });
    w.on("ready", (d) => {
      setDuration(d);
      setLoading(false);
      const p = useStore.getState().play;
      loadedKey.current = p ? targetKey(p) : null;
      if (autoplay.current) {
        autoplay.current = false;
        void w.play();
      }
    });
    w.on("error", (e) => {
      setLoading(false);
      setError(errorText(e));
    });
    ws.current = w;

    // Follow theme switches.
    const obs = new MutationObserver(() => {
      const p = useStore.getState().play;
      const c = waveColors(p?.camelot ?? null, p?.color);
      w.setOptions({ waveColor: c.wave, progressColor: c.progress, cursorColor: c.cursor });
    });
    obs.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    return () => {
      obs.disconnect();
      w.destroy();
      ws.current = null;
    };
  }, []);

  // Load whatever was selected.
  useEffect(() => {
    const w = ws.current;
    if (!w || !target) return;
    let cancelled = false;
    setError(null);
    setTime(0);
    useStore.setState({ playProgress: 0 });
    setLoading(true);
    loadedKey.current = null;
    const c = waveColors(target.camelot, target.color);
    w.setOptions({ waveColor: c.wave, progressColor: c.progress, cursorColor: c.cursor });
    const fetchMedia = target.type === "asset" ? api.assetMedia(target.id) : api.inboxMedia(target.id);
    fetchMedia
      .then((m) => {
        if (cancelled) return;
        const dur = m.durationMs != null ? m.durationMs / 1000 : undefined;
        if (dur) setDuration(dur);
        return w.load(fileUrl(m.path), m.peaks.length ? [m.peaks] : undefined, dur);
      })
      .catch((e) => {
        if (!cancelled) {
          setLoading(false);
          setError(errorText(e));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [target]);

  // Keyboard transport. Ignored while typing or when a dialog is open.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const w = ws.current;
      if (!w || isTyping(e) || e.metaKey || e.ctrlKey || e.altKey) return;
      if (document.querySelector('[role="dialog"]')) return;
      const seek = (d: number) => w.setTime(Math.max(0, Math.min(w.getDuration(), w.getCurrentTime() + d)));
      switch (e.key) {
        case " ":
          e.preventDefault();
          if (!useStore.getState().play) return;
          void w.playPause();
          break;
        case "ArrowLeft":
          e.preventDefault();
          seek(-5);
          break;
        case "ArrowRight":
          e.preventDefault();
          seek(5);
          break;
        case "j":
        case "J":
          seek(-10);
          break;
        case "k":
        case "K":
          w.pause();
          break;
        case "l":
        case "L":
          seek(10);
          if (!w.isPlaying()) void w.play();
          break;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // Rows ask for "load and play" (double click, play buttons).
  useEffect(() => {
    const onPlayRequest = (e: Event) => {
      const key = (e as CustomEvent<string>).detail;
      const w = ws.current;
      if (w && loadedKey.current === key) {
        void w.play();
      } else {
        autoplay.current = true; // plays on "ready"
      }
    };
    const onToggle = () => void ws.current?.playPause();
    const onSeek = (e: Event) => {
      const w = ws.current;
      if (!w || !(w.getDuration() > 0)) return;
      w.seekTo((e as CustomEvent<number>).detail);
      if (!w.isPlaying()) void w.play();
    };
    window.addEventListener("beatshelf:play", onPlayRequest);
    window.addEventListener("beatshelf:toggle", onToggle);
    window.addEventListener("beatshelf:seek", onSeek);
    return () => {
      window.removeEventListener("beatshelf:play", onPlayRequest);
      window.removeEventListener("beatshelf:toggle", onToggle);
      window.removeEventListener("beatshelf:seek", onSeek);
    };
  }, []);

  return (
    <footer className="grid h-[72px] shrink-0 grid-cols-[minmax(220px,300px)_1fr_auto] items-center gap-5 border-t border-line bg-surface px-5">
      <div className="flex min-w-0 items-center gap-3">
        <button
          type="button"
          onClick={() => ws.current?.playPause()}
          disabled={!target || !!error}
          aria-label={playing ? t("player.pause") : t("player.play")}
          className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-text text-bg transition-transform hover:scale-105 disabled:opacity-30 disabled:hover:scale-100"
        >
          {playing ? <Pause size={16} /> : <Play size={16} className="translate-x-[1px]" />}
        </button>
        {target ? (
          <div className="min-w-0">
            <div className="truncate font-medium">{target.title}</div>
            <div className="flex items-center gap-2 text-cap text-muted">
              {target.bpm != null && <span className="num">{formatBpm(target.bpm)} BPM</span>}
              {target.key && <KeyChip keyName={target.key} camelot={target.camelot} />}
            </div>
          </div>
        ) : (
          <div className="text-muted">{t("player.nothing")}</div>
        )}
      </div>
      <div className="relative min-w-0">
        <div ref={container} className={`transition-opacity ${error || !target ? "invisible" : ""} ${loading ? "opacity-40" : ""}`} />
        {error && <div className="absolute inset-0 flex items-center text-danger">{t("player.error")}</div>}
      </div>
      <div className="num w-24 text-right text-muted">
        {target ? `${formatTime(time)} / ${formatTime(duration)}` : ""}
      </div>
    </footer>
  );
}

function targetKey(p: PlayTarget): string {
  return `${p.type}:${p.id}`;
}

/** Point the player at a file and start playing as soon as it is loaded. */
export function playNow(p: PlayTarget) {
  const current = useStore.getState().play;
  if (!current || targetKey(current) !== targetKey(p)) useStore.getState().setPlay(p);
  window.dispatchEvent(new CustomEvent("beatshelf:play", { detail: targetKey(p) }));
}

/** Play/pause whatever is loaded. */
export function togglePlay() {
  window.dispatchEvent(new Event("beatshelf:toggle"));
}

/** Jump to a position (0..1) in the loaded file and keep playing. */
export function seekTo(fraction: number) {
  window.dispatchEvent(new CustomEvent("beatshelf:seek", { detail: fraction }));
}
