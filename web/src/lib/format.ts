const dtf = new Map<string, Intl.DateTimeFormat>();

function formatter(tz: string, opts: Intl.DateTimeFormatOptions): Intl.DateTimeFormat {
  const key = tz + JSON.stringify(opts);
  let f = dtf.get(key);
  if (!f) {
    f = new Intl.DateTimeFormat("en-GB", { timeZone: tz, hourCycle: "h23", ...opts });
    dtf.set(key, f);
  }
  return f;
}

function parts(ms: number, tz: string, opts: Intl.DateTimeFormatOptions): Record<string, string> {
  const out: Record<string, string> = {};
  for (const p of formatter(tz, opts).formatToParts(ms)) out[p.type] = p.value;
  return out;
}

const full: Intl.DateTimeFormatOptions = {
  year: "numeric",
  month: "2-digit",
  day: "2-digit",
  hour: "2-digit",
  minute: "2-digit",
  second: "2-digit",
};

export function toMs(v: string | number): number {
  return typeof v === "number" ? v : Date.parse(v);
}

export function fmtHMS(v: string | number, tz: string): string {
  const p = parts(toMs(v), tz, full);
  return `${p.hour}:${p.minute}:${p.second}`;
}

export function fmtHM(v: string | number, tz: string): string {
  const p = parts(toMs(v), tz, full);
  return `${p.hour}:${p.minute}`;
}

export function fmtDateTime(v: string | number, tz: string): string {
  const p = parts(toMs(v), tz, full);
  return `${p.year}-${p.month}-${p.day} ${p.hour}:${p.minute}:${p.second}`;
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

export function fmtMonthDay(v: string | number, tz: string): string {
  const p = parts(toMs(v), tz, full);
  return `${MONTHS[Number(p.month) - 1]} ${Number(p.day)}`;
}

/** `HH:mm:ss.SSSSSS` from an RFC 3339 timestamp, keeping the microsecond digits. */
export function fmtMicros(iso: string, tz: string): string {
  const frac = /\.(\d+)/.exec(iso)?.[1] ?? "";
  return `${fmtHMS(iso, tz)}.${frac.padEnd(6, "0")}`;
}

export function usOf(iso: string): number {
  const frac = /\.(\d+)/.exec(iso)?.[1] ?? "0";
  return Math.floor(Date.parse(iso) / 1000) * 1_000_000 + Number(frac.padEnd(6, "0"));
}

/** Minutes east of UTC for `ms` in `tz`, so labels can be placed on local boundaries. */
export function tzOffsetMs(ms: number, tz: string): number {
  const p = parts(ms, tz, full);
  const asUtc = Date.UTC(+p.year, +p.month - 1, +p.day, +p.hour, +p.minute, +p.second);
  return asUtc - Math.floor(ms / 1000) * 1000;
}

export function fmtDuration(ms: number | null | undefined): string {
  if (ms === null || ms === undefined) return "–";
  if (ms < 1000) return `${Math.round(ms)} ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(ms < 10_000 ? 2 : 1)} s`;
  const s = Math.round(ms / 1000);
  if (s < 3600) return `${Math.floor(s / 60)} m ${String(s % 60).padStart(2, "0")} s`;
  return `${Math.floor(s / 3600)} h ${String(Math.floor((s % 3600) / 60)).padStart(2, "0")} m`;
}

export function fmtDelta(us: number): string {
  const ms = us / 1000;
  if (ms < 1000) return `+${ms.toFixed(ms < 10 ? 2 : 1)} ms`;
  return `+${(ms / 1000).toFixed(3)} s`;
}

export function fmtInt(v: number): string {
  return Math.round(v).toLocaleString("en-US");
}

export function compact(v: number): string {
  const a = Math.abs(v);
  if (a >= 1e6) return `${trim(v / 1e6)}M`;
  if (a >= 1e3) return `${trim(v / 1e3)}K`;
  if (a >= 100 || Number.isInteger(v)) return String(Math.round(v));
  if (a >= 10) return v.toFixed(1);
  return v.toFixed(2);
}

function trim(v: number): string {
  return v.toFixed(Math.abs(v) >= 100 ? 0 : 1).replace(/\.0$/, "");
}

export interface Formatted {
  value: string;
  unit: string;
}

/** One number formatter for every chart, stat and table (FR-CH-17). */
export function formatMetric(v: number | null | undefined, unit: string): Formatted {
  if (v === null || v === undefined || Number.isNaN(v)) return { value: "–", unit: "" };
  switch (unit) {
    case "%":
      return { value: v.toFixed(1), unit: "%" };
    case "ms":
      if (v >= 60_000) return { value: (v / 60_000).toFixed(1), unit: "min" };
      if (v >= 1000) return { value: (v / 1000).toFixed(2), unit: "s" };
      return { value: v >= 10 ? Math.round(v).toString() : v.toFixed(1), unit: "ms" };
    case "s":
      if (v >= 3600) return { value: (v / 3600).toFixed(1), unit: "h" };
      if (v >= 60) return { value: `${Math.floor(v / 60)}:${String(Math.round(v % 60)).padStart(2, "0")}`, unit: "min" };
      return { value: v.toFixed(0), unit: "s" };
    case "mos":
      return { value: v.toFixed(2), unit: "" };
    case "/s":
    case "pps":
      return { value: v >= 100 ? compact(v) : v.toFixed(v >= 10 ? 1 : 2), unit: unit === "pps" ? "pkt/s" : "/s" };
    case "erl":
      return { value: v.toFixed(1), unit: "E" };
    case "min":
      return { value: compact(v), unit: "min" };
    case "MB":
      return { value: compact(v), unit: "MB" };
    default:
      return { value: v >= 10_000 ? compact(v) : Number.isInteger(v) ? fmtInt(v) : v >= 10 ? v.toFixed(0) : v.toFixed(1), unit: "" };
  }
}

export function metricText(v: number | null | undefined, unit: string): string {
  const f = formatMetric(v, unit);
  return f.unit ? `${f.value} ${f.unit}` : f.value;
}

export function mosVerdict(m: number | null | undefined): "good" | "fair" | "poor" | "bad" | null {
  if (m === null || m === undefined) return null;
  if (m >= 4.0) return "good";
  if (m >= 3.6) return "fair";
  if (m >= 3.1) return "poor";
  return "bad";
}

export function maskNumber(n: string): string {
  return n.length <= 4 ? n : `${"•".repeat(Math.min(n.length - 4, 6))}${n.slice(-4)}`;
}

export function downloadBlob(name: string, content: BlobPart, type: string) {
  const url = URL.createObjectURL(new Blob([content], { type }));
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 2000);
}
