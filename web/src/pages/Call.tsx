import { LineChart } from "echarts/charts";
import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate, useSearch } from "@tanstack/react-router";
import { Copy, Download, FileText, X } from "lucide-react";
import * as echarts from "echarts/core";
import { useEffect, useMemo, useRef, useState } from "react";
import { useEChart } from "../components/charts";
import { Ladder } from "../components/Ladder";
import { Badge, Button, CallStateBadge, Card, Empty, ErrorNote, FindingBadge, KV, Loading, MosCell, RegStatus, SeverityBadge, StatusChip, Tabs, Toggle } from "../components/ui";
import { get } from "../lib/api";
import type { Call, Flow, Msg } from "../lib/api";
import { usePrefs } from "../lib/context";
import { downloadBlob, fmtDateTime, fmtDuration, fmtHMS, fmtMicros, usOf } from "../lib/format";

echarts.use([LineChart]);

const TABS = [
  { id: "flow", label: "Flow" },
  { id: "timeline", label: "Timeline" },
  { id: "messages", label: "Messages" },
  { id: "media", label: "Media" },
  { id: "logs", label: "Logs" },
  { id: "diagnostics", label: "Diagnostics" },
  { id: "registrations", label: "Registrations" },
  { id: "related", label: "Related" },
  { id: "export", label: "Export" },
];

const noScrub = (s: string) => s;

export function CallPage({ uid }: { uid: string }) {
  const search = useSearch({ strict: false }) as { tab?: string; subject?: string; msg?: string };
  const navigate = useNavigate();
  const { tz, refreshMs } = usePrefs();
  const tab = search.tab ?? "flow";

  const sub = search.subject ? `?subject=${encodeURIComponent(search.subject)}` : "";
  const { data, error } = useQuery({ queryKey: ["call", uid, sub], queryFn: () => get(`/calls/${uid}${sub}`), refetchInterval: (q) => (q.state.data && !q.state.data.call.end_ts ? 3000 : false) });
  const flow = useQuery<Flow>({ queryKey: ["flow", uid, sub], queryFn: () => get(`/calls/${uid}/flow${sub}`), refetchInterval: () => (data && !data.call.end_ts ? 3000 : false) });
  const call: Call | undefined = data?.call;
  const scrub = noScrub;

  const setTab = (id: string) => navigate({ to: ".", search: (p: any) => ({ ...p, tab: id }) } as any);
  const openMsg = (idx: number | null) => navigate({ to: ".", search: (p: any) => ({ ...p, msg: idx === null ? undefined : String(idx) }), replace: true } as any);
  const selected = search.msg !== undefined ? Number(search.msg) : null;

  void refreshMs;
  if (error) return <ErrorNote error={error} />;
  if (!call || !flow.data) return <Loading />;

  const selMsg = selected !== null ? flow.data.messages.find((m) => m.idx === selected) ?? null : null;

  return (
    <div className="space-y-3">
      <Summary call={call} tz={tz} scrub={scrub} />
      <Card>
        <div className="px-2">
          <Tabs tabs={TABS.map((t) => (t.id === "diagnostics" && call.findings.length ? { ...t, label: `Diagnostics (${call.findings.length})` } : t))} active={tab} onChange={setTab} />
        </div>
        <div className="p-3">
          {tab === "flow" && <FlowTab flow={flow.data} selected={selected} onSelect={openMsg} tz={tz} scrub={scrub} call={call} />}
          {tab === "timeline" && <TimelineTab call={call} flow={flow.data} tz={tz} />}
          {tab === "messages" && <MessagesTab flow={flow.data} tz={tz} onSelect={openMsg} scrub={scrub} />}
          {tab === "media" && <MediaTab uid={uid} tz={tz} sub={sub} />}
          {tab === "logs" && <LogsTab uid={uid} tz={tz} sub={sub} />}
          {tab === "diagnostics" && <DiagnosticsTab uid={uid} sub={sub} onEvidence={(i) => navigate({ to: ".", search: (p: any) => ({ ...p, tab: "flow", msg: String(i) }) } as any)} />}
          {tab === "registrations" && <RegistrationsTab uid={uid} tz={tz} sub={sub} />}
          {tab === "related" && <RelatedTab uid={uid} tz={tz} sub={sub} />}
          {tab === "export" && <ExportTab uid={uid} call={call} flow={flow.data} sub={sub} />}
        </div>
      </Card>
      {selMsg && <Drawer m={selMsg} flow={flow.data} tz={tz} scrub={scrub} onClose={() => openMsg(null)} />}
    </div>
  );
}

