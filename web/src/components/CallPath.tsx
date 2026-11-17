import { ArrowRight, CircleAlert, Clock, Server } from "lucide-react";
import { useMemo } from "react";
import type { Call, Flow, Msg } from "../lib/api";
import { fmtDuration, usOf } from "../lib/format";
import { msgColor } from "./Ladder";
import { Empty } from "./ui";

/** One device the call passed through, in the order the INVITE reached it. */
interface Hop {
  col: number;
  name: string;
  ip: string;
  software: string;
  capture: boolean;
  /** When the INVITE arrived here and when it was sent on, in microseconds. */
  inUs: number | null;
  outUs: number | null;
}

/** The INVITE transaction between two neighbouring devices. */
interface Link {
  from: number;
  to: number;
  callId: string;
  inviteUs: number;
  firstUs: number | null;
  ringUs: number | null;
  final: Msg | null;
  finalUs: number | null;
  vias: string[];
  /** Via hosts that are not in the ladder: proxies that forwarded the INVITE without being captured. */
  unseen: string[];
}

const header = (m: Msg, ...names: string[]) => m.headers.filter(([k]) => names.includes(k.toLowerCase())).map(([, v]) => v);
const viaHost = (v: string) => v.replace(/^SIP\/2\.0\/\w+\s+/i, "").split(/[;:\s]/)[0];

export function buildPath(flow: Flow) {
  const invites = flow.messages.filter((m) => m.method === "INVITE" && m.retx === 0 && !header(m, "to", "t").some((t) => /;\s*tag=/i.test(t)));
  const byKey = new Map<string, Link>();
  for (const m of invites) {
    const key = `${m.src}>${m.dst}>${m.call_id}`;
    if (byKey.has(key)) continue;
    const vias = header(m, "via", "v").flatMap((v) => v.split(",")).map((v) => v.trim()).filter(Boolean);
    const known = new Set(flow.columns.map((c) => c.ip));
    byKey.set(key, { from: m.src, to: m.dst, callId: m.call_id, inviteUs: usOf(m.ts), firstUs: null, ringUs: null, final: null, finalUs: null, vias, unseen: vias.map(viaHost).filter((h) => h && !known.has(h)) });
  }
  const links = [...byKey.values()].sort((a, b) => a.inviteUs - b.inviteUs);
  for (const l of links) {
    const replies = flow.messages.filter((m) => m.status > 0 && m.cseq_method === "INVITE" && m.call_id === l.callId && m.src === l.to && m.dst === l.from && usOf(m.ts) >= l.inviteUs);
    const first = replies[0];
    if (first) l.firstUs = usOf(first.ts);
    const ring = replies.find((m) => m.status === 180 || m.status === 183);
    if (ring) l.ringUs = usOf(ring.ts);
    const finals = replies.filter((m) => m.status >= 200);
    // A challenge only counts when nothing followed it.
    const real = finals.filter((m) => m.status !== 401 && m.status !== 407);
    const final = real.find((m) => m.status < 300) ?? real[real.length - 1] ?? finals[finals.length - 1] ?? null;
    if (final) {
      l.final = final;
      l.finalUs = usOf(final.ts);
    }
  }

  const order: number[] = [];
  for (const l of links) {
    if (!order.includes(l.from)) order.push(l.from);
    if (!order.includes(l.to)) order.push(l.to);
  }
  const software = (col: number) => {
    for (const m of flow.messages) {
      if (m.src !== col) continue;
      const ua = header(m, "user-agent", "server")[0];
      if (ua) return ua;
    }
    return "";
  };
  const hops: Hop[] = order.map((col) => {
    const c = flow.columns[col];
    const inUs = links.find((l) => l.to === col)?.inviteUs ?? null;
    const outUs = links.find((l) => l.from === col && (inUs === null || l.inviteUs >= inUs))?.inviteUs ?? null;
    return { col, name: c.name, ip: c.ip, software: software(col), capture: c.role === "capture", inUs, outUs };
  });

  // The failure starts at the deepest hop that answered with an error.
  const failed = links.filter((l) => l.final && l.final.status >= 300);
  const origin = failed.length ? failed.reduce((a, b) => (order.indexOf(b.to) > order.indexOf(a.to) ? b : a)) : null;
  return { hops, links, origin };
}

