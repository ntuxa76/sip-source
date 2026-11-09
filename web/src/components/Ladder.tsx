import { useEffect, useMemo, useRef, useState } from "react";
import type { Column, Msg } from "../lib/api";
import { fmtDelta, fmtMicros, usOf } from "../lib/format";

const ROW = 27;
const GAP_ROW = 22;
const TIME_W = 124;
const GAP_THRESHOLD_US = 5_000_000;

const LEG_COLOR: Record<string, string> = { A: "var(--leg-a)", B: "var(--leg-b)" };

function lineColor(m: Msg): string {
  if (m.class === "client_error") return "var(--ladder-warn)";
  if (m.class === "server_error") return "var(--ladder-crit)";
  return LEG_COLOR[m.leg];
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
}

export function Ladder({ columns, messages, selected, onSelect, tz, timeMode, svgRef, scrub = (s) => s }: LadderProps) {
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

  let prevUs = firstUs;
  return (
    <div ref={box} className="ladder">
      <div className="flex sticky top-[-16px] z-10 bg-surface border-b border-line-2">
        <div style={{ width: TIME_W }} className="shrink-0 px-2 py-1.5 text-ink-3 text-[11.5px] self-end">
          {timeMode === "absolute" ? `Time (${tz})` : "Delta"}
        </div>
        {columns.map((c) => (
          <div key={c.name} style={{ width: colW }} className="text-center py-1.5 px-1 min-w-0">
            <div className="font-semibold text-[12.5px] truncate">{scrub(c.name)}</div>
            <div className="text-ink-3 text-[11px] font-mono truncate">{c.ip}{c.capture_id ? ` · capture ${c.capture_id}` : ""}</div>
          </div>
        ))}
      </div>
      <svg ref={svgRef} width={width} height={height} className="block select-none" role="list" aria-label="SIP message flow">
        {columns.map((c, i) => (
          <line key={c.name} x1={x(i)} x2={x(i)} y1={0} y2={height} stroke="var(--line-2)" strokeWidth={1} />
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
          const color = lineColor(m);
          const mid = (x1 + x2) / 2;
          const isSel = selected === m.idx;
          const faded = m.class === "provisional";
          const label = [m.label, m.cseq_method && m.status > 0 ? `(${m.cseq_method})` : "", m.has_sdp ? "· SDP" : "", m.retx ? `· retx ${m.retx}` : "", m.captures.length > 1 ? `· ×${m.captures.length}` : ""].filter(Boolean).join(" ");
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
            >
              <rect x={0} y={r.y} width={width} height={ROW} fill={isSel ? "var(--accent-wash)" : "transparent"} className="ladder-hit" />
              <text x={8} y={r.y + 18} fontSize={11} fill="var(--ink-2)" fontFamily="var(--font-mono)">
                {timeMode === "absolute" ? fmtMicros(m.ts, tz).slice(0, 15) : fmtDelta(delta)}
              </text>
              <line x1={x1} x2={x2 - dir * 7} y1={y} y2={y} stroke={color} strokeWidth={m.class === "request" || m.class === "success" ? 1.75 : 1.25} strokeDasharray={m.retx ? "4 3" : undefined} opacity={faded ? 0.6 : 1} />
              <polygon points={`${x2},${y} ${x2 - dir * 8},${y - 3.75} ${x2 - dir * 8},${y + 3.75}`} fill={color} opacity={faded ? 0.6 : 1} />
              <text x={mid} y={y - 4.5} textAnchor="middle" fontSize={11.5} fontWeight={m.class === "request" || m.status >= 200 ? 500 : 400} fill="var(--ink)" stroke={isSel ? "var(--accent-wash)" : "var(--surface)"} strokeWidth={3} paintOrder="stroke">
                {label}
              </text>
            </g>
          );
        })}
      </svg>
    </div>
  );
}