function Copyable({ value }: { value: string }) {
  return (
    // The padding gives a 24 px target; the negative margins keep the icon where it was.
    <button type="button" className="inline-flex p-1.5 -my-1.5 -mr-1.5 -ml-0.5 rounded text-ink-3 hover:text-ink align-middle" aria-label={`Copy ${value}`} title="Copy" onClick={() => navigator.clipboard?.writeText(value)}>
      <Copy size={12} />
    </button>
  );
}

function Summary({ call: c, tz, scrub }: { call: Call; tz: string; scrub: (s: string) => string }) {
  const crit = ["caller", "callee"] as const;
  return (
    <Card>
      <div className="px-4 pt-3 pb-3.5">
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5">
          <h1 className="text-lg font-semibold font-mono tracking-tight">
            {scrub(c.caller_n)} <span className="text-ink-3 font-sans">→</span> {scrub(c.callee_n)}
          </h1>
          <CallStateBadge state={c.state} />
          {c.final_status > 0 && (
            <span className="flex items-center gap-1.5">
              <StatusChip status={c.final_status} reason={c.final_reason} />
              <span className="text-ink-2">{c.final_reason}</span>
            </span>
          )}
          {c.findings.map((f) => <FindingBadge code={f} key={f} />)}
        </div>
        <div className="text-ink-2 mt-1">
          {fmtDateTime(c.start_ts, tz)} {tz} · {c.direction}
          {c.trunk ? ` via ${c.trunk}` : ""} · {c.end_cause ? `${c.end_cause.replaceAll("_", " ")}` : "in progress"}
          {c.hangup_side !== "unknown" && c.end_cause ? `, ${c.hangup_side} side` : ""}
        </div>
        <div className="grid grid-cols-3 md:grid-cols-6 xl:grid-cols-9 gap-x-4 gap-y-2.5 mt-3">
          <KV label="PDD">{fmtDuration(c.pdd_ms)}</KV>
          <KV label="Setup">{fmtDuration(c.setup_ms)}</KV>
          <KV label="Talk">{c.answer_ts ? fmtDuration(c.talk_ms) : "–"}</KV>
          <KV label="Egress status">{c.egress_status || "–"}{c.q850_cause > 0 ? ` · Q.850 ${c.q850_cause}` : ""}</KV>
          <KV label="Worst MOS"><MosCell mos={c.min_mos} /></KV>
          <KV label="Codecs">{c.codecs.join(", ") || "–"}</KV>
          <KV label="Legs">{c.b_call_id ? 2 : 1}</KV>
          <KV label="Nodes">{[c.node, c.pbx].filter(Boolean).join(", ")}</KV>
          <KV label="Capture IDs" mono>{c.capture_ids.join(", ")}</KV>
          <KV label="Messages">{c.msg_count}</KV>
        </div>
        <div className="grid md:grid-cols-2 gap-x-4 gap-y-1 mt-3 text-xs text-ink-2">
          {crit.map((p) => (
            <div key={p} className="truncate">
              <span className="text-ink-3 capitalize">{p}: </span>
              <span className="font-mono">{scrub(c[p])}</span>, {p === "caller" ? c.ua_caller : c.ua_callee}, <span className="font-mono">{p === "caller" ? c.caller_ip : c.callee_ip}</span>
            </div>
          ))}
          <div className="truncate"><span className="text-ink-3">A-leg: </span><span className="font-mono">{c.root_call_id}</span><Copyable value={c.root_call_id} /></div>
          {c.b_call_id && <div className="truncate"><span className="text-ink-3">B-leg: </span><span className="font-mono">{c.b_call_id}</span><Copyable value={c.b_call_id} /></div>}
        </div>
      </div>
    </Card>
  );
}

