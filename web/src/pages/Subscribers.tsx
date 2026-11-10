import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { useEffect, useState } from "react";
import { Badge, Button, Card, Empty, FindingBadge, KV, Loading, MosCell, PageHeader, RegStatus, SeverityBadge, StatusChip } from "../components/ui";
import { get, qs } from "../lib/api";
import type { Call, RegState } from "../lib/api";
import { usePrefs } from "../lib/context";
import { fmtDateTime, fmtDuration, fmtHM, fmtHMS, toMs } from "../lib/format";

export function SubscribersPage() {
  const navigate = useNavigate();
  const [number, setNumber] = useState("");
  const { can } = usePrefs();
  const browse = can("search_all");
  const [status, setStatus] = useState("");
  const [q, setQ] = useState("");
  const list = useQuery({ queryKey: ["reg-now", status, q], queryFn: () => get(`/registrations/now${qs({ status, q, limit: 100 })}`), placeholderData: keepPreviousData, enabled: browse });

  return (
    <div className="space-y-3">
      <PageHeader title="Subscribers" description="Enter a number in any format to see its registration state, devices, calls and quality." />
      <form
        className="flex gap-2 max-w-[520px]"
        onSubmit={(e) => {
          e.preventDefault();
          if (number.trim()) navigate({ to: "/subscribers/$number", params: { number: number.trim() } } as any);
        }}
      >
        <input value={number} onChange={(e) => setNumber(e.target.value)} placeholder="07700012345, +9647700012345 or 009647700012345" aria-label="Subscriber number" className="flex-1 h-8 rounded-md bg-raised border border-line-2 px-2.5 font-mono text-[12.5px]" />
        <Button variant="primary" type="submit" className="!h-8">Open</Button>
      </form>

      {browse && list.data && (
        <Card
          title={`${list.data.total.toLocaleString()} subscribers`}
          right={
            <div className="flex items-center gap-2">
              {list.data.facets.status.map((f: any) => (
                <button type="button" key={f.value} onClick={() => setStatus(status === f.value ? "" : f.value)} className={`h-6 px-1.5 rounded ${status === f.value ? "bg-accent-wash text-ink" : "hover:bg-sunken"}`}>
                  {f.value.replace("_", " ")} <span className="tnum">{f.count.toLocaleString()}</span>
                </button>
              ))}
              <input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Filter number, IP or model" aria-label="Filter subscribers" className="h-6 rounded bg-sunken border border-line px-2 text-xs w-[180px]" />
            </div>
          }
        >
          <div className="overflow-auto max-h-[420px]">
            <table className="data-table">
              <thead><tr><th>Number</th><th>State</th><th>Group</th><th>Handset</th><th>Source IP</th><th>NAT</th><th>Last failure</th></tr></thead>
              <tbody>
                {list.data.rows.map((r: RegState) => (
                  <tr key={r.number} className="cursor-pointer" onClick={() => navigate({ to: "/subscribers/$number", params: { number: r.number } } as any)}>
                    <td className="font-mono text-xs"><Link to="/subscribers/$number" params={{ number: r.number }} onClick={(e) => e.stopPropagation()} className="hover:underline">{r.number}</Link></td>
                    <td><RegStatus status={r.status} /></td>
                    <td className="text-ink-2">{r.group}</td>
                    <td>{r.ua_vendor} {r.ua_model}</td>
                    <td className="font-mono text-xs">{r.src_ip}</td>
                    <td className="text-ink-2">{r.nat ? "Yes" : "No"}</td>
                    <td className="text-ink-2">{r.last_failure?.code ?? "–"}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </Card>
      )}
    </div>
  );
}

function Countdown({ to }: { to: string }) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, []);
  const left = Math.max(0, toMs(to) - now);
  return <span className="tnum">{Math.floor(left / 60000)}:{String(Math.floor((left % 60000) / 1000)).padStart(2, "0")}</span>;
}

const STATE_COLOR: Record<string, string> = { registered: "#0ca30c", failing: "#d03b3b", unregistered: "var(--line-2)" };

function StateTimeline({ segments, from, to, tz }: { segments: any[]; from: number; to: number; tz: string }) {
  const span = to - from;
  const ticks = Array.from({ length: 7 }, (_, i) => from + (span * i) / 6);
  return (
    <div className="px-1">
      <div className="relative h-6 rounded-sm overflow-hidden bg-sunken">
        {segments.map((s, i) => (
          <div
            key={i}
            title={`${s.state}: ${fmtHM(s.from, tz)} – ${fmtHM(s.to, tz)}`}
            className="absolute top-0 bottom-0"
            style={{ left: `${((toMs(s.from) - from) / span) * 100}%`, width: `max(1px, ${((toMs(s.to) - toMs(s.from)) / span) * 100}%)`, background: STATE_COLOR[s.state] }}
          />
        ))}
      </div>
      <div className="relative h-4 mt-1">
        {ticks.map((t, i) => (
          <span key={i} className={`absolute text-[11.5px] text-ink-3 tnum ${i === 0 ? "" : i === 6 ? "-translate-x-full" : "-translate-x-1/2"}`} style={{ left: `${(i / 6) * 100}%` }}>{fmtHM(t, tz)}</span>
        ))}
      </div>
      <div className="flex gap-4 mt-1 text-xs text-ink-2">
        {Object.entries(STATE_COLOR).map(([k, c]) => <span key={k} className="flex items-center gap-1.5"><i className="w-2.5 h-2.5 rounded-sm inline-block" style={{ background: c }} />{k}</span>)}
      </div>
    </div>
  );
}

export function SubscriberPage({ number }: { number: string }) {
  const { tz, refreshMs } = usePrefs();
  const navigate = useNavigate();
  const { data, error } = useQuery({ queryKey: ["subscriber", number], queryFn: () => get(`/subscribers/${encodeURIComponent(number)}`), refetchInterval: refreshMs, placeholderData: keepPreviousData });
  if (error) {
    return (
      <div>
        <PageHeader title="Subscriber" />
        <Card><Empty><div className="font-medium text-ink mb-1">Number not found</div>{(error as Error).message}</Empty></Card>
      </div>
    );
  }
  if (!data) return <Loading />;
  const s: RegState = data.state;
  const other = (c: Call) => (c.caller_n === data.number ? c.callee_n : c.caller_n);
  const headline = s.status === "failing" && s.last_failure
    ? `Not registered. Last attempt ${s.last_failure.code}${s.failing_since ? ` since ${fmtHM(s.failing_since, tz)}` : ""}, ${s.ua_vendor} ${s.ua_model}, IP ${s.src_ip}`
    : s.contact ? `Registered from ${s.contact.src_ip}, ${s.contact.ua_vendor} ${s.contact.ua_model}` : "Not registered";

  return (
    <div className="space-y-3">
      <Card>
        <div className="px-4 py-3">
          <div className="flex items-center gap-3">
            <h1 className="font-mono text-lg font-semibold">{data.number}</h1>
            <RegStatus status={s.status} />
            <span className="text-ink-2">{s.group}</span>
          </div>
          <p className="mt-1">{headline}</p>
          {data.summary.length > 0 && (
            <div className="flex flex-wrap gap-2 mt-2">
              {data.summary.map((f: any, i: number) => (
                <SeverityBadge severity={f.severity} key={i}>{f.text}{f.since ? ` since ${fmtHM(f.since, tz)}` : ""}</SeverityBadge>
              ))}
            </div>
          )}
        </div>
      </Card>

      <div className="grid lg:grid-cols-3 gap-3">
        <Card title="Now" className="lg:col-span-1">
          <div className="px-3.5 pb-3 grid grid-cols-2 gap-x-4 gap-y-2">
            <KV label="Handset">{s.ua_vendor} {s.ua_model}{s.contact ? ` · ${s.contact.ua_firmware}` : ""}</KV>
            <KV label="Public address" mono>{s.contact ? `${s.contact.src_ip}:${s.contact.src_port}` : s.src_ip}</KV>
            <KV label="NAT">{s.nat ? "Behind NAT" : "No"}</KV>
            <KV label="Expires in">{s.contact ? <Countdown to={s.contact.expires_at} /> : "–"}</KV>
            <KV label="Last success">{s.last_success ? fmtHMS(s.last_success, tz) : "None recently"}</KV>
            <KV label="Last failure">{s.last_failure ? `${s.last_failure.code} at ${fmtHMS(s.last_failure.ts, tz)}` : "–"}</KV>
            <KV label="Consecutive failures">{s.consecutive_failures}</KV>
            <KV label="First seen">{fmtDateTime(s.first_seen, tz).slice(0, 10)}</KV>
          </div>
        </Card>
        <Card title="Registration history, 24 h" className="lg:col-span-2" right={<span>{tz}</span>}>
          <div className="px-3 pb-3 pt-1">
            <StateTimeline segments={data.timeline} from={data.from} to={data.to} tz={tz} />
          </div>
        </Card>
      </div>

      <div className="grid lg:grid-cols-3 gap-3">
        <Card title={`Calls, ${data.calls_7d} in 7 days`} className="lg:col-span-2">
          {data.calls.length === 0 ? <Empty>No calls in the last 7 days.</Empty> : (
            <div className="overflow-auto max-h-[340px]">
              <table className="data-table">
                <thead><tr><th>Start</th><th>Dir</th><th>Other party</th><th>Status</th><th className="num">PDD</th><th className="num">Talk</th><th className="num">MOS</th><th>Findings</th></tr></thead>
                <tbody>
                  {data.calls.map((c: Call) => (
                    <tr key={c.call_uid} className="cursor-pointer" onClick={() => navigate({ to: "/calls/$uid", params: { uid: c.call_uid }, search: { subject: data.number } } as any)}>
                      <td><Link to="/calls/$uid" params={{ uid: c.call_uid }} search={{ subject: data.number } as any} onClick={(e) => e.stopPropagation()} className="hover:underline">{fmtDateTime(c.start_ts, tz).slice(5)}</Link></td>
                      <td className="text-ink-2">{c.caller_n === data.number ? "out" : "in"}</td>
                      <td className="font-mono text-xs">{other(c)}</td>
                      <td><StatusChip status={c.final_status} reason={c.final_reason} /></td>
                      <td className="num">{fmtDuration(c.pdd_ms)}</td>
                      <td className="num">{c.talk_ms ? fmtDuration(c.talk_ms) : "–"}</td>
                      <td className="num"><MosCell mos={c.min_mos} /></td>
                      <td className="space-x-1">{c.findings.slice(0, 2).map((f) => <FindingBadge code={f} key={f} />)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </Card>
        <Card title="Quality per day">
          <div className="overflow-auto">
            <table className="data-table text-xs">
              <thead><tr><th>Day</th><th className="num">Calls</th><th className="num">MOS</th><th className="num">Loss</th></tr></thead>
              <tbody>
                {data.daily.map((d: any) => (
                  <tr key={d.day}><td>{fmtDateTime(d.day, tz).slice(5, 10)}</td><td className="num">{d.calls}</td><td className="num"><MosCell mos={d.mos} /></td><td className="num">{d.loss !== null ? `${d.loss.toFixed(2)} %` : "–"}</td></tr>
                ))}
              </tbody>
            </table>
          </div>
        </Card>
      </div>

      <Card title="Registration attempts">
        <div className="overflow-auto max-h-[300px]">
          <table className="data-table text-xs">
            <thead><tr><th>Time</th><th>Result</th><th>Code</th><th>Source</th><th>Contact</th><th>User agent</th><th className="num">RRD</th></tr></thead>
            <tbody>
              {data.events.map((e: any, i: number) => {
                const prev = data.events[i + 1];
                const moved = prev && prev.src_ip !== e.src_ip;
                return (
                  <tr key={i}>
                    <td>{fmtDateTime(e.ts, tz).slice(5)}</td>
                    <td><Badge tone={e.code === 200 ? "good" : "crit"}>{e.result.replace("_", " ")}</Badge></td>
                    <td>{e.code}</td>
                    <td className="font-mono">{e.src_ip}:{e.src_port}{moved && <span className="ml-1.5"><Badge tone="warn">IP changed</Badge></span>}</td>
                    <td className="font-mono">{e.contact}</td>
                    <td>{e.user_agent}</td>
                    <td className="num">{e.rrd_ms !== null ? `${e.rrd_ms} ms` : "–"}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </Card>
    </div>
  );
}
