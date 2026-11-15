/** The mark: a lowercase "b" built from one stem and one ring.
 *  A single colour, taken from the surrounding text (`currentColor`). */
export function LogoMark({ size = 22 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" aria-hidden>
      <rect x="3.5" y="2" width="5" height="20" rx="2.5" fill="currentColor" />
      <circle cx="14" cy="14.5" r="5" stroke="currentColor" strokeWidth="5" />
    </svg>
  );
}

/** Mark plus wordmark. */
export function Logo() {
  return (
    <span className="inline-flex items-center gap-2 text-text" aria-label="Beatshelf">
      <LogoMark />
      <span className="text-[17px] font-semibold leading-none tracking-tight" aria-hidden>
        beatshelf
      </span>
    </span>
  );
}