function FlowTab({ flow, selected, onSelect, tz, scrub, call }: { flow: Flow; selected: number | null; onSelect: (i: number | null) => void; tz: string; scrub: (s: string) => string; call: Call }) {
  const [hideProv, setHideProv] = useState(false);
  const [hideRetx, setHideRetx] = useState(false);
  const [legs, setLegs] = useState({ A: true, B: true });
  const [timeMode, setTimeMode] = useState<"absolute" | "delta">("absolute");
  const svg = useRef<SVGSVGElement>(null);

  const visible = useMemo(() => flow.messages.filter((m) => legs[m.leg] && !(hideProv && m.class === "provisional") && !(hideRetx && m.retx > 0)), [flow, hideProv, hideRetx, legs]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!["ArrowDown", "ArrowUp"].includes(e.key) || visible.length === 0) return;
      const tag = (e.target as HTMLElement).tagName;
      if (tag === "INPUT" || tag === "SELECT") return;
      e.preventDefault();
      const i = visible.findIndex((m) => m.idx === selected);
      const next = e.key === "ArrowDown" ? Math.min(i + 1, visible.length - 1) : Math.max(i - 1, 0);
      onSelect(visible[next].idx);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [visible, selected, onSelect]);

  const exportSvg = () => {
    if (svg.current) downloadBlob(`flow-${call.call_uid.slice(0, 8)}.svg`, new XMLSerializer().serializeToString(svg.current), "image/svg+xml");
  };
  const hasRetx = flow.messages.some((m) => m.retx > 0);
  const skew = call.findings.includes("CLOCK_SKEW");

  return (
    <div>
      {skew && (
        <div className="mb-2 px-2.5 py-1.5 rounded bg-info-wash text-info text-xs">
          Capture times disagree between agents on this call, so message order may be slightly off.
        </div>
      )}
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1.5 mb-2">
        <Toggle checked={legs.A} onChange={(v) => setLegs((l) => ({ ...l, A: v }))}><i className="inline-block w-3 h-0.5 align-middle" style={{ background: "var(--leg-a)" }} /> A leg</Toggle>
        {call.b_call_id && <Toggle checked={legs.B} onChange={(v) => setLegs((l) => ({ ...l, B: v }))}><i className="inline-block w-3 h-0.5 align-middle" style={{ background: "var(--leg-b)" }} /> B leg</Toggle>}
        <Toggle checked={hideProv} onChange={setHideProv}>Hide provisional</Toggle>
        {hasRetx && <Toggle checked={hideRetx} onChange={setHideRetx}>Hide retransmissions</Toggle>}
        <div className="flex-1" />
        <span className="text-ink-3 text-xs tnum">{visible.length} of {flow.messages.length} messages, ↑↓ to step</span>
        <Button onClick={() => setTimeMode(timeMode === "absolute" ? "delta" : "absolute")}>{timeMode === "absolute" ? "Show deltas" : "Show times"}</Button>
        <Button onClick={exportSvg}><Download size={13} />SVG</Button>
      </div>
      <Ladder columns={flow.columns} messages={visible} selected={selected} onSelect={onSelect} tz={tz} timeMode={timeMode} svgRef={svg} scrub={scrub} />
    </div>
  );
}

