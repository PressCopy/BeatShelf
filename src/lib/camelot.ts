// Key colours follow the Camelot wheel: neighbouring numbers are harmonically
// compatible, so they get neighbouring hues. Minor (A) is a touch darker.

export function camelotParts(code: string | null | undefined): { n: number; minor: boolean } | null {
  if (!code) return null;
  const m = /^(1[0-2]|[1-9])([AB])$/i.exec(code.trim());
  if (!m) return null;
  return { n: Number(m[1]), minor: m[2].toUpperCase() === "A" };
}

export function keyHue(code: string | null | undefined): number | null {
  const p = camelotParts(code);
  return p ? ((p.n - 1) * 30 + 20) % 360 : null;
}

export function keyColor(code: string | null | undefined): string | null {
  const p = camelotParts(code);
  if (!p) return null;
  const h = keyHue(code);
  return `hsl(${h} var(--key-s) var(${p.minor ? "--key-l-minor" : "--key-l-major"}))`;
}

/** Same hue at low alpha, for chip backgrounds. */
export function keyTint(code: string | null | undefined, alpha = 0.16): string | null {
  const h = keyHue(code);
  return h === null ? null : `hsl(${h} 70% 60% / ${alpha})`;
}

// ── A beat's own colour ──

/** Swatches offered in the beat card. Mid-tone, readable on dark and light. */
export const BEAT_COLORS = ["#ff5c5c", "#ff9a3d", "#f5c84b", "#7ad66a", "#3fd0b0", "#4db8ff", "#6f8cff", "#a57bff", "#f06fd0", "#b8bcc8"];

const HEX = /^#[0-9a-f]{6}$/i;

function hexAlpha(hex: string, alpha: number): string {
  const n = parseInt(hex.slice(1), 16);
  return `rgba(${n >> 16}, ${(n >> 8) & 255}, ${n & 255}, ${alpha})`;
}

/** The colour a beat is shown in: its own if one is set, otherwise its key's. */
export function beatColor(color: string | null | undefined, camelot: string | null | undefined): string | null {
  return color && HEX.test(color) ? color : keyColor(camelot);
}

/** The same colour at low alpha, for tiles. */
export function beatTint(color: string | null | undefined, camelot: string | null | undefined, alpha = 0.24): string | null {
  return color && HEX.test(color) ? hexAlpha(color, alpha) : keyTint(camelot, alpha);
}
