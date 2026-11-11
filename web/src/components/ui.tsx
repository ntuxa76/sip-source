import { CircleAlert, Info, TriangleAlert, X } from "lucide-react";
import { useEffect, useId, useRef, type ComponentProps, type ReactNode, type RefObject } from "react";
import { createPortal } from "react-dom";
import { mosVerdict } from "../lib/format";

/** A bordered panel. With a title it gets a heading row and a hairline under it. */
export function Card({ title, right, children, className = "" }: { title?: ReactNode; right?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <section className={`bg-surface border border-line rounded-lg ${className}`}>
      {(title || right) && (
        <header className="flex flex-wrap items-center justify-between gap-x-3 gap-y-1 min-h-10 px-3 py-1.5 border-b border-line">
          <h2 className="font-semibold text-ink">{title}</h2>
          <div className="flex flex-wrap items-center gap-2 text-ink-3 text-xs">{right}</div>
        </header>
      )}
      {children}
    </section>
  );
}

export function PageHeader({ title, description, right }: { title: string; description?: ReactNode; right?: ReactNode }) {
  return (
    <div className="flex flex-wrap items-end justify-between gap-x-4 gap-y-2 mb-3">
      <div className="min-w-0">
        <h1 className="text-xl font-semibold">{title}</h1>
        {description && <p className="text-ink-2 mt-0.5 max-w-[88ch]">{description}</p>}
      </div>
      {right}
    </div>
  );
}

export type Tone = "good" | "warn" | "serious" | "crit" | "info" | "neutral";

const toneClass: Record<Tone, string> = {
  good: "bg-good-wash text-good",
  warn: "bg-warn-wash text-warn",
  serious: "bg-serious-wash text-serious",
  crit: "bg-crit-wash text-crit",
  info: "bg-info-wash text-info",
  neutral: "bg-sunken text-ink-2",
};

const dotClass: Record<Tone, string> = {
  good: "bg-good",
  warn: "bg-warn",
  serious: "bg-serious",
  crit: "bg-crit",
  info: "bg-info",
  neutral: "bg-ink-3",
};

/** A small squared tag, for codes and labels that are read as a unit. */
export function Badge({ tone = "neutral", children, title, mono }: { tone?: Tone; children: ReactNode; title?: string; mono?: boolean }) {
  return (
    <span
      title={title}
      className={`inline-flex items-center gap-1 h-5 px-1.5 rounded-xs ring-1 ring-inset ring-current/15 text-xs font-medium whitespace-nowrap ${toneClass[tone]} ${mono ? "font-mono" : ""}`}
    >
      {children}
    </span>
  );
}

/** A state written in plain text with a coloured dot in front. The words carry the meaning, the dot only helps scanning. */
export function Status({ tone = "neutral", children, title }: { tone?: Tone; children: ReactNode; title?: string }) {
  return (
    <span title={title} className="inline-flex items-center gap-1.5 whitespace-nowrap">
      <span aria-hidden className={`w-1.5 h-1.5 shrink-0 rounded-full ${dotClass[tone]}`} />
      {children}
    </span>
  );
}

export function statusTone(s: number): Tone {
  if (s === 0) return "info";
  if (s < 300) return "good";
  if (s < 400) return "info";
  if (s < 500) return "warn";
  return "crit";
}

export function StatusChip({ status, reason }: { status: number; reason?: string }) {
  if (status === 0) return <Badge tone="info">in progress</Badge>;
  return (
    <Badge tone={statusTone(status)} title={reason} mono>
      {status}
    </Badge>
  );
}

const severityIcon = { critical: CircleAlert, warning: TriangleAlert, info: Info };

export function SeverityBadge({ severity, children }: { severity: string; children?: ReactNode }) {
  const tone: Tone = severity === "critical" ? "crit" : severity === "warning" ? "warn" : "info";
  const Icon = severityIcon[severity as keyof typeof severityIcon] ?? Info;
  return (
    <Badge tone={tone}>
      <Icon size={12} aria-hidden />
      {children ?? severity}
    </Badge>
  );
}

const FINDING_SEVERITY: Record<string, string> = {
  NO_FINAL_RESPONSE: "critical",
  ACK_MISSING: "critical",
  DROP_AFTER_32S: "warning",
  AUTH_LOOP: "warning",
  NO_COMMON_CODEC: "warning",
  PRIVATE_IP_IN_SDP: "warning",
  HIGH_PDD: "warning",
  RETRANSMISSIONS: "warning",
  POOR_QUALITY: "warning",
  SESSION_TIMER_FAILURE: "warning",
  SCANNER: "warning",
  HIGH_RISK_DESTINATION: "warning",
};