function Drawer({ m, flow, tz, onClose, scrub }: { m: Msg; flow: Flow; tz: string; onClose: () => void; scrub: (s: string) => string }) {
  const [tab, setTab] = useState("headers");
  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, [onClose]);

  const tabs = [
    { id: "headers", label: "Headers" },
    { id: "raw", label: "Raw" },
    ...(m.sdp ? [{ id: "sdp", label: "SDP" }] : []),
    ...(m.hop_of !== null ? [{ id: "diff", label: `Hop diff (${m.diff.length})` }] : []),
  ];
  const active = tabs.some((t) => t.id === tab) ? tab : "headers";
  return (
    <aside className="fixed top-0 right-0 bottom-0 w-[520px] max-w-[92vw] z-30 bg-raised border-l border-line-2 shadow-xl flex flex-col" aria-label="Message detail">
      <header className="px-4 pt-3 pb-2 border-b border-line">
        <div className="flex items-center justify-between gap-3">
          <h2 className="font-semibold font-mono truncate">{m.label}</h2>
          <button type="button" onClick={onClose} aria-label="Close message detail" className="p-1 rounded hover:bg-sunken"><X size={16} /></button>
        </div>
        <div className="text-xs text-ink-2 mt-0.5">
          {scrub(flow.columns[m.src].name)} → {scrub(flow.columns[m.dst].name)} · leg {m.leg} · CSeq {m.cseq} {m.cseq_method}
        </div>
        <div className="text-xs text-ink-3 font-mono mt-0.5">{m.src_addr} → {m.dst_addr} {m.transport.toUpperCase()} · {m.size} bytes</div>
        <div className="flex flex-wrap gap-x-3 mt-1 text-xs text-ink-2">
          {m.captures.map((c) => <span key={c.point}>{c.point} (capture {c.capture_id}): <span className="font-mono">{fmtMicros(c.ts, tz)}</span></span>)}
        </div>
        {m.q850 > 0 && <div className="mt-1.5"><Badge tone="warn">Q.850 cause {m.q850}: {qText(m)}</Badge></div>}
      </header>
      <div className="px-3"><Tabs tabs={tabs} active={active} onChange={setTab} /></div>
      <div className="flex-1 overflow-auto p-3">
        {active === "headers" && (
          <table className="w-full text-xs">
            <tbody>
              {m.headers.filter(([k]) => k !== ":start-line").map(([k, v], i) => (
                <tr key={i} className="align-top border-b border-line">
                  <td className={`py-1 pr-3 whitespace-nowrap font-medium ${/^(X-|P-|Reason)/.test(k) ? "text-accent" : "text-ink-2"}`}>{k}</td>
                  <td className="py-1 font-mono break-all">{scrub(v)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        {active === "raw" && (
          <div>
            <Button className="mb-2" onClick={() => navigator.clipboard?.writeText(scrub(m.raw).replace(/\r/g, ""))}><Copy size={13} />Copy</Button>
            <pre className="font-mono text-[11.5px] whitespace-pre-wrap break-all">{scrub(m.raw).replace(/\r/g, "")}</pre>
          </div>
        )}
        {active === "sdp" && m.sdp && (
          <div className="grid grid-cols-2 gap-3">
            <KV label="Connection" mono>{m.sdp.ip}</KV>
            <KV label="Port" mono>{m.sdp.port}</KV>
            <KV label="Direction">{m.sdp.direction}</KV>
            <KV label="SRTP">{m.sdp.srtp ? "SDES (keys masked)" : "No"}</KV>
            <div className="col-span-2"><KV label="Codecs">{m.sdp.codecs.join(", ")}</KV></div>
          </div>
        )}
        {active === "diff" && (
          m.diff.length === 0 ? <Empty>No header changes on this hop.</Empty> : (
            <div className="space-y-2">
              <p className="text-xs text-ink-3">Compared with the previous capture of this message.</p>
              {m.diff.map((d, i) => (
                <div key={i} className="text-xs border border-line rounded p-2">
                  <div className="flex gap-2 items-center mb-1"><Badge tone={d.op === "added" ? "good" : d.op === "removed" ? "crit" : "warn"}>{d.op}</Badge><span className="font-medium">{d.name === ":start-line" ? "Request line" : d.name}</span></div>
                  {d.old && <div className="font-mono break-all text-ink-2">− {scrub(d.old)}</div>}
                  {d.new && <div className="font-mono break-all">+ {scrub(d.new)}</div>}
                </div>
              ))}
            </div>
          )
        )}
      </div>
    </aside>
  );
}

function qText(m: Msg): string {
  return /text="([^"]*)"/.exec(m.headers.find(([k]) => k === "Reason")?.[1] ?? "")?.[1] ?? "";
}

function TimelineTab({ call: c, flow, tz }: { call: Call; flow: Flow; tz: string }) {
  const startUs = usOf(c.start_ts);
  const lastUs = c.end_ts ? usOf(c.end_ts) : Math.max(...flow.messages.map((m) => usOf(m.ts)), startUs + 1000);
  const span = Math.max(lastUs - startUs, 1);
  const pct = (us: number) => `${((us - startUs) / span) * 100}%`;
  const width = (a: number, b: number) => `max(3px, ${((b - a) / span) * 100}%)`;

  const legRow = (leg: "A" | "B") => {
    const ms = flow.messages.filter((m) => m.leg === leg);
    if (!ms.length) return null;
    const first = usOf(ms[0].ts);
    const end = usOf(ms[ms.length - 1].ts);
    const ring = ms.find((m) => m.status === 180);
    const ok = ms.find((m) => m.status === 200 && m.cseq_method === "INVITE");
    const seg: { from: number; to: number; label: string; color: string }[] = [];
    const ringUs = ring ? usOf(ring.ts) : null;
    const okUs = ok ? usOf(ok.ts) : null;
    seg.push({ from: first, to: ringUs ?? okUs ?? end, label: "Setup", color: "var(--line-2)" });
    if (ringUs) seg.push({ from: ringUs, to: okUs ?? end, label: "Ringing", color: "#eda100" });
    if (okUs) seg.push({ from: okUs, to: end, label: "Talking", color: "#1baf7a" });
    return { leg, seg };
  };
  const holds = flow.messages.filter((m) => m.leg === "A" && m.sdp?.direction === "sendonly" && m.src === 0);
  const rows = (["A", "B"] as const).map(legRow).filter(Boolean) as { leg: string; seg: { from: number; to: number; label: string; color: string }[] }[];
  const ticks = Array.from({ length: 6 }, (_, i) => startUs + (span * i) / 5);

  return (
    <div className="pl-14 pr-3">
      <div className="relative h-5 border-b border-line-2 mb-3">
        {ticks.map((t, i) => (
          <span key={i} className="absolute text-[11.5px] text-ink-3 tnum -translate-x-1/2" style={{ left: pct(t) }}>{fmtHMS(t / 1000, tz)}</span>
        ))}
      </div>
      <div className="space-y-3">
        {rows.map((r) => (
          <div key={r.leg} className="relative h-7">
            <span className="absolute -left-12 top-1 text-xs text-ink-2">{r.leg} leg</span>
            {r.seg.map((s) => (
              <div key={s.label} title={`${s.label}: ${fmtDuration((s.to - s.from) / 1000)}`} className="absolute top-0 bottom-0 rounded-sm flex items-center overflow-hidden" style={{ left: pct(s.from), width: width(s.from, s.to), background: s.color }}>
                <span className="px-1.5 text-[11.5px] text-black/80 truncate">{s.label}</span>
              </div>
            ))}
            {r.leg === "A" && holds.map((m) => <div key={m.idx} title="Hold" className="absolute -top-1 -bottom-1 w-0.5 bg-[var(--leg-b)]" style={{ left: pct(usOf(m.ts)) }} />)}
          </div>
        ))}
      </div>
      <div className="flex gap-4 mt-4 text-xs text-ink-2">
        {[["Setup", "var(--line-2)"], ["Ringing", "#eda100"], ["Talking", "#1baf7a"]].map(([l, col]) => <span key={l} className="flex items-center gap-1.5"><i className="w-2.5 h-2.5 rounded-sm inline-block" style={{ background: col }} />{l}</span>)}
        {holds.length > 0 && <span className="flex items-center gap-1.5"><i className="w-0.5 h-3 inline-block bg-[var(--leg-b)]" />Hold</span>}
      </div>
    </div>
  );
}

function MessagesTab({ flow, tz, onSelect, scrub }: { flow: Flow; tz: string; onSelect: (i: number) => void; scrub: (s: string) => string }) {
  let prev = flow.messages.length ? usOf(flow.messages[0].ts) : 0;
  return (
    <div className="overflow-x-auto">
      <table className="data-table text-xs">
        <thead><tr><th>Time</th><th className="num">Delta</th><th>From</th><th>To</th><th>Message</th><th>CSeq</th><th>Leg</th><th>Capture</th><th className="num">Size</th></tr></thead>
        <tbody>
          {flow.messages.map((m) => {
            const us = usOf(m.ts);
            const d = us - prev;
            prev = us;
            return (
              <tr key={m.idx} className="cursor-pointer" onClick={() => onSelect(m.idx)}>
                <td className="font-mono">{fmtMicros(m.ts, tz)}</td>
                <td className="num">{(d / 1000).toFixed(1)} ms</td>
                <td>{scrub(flow.columns[m.src].name)}</td>
                <td>{scrub(flow.columns[m.dst].name)}</td>
                <td className="font-medium">{m.label}{m.retx ? <span className="text-warn font-normal"> retx {m.retx}</span> : null}</td>
                <td>{m.cseq} {m.cseq_method}</td>
                <td>{m.leg}</td>
                <td>{m.captures.map((c) => `${c.point} (${c.capture_id})`).join(", ")}</td>
                <td className="num">{m.size}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

function StreamChart({ title, streams, field, unit, height = 130 }: { title: string; streams: any[]; field: number; unit: string; height?: number }) {
  const { theme, tz } = usePrefs();
  const option = useMemo(() => {
    const dark = theme === "dark";
    const colors = dark ? ["#3987e5", "#d95926"] : ["#2a78d6", "#eb6834"];
    const [muted, grid, axis, raised, ink] = dark ? ["#9a9992", "#2c2c2a", "#383835", "#232322", "#f0efec"] : ["#6b6a65", "#e1e0d9", "#c3c2b7", "#ffffff", "#0b0b0b"];
    return {
      animation: false,
      textStyle: { fontFamily: "'Inter Variable', sans-serif" },
      grid: { left: 40, right: 8, top: 6, bottom: 18 },
      xAxis: { type: "time", axisLine: { lineStyle: { color: axis } }, axisTick: { show: false }, splitLine: { show: false }, axisLabel: { color: muted, fontSize: 11, hideOverlap: true, formatter: (v: number) => fmtHMS(v, tz) } },
      yAxis: { type: "value", scale: field === 1, splitNumber: 3, axisLine: { show: false }, axisTick: { show: false }, splitLine: { lineStyle: { color: grid } }, axisLabel: { color: muted, fontSize: 11 } },
      tooltip: { trigger: "axis", confine: true, backgroundColor: raised, borderColor: axis, textStyle: { color: ink, fontSize: 12 }, formatter: (p: any[]) => `${fmtHMS(p[0].value[0], tz)}<br/>${p.map((x) => `${x.seriesName}: <b>${x.value[1].toFixed(field === 1 ? 2 : 1)}${unit}</b>`).join("<br/>")}` },
      series: streams.filter((s) => s.points.length).map((s, i) => ({ name: s.label, type: "line", showSymbol: false, lineStyle: { width: 2, color: colors[i] }, itemStyle: { color: colors[i] }, data: s.points.map((p: number[]) => [p[0], Number(p[field].toFixed(3))]) })),
    };
  }, [streams, field, unit, theme, tz]);
  const { ref } = useEChart(option, "call-media");
  return (
    <div>
      <div className="text-xs font-medium text-ink-2 pl-1">{title}</div>
      <div ref={ref} style={{ height }} />
    </div>
  );
}

function MediaTab({ uid, tz, sub }: { uid: string; tz: string; sub: string }) {
  const { data, error } = useQuery({ queryKey: ["media", uid, sub], queryFn: () => get(`/calls/${uid}/media${sub}`) });
  void tz;
  if (error) return <ErrorNote error={error} />;
  if (!data) return <Loading />;
  const has = data.streams.some((s: any) => s.points.length);
  return (
    <div className="space-y-4">
      <div className="overflow-x-auto">
        <table className="data-table">
          <thead><tr><th>Stream</th><th>Reporter</th><th>Codec</th><th className="num">Avg MOS</th><th className="num">Worst MOS</th><th className="num">Loss</th><th className="num">Max jitter</th><th className="num">Avg RTT</th><th>Verdict</th></tr></thead>
          <tbody>
            {data.streams.length === 0 && <tr><td colSpan={9} className="text-ink-3">No media: the call was not answered.</td></tr>}
            {data.streams.map((s: any) => (
              <tr key={s.id}>
                <td className="font-medium">{s.label}</td><td>{s.reporter} ({s.source_type})</td><td>{s.codec}</td>
                <td className="num">{s.avg_mos?.toFixed(2) ?? "–"}</td><td className="num"><MosCell mos={s.min_mos} /></td>
                <td className="num">{s.loss_pct !== null ? `${s.loss_pct.toFixed(2)} %` : "–"}</td><td className="num">{s.max_jitter_ms !== null ? `${s.max_jitter_ms.toFixed(1)} ms` : "–"}</td><td className="num">{s.avg_rtt_ms !== null ? `${s.avg_rtt_ms.toFixed(0)} ms` : "–"}</td>
                <td><Badge tone={({ good: "good", fair: "info", poor: "warn", bad: "crit", "no media": "crit" } as any)[s.verdict] ?? "neutral"}>{s.verdict}</Badge></td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {has && (
        <div className="grid md:grid-cols-2 gap-3">
          <StreamChart title="MOS" streams={data.streams} field={1} unit="" />
          <StreamChart title="Packet loss (%)" streams={data.streams} field={2} unit=" %" />
          <StreamChart title="Jitter (ms)" streams={data.streams} field={3} unit=" ms" />
          <StreamChart title="RTT (ms)" streams={data.streams} field={4} unit=" ms" />
        </div>
      )}
      {has && (
        <div className="flex gap-4 text-xs text-ink-2">
          <span className="flex items-center gap-1.5"><i className="w-2.5 h-2.5 rounded-sm inline-block" style={{ background: "#2a78d6" }} />Caller → callee</span>
          <span className="flex items-center gap-1.5"><i className="w-2.5 h-2.5 rounded-sm inline-block" style={{ background: "#eb6834" }} />Callee → caller</span>
          <span className="text-ink-3">Media anchored on {data.media_node}</span>
        </div>
      )}
      <div>
        <h3 className="font-semibold mb-1.5">SDP offer and answer</h3>
        <table className="data-table text-xs">
          <thead><tr><th>Leg</th><th>Message</th><th>From</th><th>To</th><th>Media address</th><th>Codecs</th><th>Direction</th><th>SRTP</th></tr></thead>
          <tbody>
            {data.sdp.map((s: any) => (
              <tr key={s.idx}><td>{s.leg}</td><td>{s.label}</td><td>{s.from}</td><td>{s.to}</td><td className="font-mono">{s.sdp.ip}:{s.sdp.port}</td><td>{s.sdp.codecs.join(", ")}</td><td>{s.sdp.direction}</td><td>{s.sdp.srtp ? "yes" : "no"}</td></tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function LogsTab({ uid, tz, sub }: { uid: string; tz: string; sub: string }) {
  const { data, error } = useQuery({ queryKey: ["logs", uid, sub], queryFn: () => get(`/calls/${uid}/logs${sub}`) });
  if (error) return <ErrorNote error={error} />;
  if (!data) return <Loading />;
  if (data.length === 0) return <Empty>No log lines linked to this call.</Empty>;
  const tone = (s: string) => (s === "ERROR" ? "crit" : s === "WARNING" ? "warn" : "neutral") as "crit" | "warn" | "neutral";
  return (
    <table className="data-table text-xs">
      <thead><tr><th>Time</th><th>Node</th><th>Level</th><th>Message</th></tr></thead>
      <tbody>
        {data.map((l: any, i: number) => (
          <tr key={i}><td className="font-mono">{fmtMicros(l.ts, tz)}</td><td>{l.node}</td><td><Badge tone={tone(l.severity)}>{l.severity}</Badge></td><td className="font-mono whitespace-normal">{l.message}</td></tr>
        ))}
      </tbody>
    </table>
  );
}

function DiagnosticsTab({ uid, sub, onEvidence }: { uid: string; sub: string; onEvidence: (i: number) => void }) {
  const { data, error } = useQuery({ queryKey: ["findings", uid, sub], queryFn: () => get(`/calls/${uid}/findings${sub}`) });
  if (error) return <ErrorNote error={error} />;
  if (!data) return <Loading />;
  if (data.length === 0) return <Empty>No findings on this call.</Empty>;
  return (
    <div className="space-y-3">
      {data.map((f: any) => (
        <div key={f.code} className="border border-line rounded-md p-3">
          <div className="flex items-center gap-2">
            <SeverityBadge severity={f.severity} />
            <h3 className="font-semibold">{f.title}</h3>
            <code className="text-ink-3 text-xs font-mono">{f.code}</code>
          </div>
          <p className="mt-1.5">{f.explanation}</p>
          {f.measured.length > 0 && <p className="text-ink-2 mt-1 tnum">{f.measured.join(" · ")}</p>}
          <p className="mt-1.5 text-ink-2"><span className="font-medium text-ink">Next step: </span>{f.next_step}</p>
          {f.evidence.length > 0 && (
            <div className="mt-2 flex items-center gap-1.5 flex-wrap">
              <span className="text-ink-3 text-xs">Evidence:</span>
              {f.evidence.map((i: number) => <Button key={i} onClick={() => onEvidence(i)}>Message {i + 1}</Button>)}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}

function RegistrationsTab({ uid, tz, sub }: { uid: string; tz: string; sub: string }) {
  const { data, error } = useQuery({ queryKey: ["call-regs", uid, sub], queryFn: () => get(`/calls/${uid}/registrations${sub}`) });
  if (error) return <ErrorNote error={error} />;
  if (!data) return <Loading />;
  if (data.length === 0) return <Empty>Neither party is a local subscriber.</Empty>;
  return (
    <div className="grid lg:grid-cols-2 gap-4">
      {data.map((p: any) => (
        <div key={p.role}>
          <div className="flex items-center gap-2 mb-1.5">
            <h3 className="font-semibold">{p.role}</h3>
            <Link to="/subscribers/$number" params={{ number: p.number }} className="font-mono hover:underline">{p.number}</Link>
            <RegStatus status={p.state.status} />
          </div>
          <p className="text-ink-2 text-xs mb-1.5">State at call time. Contact {p.state.src_ip}, {p.state.ua_vendor} {p.state.ua_model}.</p>
          <table className="data-table text-xs">
            <thead><tr><th>Time</th><th>Result</th><th>Code</th><th>Source</th></tr></thead>
            <tbody>
              {p.events.slice(-14).map((e: any, i: number) => (
                <tr key={i}><td>{fmtHMS(e.ts, tz)}</td><td>{e.result}</td><td>{e.code}</td><td className="font-mono">{e.src_ip}:{e.src_port}</td></tr>
              ))}
            </tbody>
          </table>
        </div>
      ))}
    </div>
  );
}

function RelatedTab({ uid, tz, sub }: { uid: string; tz: string; sub: string }) {
  const { data, error } = useQuery({ queryKey: ["related", uid, sub], queryFn: () => get(`/calls/${uid}/related${sub}`) });
  if (error) return <ErrorNote error={error} />;
  if (!data) return <Loading />;
  if (data.length === 0) return <Empty>No other calls between these parties within one hour.</Empty>;
  return (
    <table className="data-table">
      <thead><tr><th>Start</th><th>Relation</th><th>Status</th><th className="num">Talk</th></tr></thead>
      <tbody>
        {data.map((r: any) => (
          <tr key={r.call.call_uid}>
            <td><Link to="/calls/$uid" params={{ uid: r.call.call_uid }} className="hover:underline">{fmtDateTime(r.call.start_ts, tz).slice(5)}</Link></td>
            <td>{r.relation}</td><td><StatusChip status={r.call.final_status} /></td><td className="num">{fmtDuration(r.call.talk_ms)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function ExportTab({ uid, call, flow, sub }: { uid: string; call: Call; flow: Flow; sub: string }) {
  const { can } = usePrefs();
  if (!can("export_single") || !can("view_payload")) return <Empty>Your account can't export calls.</Empty>;
  const link = (format: string, label: string, note: string, Icon: typeof Download) => (
    <a href={`/api/v1/calls/${uid}/export/${format}${sub}`} className="border border-line rounded-md p-3 hover:border-accent block" download>
      <div className="flex items-center gap-2 font-medium"><Icon size={14} />{label}</div>
      <p className="text-ink-2 text-xs mt-1">{note}</p>
    </a>
  );
  return (
    <div className="grid md:grid-cols-3 gap-3 max-w-[820px]">
      {link("pcap", "PCAP", `${flow.messages.length} messages across ${call.b_call_id ? "both legs" : "one leg"}, as UDP frames with original addresses and times.`, Download)}
      {link("text", "Text", "Flow in sngrep style, then every full message.", FileText)}
      {link("json", "JSON", "Call record, messages, QoS streams and findings.", FileText)}
      <div className="md:col-span-3 text-ink-3 text-xs">Expiring share links aren't in this version.</div>
    </div>
  );
}
