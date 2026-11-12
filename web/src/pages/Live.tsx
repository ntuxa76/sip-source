import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { Pause, Play, Square, Trash2 } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { Button, Card, Empty, PageHeader, Status, fieldClass } from "../components/ui";
import { Orb } from "../components/Orb";
import { put } from "../lib/api";
import { usePrefs, useSource } from "../lib/context";
import { fmtMicros, usOf } from "../lib/format";

interface LiveEvent {
  ts: string;
  call_id: string;
  label: string;
  class: string;
  from: string;
  to: string;
  retx: number;
  caller: string;
  callee: string;
  state: string;
  node: string;
  ua: string;
  capture_id: number;
  agent_ip: string;
}

interface Dialog {
  id: string;
  caller: string;
  callee: string;
  state: string;
  node: string;
  ua: string;
  last: number;
  events: LiveEvent[];
}

const MAX_DIALOGS = 200;

export function LivePage() {
  const { tz, settings, can } = usePrefs();
  const client = useQueryClient();
  const [streaming, setStreaming] = useState(false);
  const [paused, setPaused] = useState(false);
  const pausedRef = useRef(false);
  pausedRef.current = paused;
  const { source } = useSource();
  const sourceRef = useRef(source);
  sourceRef.current = source;
  const [dialogs, setDialogs] = useState<Dialog[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [connected, setConnected] = useState(false);
  const [note, setNote] = useState("");

  const enable = useMutation({
    mutationFn: () => put("/settings/live_enabled", { value: "true" }),
    onSuccess: () => client.invalidateQueries({ queryKey: ["me"] }),
    onError: (e: Error) => setNote(e.message),
  });

  useEffect(() => {
    if (!streaming) return;
    const es = new EventSource("/api/v1/live");
    es.onopen = () => {
      setConnected(true);
      setNote("");
    };
    es.onerror = () => {
      // The browser would reconnect forever; a stopped stream should stay stopped.
      es.close();
      setConnected(false);
      setStreaming(false);
      setNote("The stream ended. It stops by itself after a time limit, and there is a limit on streams running at once.");
    };
    es.onmessage = (e) => {
      if (pausedRef.current) return;
      const events: LiveEvent[] = (JSON.parse(e.data) as LiveEvent[]).filter((ev) => !sourceRef.current || ev.agent_ip === sourceRef.current);
      if (events.length === 0) return;
      setDialogs((prev) => {
        const map = new Map(prev.map((d) => [d.id, d]));
        for (const ev of events) {
          const d = map.get(ev.call_id) ?? { id: ev.call_id, caller: ev.caller, callee: ev.callee, state: ev.state, node: ev.node, ua: ev.ua, last: 0, events: [] };
          map.set(ev.call_id, { ...d, state: ev.state, last: usOf(ev.ts), events: [...d.events, ev] });
        }
        return [...map.values()].sort((a, b) => b.last - a.last).slice(0, MAX_DIALOGS);
      });
    };
    return () => {
      es.close();
      setConnected(false);
    };
  }, [streaming]);

  const shown = useMemo(() => {
    const f = filter.trim().toLowerCase();
    if (!f) return dialogs;
    return dialogs.filter((d) => [d.caller, d.callee, d.node, d.ua].some((x) => x.toLowerCase().includes(f)) || d.events.some((e) => String(e.capture_id) === f));
  }, [dialogs, filter]);
  const current = dialogs.find((d) => d.id === selected) ?? null;

  if (!settings.live_enabled) {
    return (
      <div>
        <PageHeader title="Live capture" description="Dialogs as they are captured, grouped by Call-ID." />
        <Card>
          <Empty>
            <div className="font-medium text-ink mb-1">Live capture is turned off</div>
            <p className="mb-3">It's off by default so it uses no resources.</p>
            {can("settings_manage") ? <Button variant="primary" onClick={() => enable.mutate()} className="mx-auto">Turn on live capture</Button> : <p>Ask an administrator to turn it on in Settings.</p>}
            {note && <p className="text-crit mt-2">{note}</p>}
          </Empty>
        </Card>
      </div>
    );
  }

  return (
    <div>
      <PageHeader
        title="Live capture"
        description={`Runs only while started, and stops by itself after ${settings.live_idle_minutes} minutes.`}
        right={
          <div className="flex flex-wrap items-center gap-2">
            <span role="status" className="mr-1 inline-flex items-center gap-2">
              {streaming && !paused ? (
                <>
                  <Orb work={connected ? "stream" : "connect"} label={connected ? "Streaming" : "Connecting"} />
                  {connected ? "Streaming" : "Connecting"}
                </>
              ) : (
                <Status tone={paused ? "warn" : "neutral"}>{paused ? "Paused" : "Stopped"}</Status>
              )}
            </span>
            <input value={filter} onChange={(e) => setFilter(e.target.value)} placeholder="Filter number, node, UA, capture ID" aria-label="Filter dialogs" className={`${fieldClass} h-8 px-2.5 w-[250px] max-w-full`} />
            {streaming ? (
              <>
                <Button onClick={() => setPaused(!paused)}>{paused ? <Play size={14} /> : <Pause size={14} />}{paused ? "Resume" : "Pause"}</Button>
                <Button onClick={() => { setStreaming(false); setPaused(false); }}><Square size={14} />Stop</Button>
              </>
            ) : (
              <Button variant="primary" onClick={() => setStreaming(true)}><Play size={14} />Start</Button>
            )}
            <Button onClick={() => { setDialogs([]); setSelected(null); }}><Trash2 size={14} />Clear</Button>
          </div>
        }
      />
      {note && <p className="text-ink-2 mb-2">{note}</p>}
      <div className="grid lg:grid-cols-2 gap-3 items-start">
        <Card>
          <div className="overflow-auto h-[calc(100vh-150px)]">
            {shown.length === 0 ? (
              streaming && !paused ? (
                <div className="flex flex-col items-center gap-3 py-10 text-ink-3">
                  <Orb work="stream" size={64} label="Waiting for traffic" />
                  Waiting for traffic. New dialogs appear within a few seconds.
                </div>
              ) : <Empty>{streaming ? "Paused." : "Press Start to begin."}</Empty>
            ) : (
              <table className="data-table text-xs">
                <thead><tr><th>From</th><th>To</th><th>State</th><th className="num">Msgs</th><th>Last</th><th>Node</th></tr></thead>
                <tbody>
                  {shown.map((d) => (
                    <tr key={d.id} className="cursor-pointer" aria-selected={selected === d.id} onClick={() => setSelected(d.id)}>
                      <td className="font-mono">{d.caller}</td>
                      <td className="font-mono">{d.callee}</td>
                      <td><Status tone={d.state === "answered" ? "good" : d.state === "failed" || d.state === "timeout" ? "crit" : "info"}>{d.state}</Status></td>
                      <td className="num">{d.events.length}</td>
                      <td>{d.events[d.events.length - 1].label}</td>
                      <td className="text-ink-2">{d.node}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        </Card>
        <Card
          title={current ? `${current.caller} → ${current.callee}` : "Select a dialog"}
          right={current && <Link to="/search" search={{ q: `callid:${current.id}` } as any} className="text-accent hover:underline">Find this call</Link>}
        >
          <div className="overflow-auto h-[calc(100vh-191px)] px-3 pb-3">
            {!current ? <Empty>The message list for the selected dialog updates here as it happens.</Empty> : (
              <table className="w-full text-xs">
                <tbody>
                  {current.events.map((e, i) => (
                    <tr key={i} className="border-b border-line last:border-b-0">
                      <td className="py-1.5 pr-3 font-mono text-ink-2 whitespace-nowrap">{fmtMicros(e.ts, tz).slice(0, 12)}</td>
                      <td className="py-1.5 pr-3 whitespace-nowrap text-ink-2">{e.from} → {e.to}</td>
                      <td className="py-1.5 pr-3 font-mono text-ink-3">{e.capture_id || ""}</td>
                      <td className={`py-1 font-medium ${e.class === "server_error" ? "text-crit" : e.class === "client_error" ? "text-warn" : ""}`}>{e.label}{e.retx ? ` (retx ${e.retx})` : ""}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        </Card>
      </div>
    </div>
  );
}
