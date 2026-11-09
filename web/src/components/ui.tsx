import { CircleAlert, CircleCheck, Info, TriangleAlert } from "lucide-react";
import type { ComponentProps, ReactNode } from "react";
import { mosVerdict } from "../lib/format";

export function Card({ title, right, children, className = "" }: { title?: ReactNode; right?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <section className={`bg-surface border border-line rounded-lg ${className}`}>
      {(title || right) && (
        <header className="flex items-center justify-between gap-3 px-3.5 pt-2.5 pb-1.5">
          <h2 className="font-semibold text-[13px] text-ink">{title}</h2>
          <div className="flex items-center gap-2 text-ink-3 text-xs">{right}</div>
        </header>
      )}
      {children}
    </section>
  );
}

export function PageHeader({ title, description, right }: { title: string; description?: ReactNode; right?: ReactNode }) {
  return (
    <div className="flex items-end justify-between gap-4 mb-3.5">
      <div>
        <h1 className="text-xl font-semibold tracking-tight">{title}</h1>
        {description && <p className="text-ink-2 mt-0.5">{description}</p>}
      </div>
      {right}
    </div>
  );
}

type Tone = "good" | "warn" | "serious" | "crit" | "info" | "neutral";

const toneClass: Record<Tone, string> = {
  good: "bg-good-wash text-good",
  warn: "bg-warn-wash text-warn",
  serious: "bg-serious-wash text-serious",
  crit: "bg-crit-wash text-crit",
  info: "bg-info-wash text-info",
  neutral: "bg-sunken text-ink-2",
};

export function Badge({ tone = "neutral", children, title, mono }: { tone?: Tone; children: ReactNode; title?: string; mono?: boolean }) {
  return (
    <span title={title} className={`inline-flex items-center gap-1 rounded px-1.5 py-px text-[11.5px] font-medium whitespace-nowrap ${toneClass[tone]} ${mono ? "font-mono" : ""}`}>
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
  ONE_WAY_AUDIO: "critical",
  NO_MEDIA: "critical",
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
  return <Badge tone={tone}>{state}</Badge>;
}

export function RegStatus({ status }: { status: string }) {
  if (status === "registered")
    return (
      <Badge tone="good">
        <CircleCheck size={12} aria-hidden />
        Registered
      </Badge>
    );
  if (status === "failing")
    return (
      <Badge tone="crit">
        <CircleAlert size={12} aria-hidden />
        Failing
      </Badge>
    );
  return <Badge>Not registered</Badge>;
}

export function Button({ children, variant = "default", className = "", ...rest }: ComponentProps<"button"> & { variant?: "default" | "primary" | "ghost" }) {
  const styles = {
    default: "bg-raised border border-line-2 hover:bg-sunken text-ink",
    primary: "bg-accent border border-accent text-white hover:opacity-90",
    ghost: "text-ink-2 hover:bg-sunken",
  };
  return (
    <button {...rest} className={`inline-flex items-center gap-1.5 rounded-md px-2.5 h-7 text-[12.5px] font-medium disabled:opacity-50 ${styles[variant]} ${className}`}>
      {children}
    </button>
  );
}

export function Tabs({ tabs, active, onChange }: { tabs: { id: string; label: ReactNode }[]; active: string; onChange: (id: string) => void }) {
  return (
    <div role="tablist" className="flex gap-0.5 border-b border-line overflow-x-auto overflow-y-hidden">
      {tabs.map((t) => (
        <button
          key={t.id}
          role="tab"
          aria-selected={active === t.id}
          onClick={() => onChange(t.id)}
          className={`px-3 py-2 text-[13px] whitespace-nowrap -mb-px border-b-2 ${active === t.id ? "border-accent text-ink font-medium" : "border-transparent text-ink-2 hover:text-ink"}`}
        >
          {t.label}
        </button>
      ))}
    </div>
  );
}

export function Empty({ children }: { children: ReactNode }) {
  return <div className="text-ink-3 text-center py-8 px-4">{children}</div>;
}

export function ErrorNote({ error }: { error: unknown }) {
  return <div className="text-crit px-3.5 py-4">{error instanceof Error ? error.message : "Request failed"}</div>;
}

export function Loading() {
  return <div className="text-ink-3 px-3.5 py-6">Loading…</div>;
}

export function KV({ label, children, mono }: { label: string; children: ReactNode; mono?: boolean }) {
  return (
    <div className="min-w-0">
      <div className="text-ink-3 text-[11.5px]">{label}</div>
      <div className={`truncate ${mono ? "font-mono text-xs" : ""}`}>{children}</div>
    </div>
  );
}

export function Select({ value, onChange, options, label, className = "" }: { value: string; onChange: (v: string) => void; options: { value: string; label: string }[]; label?: string; className?: string }) {
  return (
    <select aria-label={label} value={value} onChange={(e) => onChange(e.target.value)} className={`h-7 rounded-md bg-raised border border-line-2 px-1.5 text-[12.5px] text-ink ${className}`}>
      {options.map((o) => (
        <option key={o.value} value={o.value}>
          {o.label}
        </option>
      ))}
    </select>
  );
}

export function Toggle({ checked, onChange, children }: { checked: boolean; onChange: (v: boolean) => void; children: ReactNode }) {
  return (
    <label className="inline-flex items-center gap-1.5 text-[12.5px] text-ink-2 cursor-pointer select-none">
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} className="accent-[var(--accent)]" />
      {children}
    </label>
  );
}