function statusTone(status: number | undefined) {
  if (!status) return "var(--ink-3)";
  if (status >= 500) return "var(--ladder-crit)";
  if (status >= 400) return "var(--ladder-warn)";
  if (status >= 300) return "var(--ladder-redirect)";
  return "var(--ladder-ok)";
}

const ms = (us: number | null | undefined) => (us === null || us === undefined ? "–" : fmtDuration(Math.round(us / 1000)));

export function CallPath({ flow, call, scrub = (s) => s }: { flow: Flow; call: Call; scrub?: (s: string) => string }) {
  const { hops, links, origin } = useMemo(() => buildPath(flow), [flow]);
  if (links.length === 0) return <Empty>No INVITE was captured for this call, so its path is unknown.</Empty>;
  const t0 = links[0].inviteUs;
  const end = Math.max(...links.map((l) => l.finalUs ?? l.firstUs ?? l.inviteUs), t0 + 1);
  const span = end - t0;
  const pct = (us: number) => `${((us - t0) / span) * 100}%`;
  const linkTo = (col: number) => links.find((l) => l.to === col);
  const hopName = (col: number) => {
    const h = hops.find((x) => x.col === col);
    return h ? scrub(h.software ? `${h.name} (${h.software.split(/[/ ]/)[0]})` : h.name) : "?";
  };

  // Where each upstream hop changed the answer it got from downstream.
  const translated = links
    .map((l, i) => {
      const down = links.slice(i + 1).find((d) => d.from === l.to && d.final);
      return down && l.final && down.final && down.final.status !== l.final.status ? { at: l.to, got: down.final, sent: l.final } : null;
    })
    .filter(Boolean) as { at: number; got: Msg; sent: Msg }[];

  const summary = (() => {
    const total = hops.length;
    const first = links[0];
    const last = links[links.length - 1];
    const parts = [`The INVITE crossed ${total} device${total === 1 ? "" : "s"} and reached the last one ${ms(last.inviteUs - first.inviteUs)} after it left the first.`];
    if (origin?.final) {
      parts.push(`${hopName(origin.to)} answered ${origin.final.status} ${origin.final.reason} after ${ms((origin.finalUs ?? 0) - origin.inviteUs)}.`);
    } else if (last.final && last.final.status < 300) {
      parts.push(`${hopName(last.to)} answered after ${ms((last.finalUs ?? 0) - last.inviteUs)}.`);
    } else if (!last.final) {
      parts.push(`${hopName(last.to)} has not sent a final answer.`);
    }
    for (const t of translated) parts.push(`${hopName(t.at)} received ${t.got.status} and passed it on as ${t.sent.status}.`);
    const slow = hops.filter((h) => h.inUs !== null && h.outUs !== null && h.outUs - h.inUs > 1_000_000);
    for (const h of slow) parts.push(`${hopName(h.col)} held the INVITE for ${ms((h.outUs ?? 0) - (h.inUs ?? 0))} before forwarding it.`);
    return parts;
  })();

  return (
    <div className="space-y-4">
      <div className={`rounded-lg border px-3 py-2.5 ${origin ? "border-crit/40 bg-crit-wash" : "border-good/40 bg-good-wash"}`}>
        <div className="flex items-start gap-2">
          {origin ? <CircleAlert size={16} className="text-crit mt-0.5 shrink-0" aria-hidden /> : <Clock size={16} className="text-good mt-0.5 shrink-0" aria-hidden />}
          <div className="space-y-0.5">
            {summary.map((p, i) => (
              <p key={i} className={i === 0 ? "font-medium" : "text-ink-2"}>{p}</p>
            ))}
          </div>
        </div>
      </div>

      <div className="overflow-x-auto pb-2">
        <ol className="flex items-stretch min-w-max gap-0" aria-label="Call path">
          {hops.map((h, i) => {
            const into = linkTo(h.col);
            const isOrigin = origin?.to === h.col;
            const held = h.inUs !== null && h.outUs !== null ? h.outUs - h.inUs : null;
            const role = i === 0 ? "Caller" : i === hops.length - 1 ? (call.trunk ? "Callee · trunk" : "Callee") : h.capture ? "Capture point" : "Proxy";
            return (
              <li key={h.col} className="flex items-stretch path-step" style={{ animationDelay: `${i * 90}ms` }}>
                {into && (
                  <div className="flex flex-col justify-center items-center px-2 w-[190px]">
                    <div className="text-2xs text-ink-3 tnum">+{ms(into.inviteUs - t0)}</div>
                    <div className="relative w-full h-6 flex items-center">
                      <div className="w-full h-[3px] rounded-full path-flow" style={{ background: statusTone(into.final?.status) }} />
                      <ArrowRight size={16} className="absolute -right-1.5" style={{ color: statusTone(into.final?.status) }} aria-hidden />
                    </div>
                    <div className="text-xs text-center space-y-0.5">
                      {into.final ? (
                        <div className="font-semibold" style={{ color: statusTone(into.final.status) }}>
                          {into.final.status} {into.final.reason}
                          {into.final.q850 > 0 && <span className="font-normal text-ink-3"> · Q.850 {into.final.q850}</span>}
                        </div>
                      ) : (
                        <div className="text-ink-3">no final answer</div>
                      )}
                      <div className="text-ink-3 tnum">
                        {into.firstUs !== null && <>first reply {ms(into.firstUs - into.inviteUs)}</>}
                        {into.ringUs !== null && <> · ringing {ms(into.ringUs - into.inviteUs)}</>}
                      </div>
                      {into.unseen.length > 0 && <div className="text-warn" title={into.vias.join("\n")}>{into.unseen.length} proxy not captured: {into.unseen.join(", ")}</div>}
                    </div>
                  </div>
                )}
                <div className={`w-[200px] rounded-lg border bg-raised px-3 py-2.5 ${isOrigin ? "border-crit ring-2 ring-crit/25" : h.capture ? "border-accent/50" : "border-line-2"}`}>
                  <div className="flex items-center gap-1.5 text-2xs uppercase tracking-[0.05em] text-ink-3 font-semibold">
                    <Server size={12} aria-hidden />
                    {role}
                    {isOrigin && <span className="ml-auto text-crit normal-case tracking-normal">failure starts here</span>}
                  </div>
                  <div className="font-semibold truncate mt-1">{scrub(h.name)}</div>
                  <div className="font-mono text-xs text-ink-2 truncate">{h.ip}</div>
                  {h.software && <div className="text-xs text-ink-3 truncate" title={h.software}>{h.software}</div>}
                  {held !== null && i > 0 && (
                    <div className={`mt-1.5 text-xs tnum ${held > 1_000_000 ? "text-warn font-medium" : "text-ink-2"}`}>forwarded after {ms(held)}</div>
                  )}
                </div>
              </li>
            );
          })}
        </ol>
      </div>

      <div>
        <h3 className="font-semibold mb-1.5">Hop timing</h3>
        <p className="text-ink-3 text-xs mb-2">Each bar runs from the INVITE reaching the device to its final answer. Ticks mark the first reply and ringing.</p>
        <div className="space-y-1.5">
          {links.map((l, i) => {
            const stop = l.finalUs ?? l.firstUs ?? l.inviteUs;
            const color = l.final ? msgColor(l.final) : "var(--ink-3)";
            return (
              <div key={`${l.from}-${l.to}-${l.callId}`} className="grid grid-cols-[minmax(140px,240px)_1fr_90px] items-center gap-3 text-xs">
                <div className="truncate" title={`${flow.columns[l.from].ip} → ${flow.columns[l.to].ip}`}>
                  <span className="text-ink-3">{i + 1}.</span> {scrub(flow.columns[l.from].name)} → <span className="font-medium">{scrub(flow.columns[l.to].name)}</span>
                </div>
                <div className="relative h-5 rounded bg-sunken">
                  <div className="absolute top-1 bottom-1 rounded-sm path-bar" style={{ left: pct(l.inviteUs), width: `max(3px, calc(${pct(stop)} - ${pct(l.inviteUs)}))`, background: `color-mix(in srgb, ${color} 55%, transparent)`, borderLeft: `2px solid ${color}`, animationDelay: `${i * 80}ms` }} />
                  {l.firstUs !== null && <div className="absolute top-0 bottom-0 w-px bg-ink-3" style={{ left: pct(l.firstUs) }} title={`first reply +${ms(l.firstUs - l.inviteUs)}`} />}
                  {l.ringUs !== null && <div className="absolute top-0 bottom-0 w-0.5" style={{ left: pct(l.ringUs), background: "var(--phase-ring)" }} title={`ringing +${ms(l.ringUs - l.inviteUs)}`} />}
                </div>
                <div className="tnum text-right" style={{ color }}>{l.final ? `${l.final.status} · ${ms(stop - l.inviteUs)}` : "pending"}</div>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
