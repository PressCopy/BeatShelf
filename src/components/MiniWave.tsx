import { memo, useEffect, useMemo, useState, type CSSProperties, type MouseEvent } from "react";
import { api } from "../api";
import { useStore } from "../store";

// ── Peaks for list rows: fetched in small batches, kept for the session. ──

const cache = new Map<number, number[]>();
const waiting = new Map<number, Set<() => void>>();
let queue: number[] = [];
let busy = false;
let timer: ReturnType<typeof setTimeout> | undefined;

function settle(id: number, peaks: number[]) {
  cache.set(id, peaks);
  waiting.get(id)?.forEach((done) => done());
  waiting.delete(id);
}

async function pump() {
  if (busy) return;
  busy = true;
  while (queue.length) {
    // Newest requests first: those are the rows on screen right now.
    const batch = queue.splice(-6).filter((id) => waiting.has(id));
    if (!batch.length) continue;
    try {
      const got = new Map((await api.rowPeaks(batch)).map((r) => [r.id, r.peaks]));
      for (const id of batch) settle(id, got.get(id) ?? []);
    } catch {
      for (const id of batch) settle(id, []);
    }
  }
  busy = false;
}

/** Ask for a file's peaks. `done` fires once they are cached; the returned
 *  function cancels the request (a row that scrolled away). */
function requestPeaks(id: number, done: () => void): () => void {
  let set = waiting.get(id);
  if (!set) {
    set = new Set();
    waiting.set(id, set);
    queue.push(id);
  }
  set.add(done);
  if (!busy && !timer) {
    // Let the rows of one render pass queue up before the first request.
    timer = setTimeout(() => {
      timer = undefined;
      void pump();
    }, 30);
  }
  return () => {
    const s = waiting.get(id);
    if (!s) return;
    s.delete(done);
    if (s.size === 0) {
      waiting.delete(id);
      queue = queue.filter((q) => q !== id);
    }
  };
}

// ── Drawing ──

const BARS = 64;
const H = 28;

function pathOf(peaks: number[]): string {
  const max = Math.max(1, ...peaks);
  let d = "";
  for (let i = 0; i < BARS; i++) {
    const a = Math.floor((i * peaks.length) / BARS);
    const b = Math.max(a + 1, Math.floor(((i + 1) * peaks.length) / BARS));
    let p = 0;
    for (let j = a; j < b && j < peaks.length; j++) p = Math.max(p, peaks[j]);
    const h = Math.max(0.8, (p / max) * (H / 2 - 1));
    d += `M${i + 0.5} ${(H / 2 - h).toFixed(1)}V${(H / 2 + h).toFixed(1)}`;
  }
  return d;
}

function Bars({ d, className, style }: { d: string; className?: string; style?: CSSProperties }) {
  return (
    <svg
      viewBox={`0 0 ${BARS} ${H}`}
      preserveAspectRatio="none"
      className={`absolute inset-0 h-full w-full ${className ?? ""}`}
      style={style}
      aria-hidden
    >
      <path d={d} stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" vectorEffect="non-scaling-stroke" fill="none" />
    </svg>
  );
}

/** The played part, in the key's colour. Only the playing row subscribes to progress. */
function Played({ d, color }: { d: string; color: string }) {
  const progress = useStore((s) => s.playProgress);
  return <Bars d={d} style={{ color, clipPath: `inset(0 ${(100 - progress * 100).toFixed(2)}% 0 0)` }} />;
}

/** Waveform preview inside a list row. Click to play, or to seek when it is already playing. */
export const MiniWave = memo(function MiniWave({
  assetId,
  color,
  active,
  onPick,
}: {
  assetId: number | null;
  color: string;
  active: boolean;
  onPick: (fraction: number) => void;
}) {
  const [, bump] = useState(0);
  const peaks = assetId != null ? cache.get(assetId) : undefined;

  useEffect(() => {
    if (assetId == null || cache.has(assetId)) return;
    return requestPeaks(assetId, () => bump((n) => n + 1));
  }, [assetId]);

  const d = useMemo(() => (peaks && peaks.length ? pathOf(peaks) : null), [peaks]);

  if (!d) {
    // No audio yet, or the peaks are still being computed.
    return (
      <div className="flex h-7 w-full items-center" aria-hidden>
        <div className={`h-px w-full bg-line ${assetId != null && !peaks ? "animate-pulse" : ""}`} />
      </div>
    );
  }
  const pick = (e: MouseEvent<HTMLDivElement>) => {
    const box = e.currentTarget.getBoundingClientRect();
    onPick(Math.min(1, Math.max(0, (e.clientX - box.left) / box.width)));
  };
  return (
    <div className="relative h-7 w-full cursor-pointer" onClick={pick}>
      <Bars d={d} className="text-wave" />
      {active && <Played d={d} color={color} />}
    </div>
  );
});
