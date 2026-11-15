import { useEffect, useRef, type ButtonHTMLAttributes, type InputHTMLAttributes, type ReactNode } from "react";
import { AudioLines, MicAudioLines, Repeat, Sprout, X } from "lucide-react";
import type { ItemKind } from "../types";
import { keyColor, keyTint } from "../lib/camelot";

type Tone = "primary" | "quiet" | "danger";

export function Button({
  tone = "quiet",
  className = "",
  children,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { tone?: Tone }) {
  const tones: Record<Tone, string> = {
    primary: "bg-primary-bg text-primary-fg hover:opacity-90",
    quiet: "bg-raised text-text hover:bg-line",
    danger: "bg-danger-bg text-danger hover:opacity-90",
  };
  return (
    <button
      type="button"
      className={`inline-flex h-8 items-center gap-2 rounded-full px-4 text-body font-medium transition-[background,opacity] disabled:cursor-not-allowed disabled:opacity-40 ${tones[tone]} ${className}`}
      {...rest}
    >
      {children}
    </button>
  );
}

export function IconButton({
  label,
  className = "",
  children,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { label: string }) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      className={`inline-flex h-8 w-8 items-center justify-center rounded-full text-muted transition-colors hover:bg-raised hover:text-text disabled:opacity-40 ${className}`}
      {...rest}
    >
      {children}
    </button>
  );
}

export function TextInput({ className = "", ...rest }: InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      className={`h-8 w-full rounded-lg border border-line bg-surface px-2.5 text-body outline-none placeholder:text-faint focus:border-focus ${className}`}
      {...rest}
    />
  );
}

export function Label({ children, hint }: { children: ReactNode; hint?: ReactNode }) {
  return (
    <div className="mb-2">
      <div className="text-body font-medium">{children}</div>
      {hint && <div className="text-cap text-muted">{hint}</div>}
    </div>
  );
}

export function Segmented<T extends string>({
  value,
  options,
  onChange,
}: {
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
}) {
  return (
    <div role="radiogroup" className="inline-flex rounded-full bg-raised p-0.5">
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          role="radio"
          aria-checked={value === o.value}
          onClick={() => onChange(o.value)}
          className={`h-7 rounded-full px-3 text-body transition-colors ${
            value === o.value ? "bg-text font-medium text-bg" : "text-muted hover:text-text"
          }`}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

/** Modal shell: scrim with blur, solid panel, Esc closes, focus moves inside. */
export function Dialog({
  title,
  onClose,
  children,
  footer,
  width = 640,
}: {
  title: ReactNode;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
  width?: number;
}) {
  const panel = useRef<HTMLDivElement>(null);
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  useEffect(() => {
    const prev = document.activeElement as HTMLElement | null;
    const first = panel.current?.querySelector<HTMLElement>("input, select, button:not([data-close])");
    (first ?? panel.current)?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        closeRef.current();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      prev?.focus?.();
    };
  }, []);

  return (
    <div className="scrim fixed inset-0 z-40 flex items-center justify-center p-6" onMouseDown={onClose}>
      <div
        ref={panel}
        role="dialog"
        aria-modal="true"
        tabIndex={-1}
        onMouseDown={(e) => e.stopPropagation()}
        style={{ maxWidth: width }}
        className="pop-in flex max-h-full w-full flex-col overflow-hidden rounded-2xl border border-line bg-surface shadow-2xl outline-none"
      >
        <div className="flex items-center justify-between border-b border-line px-5 py-3">
          <div className="text-sub font-semibold">{title}</div>
          <IconButton label="Esc" data-close onClick={onClose}>
            <X size={16} />
          </IconButton>
        </div>
        <div className="min-h-0 flex-1 overflow-auto px-5 py-4">{children}</div>
        {footer && <div className="flex items-center justify-end gap-2 border-t border-line px-5 py-3">{footer}</div>}
      </div>
    </div>
  );
}

export function KeyChip({ keyName, camelot }: { keyName: string | null; camelot: string | null }) {
  if (!keyName) return null;
  const color = keyColor(camelot);
  return (
    <span
      className="num inline-flex h-6 items-center gap-1.5 rounded-full px-2 text-body"
      style={{ background: keyTint(camelot) ?? undefined }}
    >
      <span className="h-2 w-2 rounded-full" style={{ background: color ?? "var(--faint)" }} />
      <span className="font-medium">{keyName}</span>
      {camelot && <span className="text-cap text-muted">{camelot}</span>}
    </span>
  );
}

/** Two-letter DAW mark; filled when it is the most recently changed project. */
export function DawMark({ daw, latest, title }: { daw: "fl" | "ableton"; latest?: boolean; title?: string }) {
  const text = daw === "fl" ? "FL" : "Ab";
  return (
    <span
      title={title}
      className={`inline-flex h-5 min-w-6 items-center justify-center rounded px-1 text-[11px] font-semibold ${
        latest ? "bg-text text-bg" : "border border-line text-muted"
      }`}
    >
      {text}
    </span>
  );
}

export function Empty({ children, action }: { children: ReactNode; action?: ReactNode }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-4 px-8 text-center">
      <p className="max-w-[44ch] text-sub text-muted">{children}</p>
      {action}
    </div>
  );
}

/** One glyph per section of the library. */
export const KIND_ICON: Record<ItemKind, typeof AudioLines> = {
  beat: AudioLines,
  loop: Repeat,
  starter: Sprout,
  voicetag: MicAudioLines,
};
