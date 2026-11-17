import { useEffect, useMemo, useRef, useState } from "react";
import type { Column, Msg } from "../lib/api";
import { fmtDelta, fmtMicros, usOf } from "../lib/format";

const ROW = 27;
const GAP_ROW = 22;
const TIME_W = 124;
const GAP_THRESHOLD_US = 5_000_000;

const LEG_COLOR: Record<string, string> = { A: "var(--leg-a)", B: "var(--leg-b)" };

/** One colour per kind of message, so a ladder reads at a glance. Requests keep their leg's colour. */
export function msgColor(m: Msg): string {
  if (m.status >= 500) return "var(--ladder-crit)";
  if (m.status >= 400) return "var(--ladder-warn)";
  if (m.status >= 300) return "var(--ladder-redirect)";
  if (m.status >= 200) return "var(--ladder-ok)";
  if (m.status >= 100) return "var(--ladder-prov)";
  if (m.method === "BYE" || m.method === "CANCEL") return "var(--ladder-end)";
  return LEG_COLOR[m.leg];
}

export const LADDER_LEGEND: [string, string][] = [
  ["var(--leg-a)", "Request"],
  ["var(--ladder-prov)", "1xx"],
  ["var(--ladder-ok)", "2xx"],
  ["var(--ladder-redirect)", "3xx"],
  ["var(--ladder-warn)", "4xx"],
  ["var(--ladder-crit)", "5xx / 6xx"],
  ["var(--ladder-end)", "BYE / CANCEL"],
];

/** Times of the call's phases, in microseconds, for the bar beside the times. */
export interface Phases {
  ring: number | null;
  answer: number | null;
  end: number | null;
}

function phaseOf(us: number, p: Phases): [string, string] {
  if (p.end !== null && us >= p.end) return ["var(--line-2)", "ended"];
  if (p.answer !== null && us >= p.answer) return ["var(--phase-talk)", "talking"];
  if (p.ring !== null && us >= p.ring) return ["var(--phase-ring)", "ringing"];
  return ["var(--phase-setup)", "setting up"];
}

type Row = { kind: "msg"; m: Msg; y: number } | { kind: "gap"; seconds: number; y: number };

export function layoutRows(messages: Msg[]): { rows: Row[]; height: number } {
  const rows: Row[] = [];
  let y = 0;
  let prev: number | null = null;
  for (const m of messages) {
    const t = usOf(m.ts);
    if (prev !== null && t - prev > GAP_THRESHOLD_US) {
      rows.push({ kind: "gap", seconds: (t - prev) / 1e6, y });
      y += GAP_ROW;
    }
    rows.push({ kind: "msg", m, y });
    y += ROW;
    prev = t;
  }
  return { rows, height: y + 6 };
}

export interface LadderProps {
  columns: Column[];
  messages: Msg[];
  selected: number | null;
  onSelect: (idx: number) => void;
  tz: string;
  timeMode: "absolute" | "delta";
  svgRef?: React.Ref<SVGSVGElement>;
  scrub?: (s: string) => string;
  phases?: Phases;
}