export function FindingBadge({ code }: { code: string }) {
  return <SeverityBadge severity={FINDING_SEVERITY[code] ?? "info"}>{code}</SeverityBadge>;
}

export function MosCell({ mos }: { mos: number | null }) {
  const v = mosVerdict(mos);
  if (v === null) return <span className="text-ink-3">–</span>;
  const tone: Record<string, string> = { good: "text-ink", fair: "text-ink", poor: "text-warn font-medium", bad: "text-crit font-medium" };
  return <span className={tone[v]} title={v}>{mos!.toFixed(2)}</span>;
}

export function CallStateBadge({ state }: { state: string }) {
  const tone: Tone = state === "completed" ? "good" : state === "failed" || state === "timeout" ? "crit" : state === "cancelled" ? "neutral" : "info";
  return <Status tone={tone}>{state}</Status>;
}

export function RegStatus({ status }: { status: string }) {
  if (status === "registered") return <Status tone="good">Registered</Status>;
  if (status === "failing") return <Status tone="crit">Failing</Status>;
  return <Status>Not registered</Status>;
}

const buttonVariant = {
  default: "bg-raised border-line-2 text-ink enabled:hover:bg-hover",
  primary: "bg-accent border-accent text-on-accent enabled:hover:bg-accent-strong enabled:hover:border-accent-strong",
  ghost: "border-transparent text-ink-2 enabled:hover:bg-hover enabled:hover:text-ink",
  danger: "bg-raised border-line-2 text-crit enabled:hover:bg-crit-wash enabled:hover:border-crit",
};

const buttonSize = {
  md: "min-h-8 px-3 gap-1.5",
  sm: "h-7 px-2.5 gap-1 text-xs",
  icon: "h-8 w-8",
};

export function Button({
  children,
  variant = "default",
  size = "md",
  className = "",
  ...rest
}: ComponentProps<"button"> & { variant?: keyof typeof buttonVariant; size?: keyof typeof buttonSize }) {
  return (
    <button
      {...rest}
      className={`inline-flex items-center justify-center shrink-0 rounded-md border font-medium whitespace-nowrap transition-colors duration-100 disabled:opacity-50 ${buttonSize[size]} ${buttonVariant[variant]} ${className}`}
    >
      {children}
    </button>
  );
}

/** A square button that holds only an icon. The label is what a screen reader and the tooltip say. */
export function IconButton({ label, children, className = "", ...rest }: ComponentProps<"button"> & { label: string }) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      {...rest}
      className={`inline-grid place-items-center w-7 h-7 shrink-0 rounded-md text-ink-2 transition-colors duration-100 enabled:hover:bg-hover enabled:hover:text-ink disabled:opacity-50 ${className}`}
    >
      {children}
    </button>
  );
}

export function Tabs({ tabs, active, onChange }: { tabs: { id: string; label: ReactNode }[]; active: string; onChange: (id: string) => void }) {
  return (
    <div className="overflow-x-auto overflow-y-hidden">
      <div role="tablist" className="flex min-w-max shadow-[inset_0_-1px_0_var(--line)]">
        {tabs.map((t) => (
          <button
            key={t.id}
            type="button"
            role="tab"
            aria-selected={active === t.id}
            onClick={() => onChange(t.id)}
            className={`h-9 px-3 border-b-2 font-medium whitespace-nowrap transition-colors duration-100 focus-visible:-outline-offset-2 ${
              active === t.id ? "border-accent text-ink" : "border-transparent text-ink-2 hover:text-ink hover:border-line-2"
            }`}
          >
            {t.label}
          </button>
        ))}
      </div>
    </div>
  );
}

export function Empty({ children }: { children: ReactNode }) {
  return <div className="text-ink-3 text-center py-8 px-4">{children}</div>;
}

export function ErrorNote({ error }: { error: unknown }) {
  return (
    <div role="alert" className="flex items-start gap-2 px-3 py-3 text-crit">
      <CircleAlert size={14} aria-hidden className="shrink-0 mt-[3px]" />
      <span className="min-w-0 break-words">{error instanceof Error ? error.message : "Request failed"}</span>
    </div>
  );
}

export function Loading() {
  return (
    <div role="status" className="text-ink-3 px-3 py-6">
      Loading…
    </div>
  );
}

export function KV({ label, children, mono }: { label: string; children: ReactNode; mono?: boolean }) {
  return (
    <div className="min-w-0">
      <div className="text-ink-3 text-xs">{label}</div>
      <div className={`truncate ${mono ? "font-mono text-xs leading-5" : ""}`}>{children}</div>
    </div>
  );
}