export function Ladder({ columns, messages, selected, onSelect, tz, timeMode, svgRef, scrub = (s) => s, phases }: LadderProps) {
  const box = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(900);

  useEffect(() => {
    if (!box.current) return;
    const ro = new ResizeObserver((e) => setWidth(e[0].contentRect.width));
    ro.observe(box.current);
    return () => ro.disconnect();
  }, []);

  const { rows, height } = useMemo(() => layoutRows(messages), [messages]);
  const colW = (width - TIME_W) / columns.length;
  const x = (i: number) => TIME_W + colW * (i + 0.5);
  const firstUs = messages.length ? usOf(messages[0].ts) : 0;
  // The software each host announces in User-Agent or Server.
  const software = useMemo(() => {
    const out: Record<number, string> = {};
    for (const m of messages) {
      if (out[m.src] !== undefined) continue;
      const h = m.headers.find(([k]) => k.toLowerCase() === "user-agent" || k.toLowerCase() === "server");
      if (h) out[m.src] = h[1];
    }
    return out;
  }, [messages]);

  let prevUs = firstUs;
  return (
    <div ref={box} className="ladder">
      <div className="flex sticky top-[-16px] z-10 bg-surface border-b border-line-2">
        <div style={{ width: TIME_W }} className="shrink-0 px-2 py-1.5 text-ink-3 text-xs self-end">
          {timeMode === "absolute" ? `Time (${tz})` : "Delta"}
        </div>
        {columns.map((c, i) => (
          <div key={c.name} style={{ width: colW }} className="py-1.5 px-1.5 min-w-0">
            <div className={`ladder-node ${c.role === "capture" ? "ladder-node-capture" : ""}`} title={software[i] ?? ""}>
              <div className="font-semibold text-[13px] truncate">{scrub(c.name)}</div>
              <div className="text-ink-3 text-xs font-mono truncate">{c.ip}{c.capture_id ? ` · capture ${c.capture_id}` : ""}</div>
              {software[i] && <div className="text-ink-2 text-2xs truncate">{software[i]}</div>}
            </div>
          </div>
        ))}
      </div>
      <svg ref={svgRef} width={width} height={height} className="block select-none" role="list" aria-label="SIP message flow">
        {columns.map((c, i) => (
          <line key={c.name} x1={x(i)} x2={x(i)} y1={0} y2={height} stroke="var(--line-2)" strokeWidth={1.5} strokeDasharray="2 3" />
        ))}
        {rows.map((r) => {
          if (r.kind === "gap") {
            return (
              <g key={`gap-${r.y}`}>
                <rect x={0} y={r.y} width={width} height={GAP_ROW} fill="var(--sunken)" />
                <text x={width / 2} y={r.y + 15} textAnchor="middle" fontSize={11} fill="var(--ink-3)">
                  {r.seconds.toFixed(1)} s with no signalling
                </text>
              </g>
            );
          }
          const m = r.m;
          const us = usOf(m.ts);
          const delta = us - prevUs;
          prevUs = us;
          const [x1, x2] = [x(m.src), x(m.dst)];
          const dir = x2 > x1 ? 1 : -1;
          const y = r.y + ROW - 7;
          const color = msgColor(m);
          const mid = (x1 + x2) / 2;
          const isSel = selected === m.idx;
          const faded = m.class === "provisional";
          const label = [m.label, m.cseq_method && m.status > 0 ? `(${m.cseq_method})` : "", m.retx ? `· retx ${m.retx}` : "", m.captures.length > 1 ? `· ×${m.captures.length}` : ""].filter(Boolean).join(" ");
          const labelW = Math.min(Math.abs(x2 - x1) - 16, label.length * 6.6 + (m.has_sdp ? 34 : 14));
          const phase = phases ? phaseOf(us, phases) : null;
          return (
            <g
              key={m.idx}
              role="listitem"
              tabIndex={0}
              aria-label={`${columns[m.src].name} to ${columns[m.dst].name}: ${m.label}`}
              onClick={() => onSelect(m.idx)}
              onKeyDown={(e) => {
                if (e.key === "Enter") onSelect(m.idx);
              }}
              className="cursor-pointer ladder-row"
              style={{ "--row-delay": `${Math.min(r.y / ROW, 40) * 18}ms` } as React.CSSProperties}
            >
              <rect x={0} y={r.y} width={width} height={ROW} fill={isSel ? "var(--accent-wash)" : "transparent"} className="ladder-hit" />
              {phase && (
                <rect x={0} y={r.y} width={3} height={ROW} fill={phase[0]}>
                  <title>{phase[1]}</title>
                </rect>
              )}
              <text x={10} y={r.y + 18} fontSize={11} fill="var(--ink-2)" fontFamily="var(--font-mono)">
                {timeMode === "absolute" ? fmtMicros(m.ts, tz).slice(0, 15) : fmtDelta(delta)}
              </text>
              <line className="ladder-arrow" x1={x1} x2={x2 - dir * 7} y1={y} y2={y} stroke={color} strokeWidth={m.class === "request" || m.class === "success" || m.status >= 300 ? 2 : 1.5} strokeDasharray={m.retx ? "4 3" : undefined} opacity={faded ? 0.75 : 1} />
              <polygon className="ladder-head" points={`${x2},${y} ${x2 - dir * 9},${y - 4.5} ${x2 - dir * 9},${y + 4.5}`} fill={color} opacity={faded ? 0.75 : 1} />
              <circle cx={x1} cy={y} r={2.5} fill={color} opacity={faded ? 0.75 : 1} />
              {labelW > 20 && <rect x={mid - labelW / 2} y={y - 17} width={labelW} height={15} rx={7.5} fill={`color-mix(in srgb, ${color} ${isSel ? 26 : 15}%, var(--surface))`} stroke={`color-mix(in srgb, ${color} 40%, transparent)`} strokeWidth={0.75} />}
              <text x={mid - (m.has_sdp ? 10 : 0)} y={y - 5.5} textAnchor="middle" fontSize={11.5} fontWeight={m.class === "request" || m.status >= 200 ? 600 : 400} fill="var(--ink)">
                {label}
              </text>
              {m.has_sdp && labelW > 40 && (
                <text x={mid + labelW / 2 - 13} y={y - 5.5} textAnchor="middle" fontSize={9} fontWeight={700} fill={color}>SDP</text>
              )}
            </g>
          );
        })}
      </svg>
    </div>
  );
}