/** The look of every text field, select and textarea, without a size. */
export const fieldClass = "rounded-md bg-raised border border-control text-ink placeholder:text-ink-3 disabled:opacity-60 focus-visible:-outline-offset-1";

/** A full-width field of the standard height. */
export const inputClass = `${fieldClass} w-full h-8 px-2.5`;

/** The floating surface of menus and pickers. */
export const popoverClass = "bg-raised border border-line-2 rounded-md shadow-pop";

/** A label above a control, with an optional line of help under it. */
export function Field({ label, hint, children, className = "" }: { label: ReactNode; hint?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <label className={`block ${className}`}>
      <span className="block mb-1 text-xs font-medium text-ink-2">{label}</span>
      {children}
      {hint && <span className="block mt-1 text-xs text-ink-3">{hint}</span>}
    </label>
  );
}

export function Select({ value, onChange, options, label, className = "" }: { value: string; onChange: (v: string) => void; options: { value: string; label: string }[]; label?: string; className?: string }) {
  return (
    <select
      aria-label={label}
      value={value}
      onChange={(e) => onChange(e.target.value)}
      className={`${fieldClass} h-8 pl-2.5 ${className}`}
    >
      {options.map((o) => (
        <option key={o.value} value={o.value}>
          {o.label}
        </option>
      ))}
    </select>
  );
}

export function Toggle({ checked, onChange, children, label }: { checked: boolean; onChange: (v: boolean) => void; children: ReactNode; label?: string }) {
  return (
    <label className="inline-flex items-center gap-2 min-h-6 text-ink-2 cursor-pointer select-none">
      <input type="checkbox" aria-label={label} checked={checked} onChange={(e) => onChange(e.target.checked)} />
      {children}
    </label>
  );
}

/** Closes a menu or picker when the pointer goes down outside `ref`, or on Escape. */
export function useDismiss(ref: RefObject<HTMLElement | null>, open: boolean, onDismiss: () => void) {
  useEffect(() => {
    if (!open) return;
    const down = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) onDismiss();
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") onDismiss();
    };
    document.addEventListener("mousedown", down);
    document.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("mousedown", down);
      document.removeEventListener("keydown", key);
    };
  }, [ref, open, onDismiss]);
}

const FOCUSABLE = 'a[href], button:not([disabled]), input:not([disabled]):not([type="hidden"]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/**
 * A modal dialog. Focus moves into it and stays there until it closes, Escape and a click on the
 * backdrop close it, and focus then returns to whatever opened it.
 */
export function Dialog({
  title,
  onClose,
  children,
  footer,
  width = 560,
}: {
  title: ReactNode;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
  width?: number;
}) {
  const panel = useRef<HTMLDivElement>(null);
  const titleId = useId();
  const close = useRef(onClose);
  useEffect(() => {
    close.current = onClose;
  });

  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null;
    const el = panel.current!;
    // Start in the first field when there is one, otherwise on the dialog itself.
    (el.querySelector<HTMLElement>("input:not([type=hidden]):not([type=checkbox]), textarea, select") ?? el).focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        close.current();
        return;
      }
      if (e.key !== "Tab") return;
      const items = Array.from(el.querySelectorAll<HTMLElement>(FOCUSABLE)).filter((i) => i.offsetParent !== null);
      if (!items.length) {
        e.preventDefault();
        return;
      }
      const first = items[0];
      const last = items[items.length - 1];
      const at = document.activeElement;
      if (e.shiftKey && (at === first || at === el)) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && (at === last || !el.contains(at))) {
        e.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("keydown", onKey, true);
      opener?.focus?.();
    };
  }, []);

  return createPortal(
    <div className="fixed inset-0 z-50 grid place-items-center p-3 sm:p-6 bg-scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div
        ref={panel}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        style={{ width: `min(${width}px, 100%)` }}
        className="flex flex-col max-h-full bg-raised border border-line-2 rounded-lg shadow-pop focus:outline-none"
      >
        <header className="flex items-center justify-between gap-3 h-11 shrink-0 pl-4 pr-2 border-b border-line">
          <h2 id={titleId} className="text-sm font-semibold truncate">
            {title}
          </h2>
          <IconButton label="Close" onClick={onClose}>
            <X size={16} aria-hidden />
          </IconButton>
        </header>
        <div className="min-h-0 p-4 overflow-y-auto overscroll-contain">{children}</div>
        {footer && <footer className="flex flex-wrap items-center justify-end gap-2 shrink-0 px-4 py-3 border-t border-line">{footer}</footer>}
      </div>
    </div>,
    document.body,
  );
}
