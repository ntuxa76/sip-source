import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate, useSearch } from "@tanstack/react-router";
import { ArrowDown, ArrowUp, Ban, Bookmark, ChevronDown, ChevronRight, CircleAlert, Columns3, Download, SlidersHorizontal, Trash2, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { HistogramStrip } from "../components/charts";
import { Button, Card, Empty, ErrorNote, FindingBadge, Loading, MosCell, RegStatus, Select, StatusChip } from "../components/ui";
import { del, get, post, qs } from "../lib/api";
import type { Call } from "../lib/api";
import { rangeParam, usePrefs, useRange } from "../lib/context";
import { fmtDateTime, fmtDuration } from "../lib/format";
import { fieldValue, hasValue, removeToken, setField, toggleValue, tokenize, parseToken } from "../lib/query";

const FACET_LABELS: Record<string, string> = {
  status: "Status code",
  capture: "Capture ID",
  backend: "Backend",
  trunk: "Trunk",
  direction: "Direction",
  state: "Call state",
  finding: "Finding",
  vendor: "Phone vendor",
  node: "SIP proxy",
  pbx: "PBX",
  codec: "Codec",
  group: "Ingress group",
  end: "End cause",
  cause: "Q.850 cause",
  country: "Country",
};

const DEFAULT_FACETS = ["status", "capture", "backend", "trunk", "direction", "finding", "vendor", "node"];

const FIELD_LABELS: Record<string, string> = { ...FACET_LABELS, caller: "Caller", callee: "Callee", number: "Number", callid: "Call-ID", ip: "IP", pdd: "PDD", dur: "Talk time", mos: "MOS", loss: "Loss", ua: "User agent", egress: "Egress status", hangup: "Hangup", last: "Last" };

interface ColumnDef {
  id: string;
  label: string;
  num?: boolean;
  sort?: boolean;
  cell: (c: Call, tz: string) => React.ReactNode;
}

const dash = <span className="text-ink-3">–</span>;
const mono = (v: string) => <span className="font-mono text-xs">{v}</span>;

const COLUMNS: ColumnDef[] = [
  { id: "start_ts", label: "Start", sort: true, cell: (c, tz) => fmtDateTime(c.start_ts, tz).slice(5) },
  { id: "caller_n", label: "Caller", sort: true, cell: (c) => mono(c.caller_n) },
  { id: "callee_n", label: "Callee", sort: true, cell: (c) => mono(c.callee_n) },
  { id: "final_status", label: "Status", sort: true, cell: (c) => <StatusChip status={c.final_status} reason={c.final_reason} /> },
  { id: "state", label: "State", sort: true, cell: (c) => c.state },
  { id: "direction", label: "Direction", sort: true, cell: (c) => <span className="text-ink-2">{c.direction}</span> },
  { id: "trunk", label: "Trunk", sort: true, cell: (c) => c.trunk || dash },
  { id: "capture_ids", label: "Capture IDs", cell: (c) => mono(c.capture_ids.join(", ")) },
  { id: "backend", label: "Backend", sort: true, cell: (c) => c.backend },
  { id: "pbx", label: "PBX", sort: true, cell: (c) => c.pbx },
  { id: "node", label: "SIP proxy", sort: true, cell: (c) => <span className="text-ink-2">{c.node}</span> },
  { id: "pdd_ms", label: "PDD", num: true, sort: true, cell: (c) => fmtDuration(c.pdd_ms) },
  { id: "setup_ms", label: "Setup", num: true, sort: true, cell: (c) => fmtDuration(c.setup_ms) },
  { id: "talk_ms", label: "Talk", num: true, sort: true, cell: (c) => (c.talk_ms ? fmtDuration(c.talk_ms) : dash) },
  { id: "min_mos", label: "MOS", num: true, sort: true, cell: (c) => <MosCell mos={c.min_mos} /> },
  { id: "max_loss_pct", label: "Loss", num: true, sort: true, cell: (c) => (c.max_loss_pct === null ? dash : `${c.max_loss_pct.toFixed(1)} %`) },
  { id: "codecs", label: "Codec", cell: (c) => c.codecs.join(", ") || dash },
  { id: "ua_vendor", label: "Vendor", sort: true, cell: (c) => c.ua_vendor },
  { id: "ua_model", label: "Model", sort: true, cell: (c) => c.ua_model },
  { id: "caller_ip", label: "Caller IP", sort: true, cell: (c) => mono(c.caller_ip) },
  { id: "callee_ip", label: "Callee IP", sort: true, cell: (c) => mono(c.callee_ip) },
  { id: "end_cause", label: "End cause", sort: true, cell: (c) => c.end_cause.replaceAll("_", " ") || dash },
  { id: "hangup_side", label: "Hangup", sort: true, cell: (c) => c.hangup_side },
  { id: "msg_count", label: "Msgs", num: true, sort: true, cell: (c) => c.msg_count },
  {
    id: "findings",
    label: "Findings",
    cell: (c) => (
      <span className="space-x-1">
        {c.findings.slice(0, 2).map((f) => <FindingBadge code={f} key={f} />)}
        {c.findings.length > 2 && <span className="text-ink-3 text-xs">+{c.findings.length - 2}</span>}
      </span>
    ),
  },
];

const DEFAULT_COLUMNS = ["start_ts", "caller_n", "callee_n", "final_status", "direction", "trunk", "capture_ids", "pdd_ms", "talk_ms", "min_mos", "findings"];

function useClickAway(onAway: () => void) {
  const ref = useRef<HTMLDivElement>(null);
  const cb = useRef(onAway);
  cb.current = onAway;
  useEffect(() => {
    const click = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) cb.current();
    };
    document.addEventListener("mousedown", click);
    return () => document.removeEventListener("mousedown", click);
  }, []);
  return ref;
}

function TextFilter({ label, q, field, op = ":", placeholder, type = "text", suffix, onApply }: { label: string; q: string; field: string; op?: string; placeholder?: string; type?: string; suffix?: string; onApply: (next: string) => void }) {
  const current = fieldValue(q, field, op).replace(/s$/, suffix === "s" ? "" : "s");
  const [value, setValue] = useState(current);
  useEffect(() => setValue(current), [current]);
  const apply = () => {
    if (value === current) return;
    onApply(setField(q, field, op, value && suffix ? `${value}${suffix}` : value));
  };
  return (
    <label className="block">
      <span className="text-xs text-ink-2">{label}</span>
      <input type={type} value={value} onChange={(e) => setValue(e.target.value)} onBlur={apply} onKeyDown={(e) => e.key === "Enter" && apply()} placeholder={placeholder} spellCheck={false} className="w-full h-7 rounded-md bg-sunken border border-line px-2 font-mono text-xs focus:border-accent focus:outline-none" />
    </label>
  );
}

function Section({ title, count, children, defaultOpen = true }: { title: string; count?: number; children: React.ReactNode; defaultOpen?: boolean }) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <section className="border-b border-line last:border-b-0 py-2">
      <button type="button" onClick={() => setOpen(!open)} aria-expanded={open} className="flex items-center gap-1 w-full text-left font-medium text-[12.5px]">
        {open ? <ChevronDown size={13} aria-hidden /> : <ChevronRight size={13} aria-hidden />}
        <span className="flex-1">{title}</span>
        {count ? <span className="bg-accent text-white rounded-full px-1.5 text-[10.5px] leading-[16px] tnum">{count}</span> : null}
      </button>
      {open && <div className="mt-1.5">{children}</div>}
    </section>
  );
}

function FilterPanel({ q, data, facets, onChange, onFacets }: { q: string; data: any; facets: string[]; onChange: (next: string) => void; onFacets: (f: string[]) => void }) {
  const [customizing, setCustomizing] = useState(false);
  const ref = useClickAway(() => setCustomizing(false));
  const tokens = tokenize(q).map(parseToken).filter(Boolean);

  return (
    <aside className="w-[272px] shrink-0 sticky top-0 self-start max-h-[calc(100vh-90px)] flex flex-col bg-surface border border-line rounded-lg" aria-label="Search filters">
      <header className="flex items-center gap-2 px-3 h-10 border-b border-line shrink-0 relative" ref={ref}>
        <SlidersHorizontal size={14} className="text-accent" aria-hidden />
        <h2 className="font-semibold flex-1">Filters</h2>
        {tokens.length > 0 && <button type="button" onClick={() => onChange("")} className="text-xs text-accent hover:underline">Clear all</button>}
        <button type="button" onClick={() => setCustomizing(!customizing)} className="text-xs text-ink-2 hover:text-ink" aria-expanded={customizing}>Customize</button>
        {customizing && (
          <div className="absolute z-20 top-full right-2 mt-1 w-[220px] bg-raised border border-line-2 rounded-md shadow-lg p-2.5">
            <p className="text-xs text-ink-3 mb-1.5">Choose which filter lists to show</p>
            {Object.entries(FACET_LABELS).map(([id, label]) => (
              <label key={id} className="flex items-center gap-2 py-0.5">
                <input type="checkbox" checked={facets.includes(id)} onChange={() => onFacets(facets.includes(id) ? facets.filter((f) => f !== id) : [...facets, id])} />
                {label}
              </label>
            ))}
            <button type="button" onClick={() => onFacets(DEFAULT_FACETS)} className="text-xs text-accent hover:underline mt-1">Reset</button>
          </div>
        )}
      </header>
      <div className="overflow-y-auto px-3 flex-1 min-h-0">
        <Section title="Find">
          <div className="space-y-1.5">
            <TextFilter label="Caller" q={q} field="caller" placeholder="07700012345" onApply={onChange} />
            <TextFilter label="Callee" q={q} field="callee" placeholder="*2345 or 96477*" onApply={onChange} />
            <TextFilter label="Either number" q={q} field="number" onApply={onChange} />
            <TextFilter label="Call-ID or correlation ID" q={q} field="callid" onApply={onChange} />
            <TextFilter label="Capture ID" q={q} field="capture" placeholder="201 or 201,211" onApply={onChange} />
            <TextFilter label="IP address or CIDR" q={q} field="ip" placeholder="100.64.0.0/16" onApply={onChange} />
            <TextFilter label="User agent contains" q={q} field="ua" onApply={onChange} />
          </div>
        </Section>
        <Section title="Limits" defaultOpen={false}>
          <div className="grid grid-cols-2 gap-1.5">
            <TextFilter label="PDD over (s)" q={q} field="pdd" op=">" suffix="s" type="number" onApply={onChange} />
            <TextFilter label="PDD under (s)" q={q} field="pdd" op="<" suffix="s" type="number" onApply={onChange} />
            <TextFilter label="Talk over (s)" q={q} field="dur" op=">" suffix="s" type="number" onApply={onChange} />
            <TextFilter label="Talk under (s)" q={q} field="dur" op="<" suffix="s" type="number" onApply={onChange} />
            <TextFilter label="MOS below" q={q} field="mos" op="<" type="number" onApply={onChange} />
            <TextFilter label="Loss over (%)" q={q} field="loss" op=">" type="number" onApply={onChange} />
          </div>
        </Section>
        {facets.map((id) => {
          const items: any[] = data.facets[id] ?? [];
          const active = items.filter((i) => hasValue(q, id, i.value, false) || hasValue(q, id, i.value, true)).length;
          const max = Math.max(1, ...items.map((i) => i.count));
          return (
            <Section key={id} title={FACET_LABELS[id] ?? id} count={active}>
              {items.length === 0 && <p className="text-ink-3 text-xs">No values in these results</p>}
              <ul>
                {items.map((i) => {
                  const on = hasValue(q, id, i.value, false);
                  const off = hasValue(q, id, i.value, true);
                  return (
                    <li key={i.value} className="group/row relative flex items-center gap-1.5 py-[3px] pl-1 pr-1 rounded hover:bg-sunken text-xs">
                      <span className="absolute left-0 top-0 bottom-0 bg-accent/10 rounded" style={{ width: `${(i.count / max) * 100}%` }} aria-hidden />
                      <input type="checkbox" id={`f-${id}-${i.value}`} checked={on} disabled={i.value === "in progress"} onChange={() => onChange(toggleValue(q, id, i.value))} className="relative" />
                      <label htmlFor={`f-${id}-${i.value}`} className={`relative flex-1 truncate cursor-pointer ${off ? "line-through text-ink-3" : ""}`}>{i.label ?? i.value}</label>
                      <button type="button" title={off ? "Stop excluding" : "Exclude this value"} aria-label={`Exclude ${i.value}`} onClick={() => onChange(toggleValue(q, id, i.value, true))} className={`relative ${off ? "text-crit" : "text-ink-3 opacity-0 group-hover/row:opacity-100 focus:opacity-100"}`}><Ban size={11} /></button>
                      <span className="relative tnum text-ink-2">{i.count.toLocaleString()}</span>
                    </li>
                  );
                })}
              </ul>
            </Section>
          );
        })}
      </div>
    </aside>
  );
}

function ActiveFilters({ q, onChange }: { q: string; onChange: (next: string) => void }) {
  const raws = tokenize(q);
  if (raws.length === 0) return <p className="text-ink-3 text-xs">No filters: showing every call in the time range. Use the panel on the left, or type in the search box.</p>;
  return (
    <div className="flex flex-wrap items-center gap-1.5">
      {raws.map((raw) => {
        const t = parseToken(raw);
        const label = t ? `${FIELD_LABELS[t.field] ?? t.field} ${t.negated ? "is not" : t.op === ":" ? "is" : t.op} ${t.value}` : raw;
        return (
          <span key={raw} className="inline-flex items-center gap-1 bg-accent-wash rounded pl-2 pr-1 h-6 text-xs">
            {label}
            <button type="button" aria-label={`Remove filter ${label}`} onClick={() => onChange(removeToken(q, raw))} className="text-ink-2 hover:text-ink"><X size={12} /></button>
          </span>
        );
      })}
    </div>
  );
}

function ColumnPicker({ columns, onChange }: { columns: string[]; onChange: (c: string[]) => void }) {
  const [open, setOpen] = useState(false);
  const ref = useClickAway(() => setOpen(false));
  return (
    <div className="relative" ref={ref}>
      <Button onClick={() => setOpen(!open)} aria-expanded={open}><Columns3 size={13} />Columns</Button>
      {open && (
        <div className="absolute z-20 right-0 mt-1 w-[210px] max-h-[360px] overflow-y-auto bg-raised border border-line-2 rounded-md shadow-lg p-2.5">
          {COLUMNS.map((c) => (
            <label key={c.id} className="flex items-center gap-2 py-0.5">
              <input type="checkbox" checked={columns.includes(c.id)} onChange={() => onChange(columns.includes(c.id) ? columns.filter((x) => x !== c.id) : COLUMNS.filter((d) => columns.includes(d.id) || d.id === c.id).map((d) => d.id))} />
              {c.label}
            </label>
          ))}
          <button type="button" onClick={() => onChange(DEFAULT_COLUMNS)} className="text-xs text-accent hover:underline mt-1">Reset</button>
        </div>
      )}
    </div>
  );
}

function SavedSearches({ q, onLoad }: { q: string; onLoad: (q: string) => void }) {
  const client = useQueryClient();
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [shared, setShared] = useState(false);
  const ref = useClickAway(() => setOpen(false));
  const { data } = useQuery({ queryKey: ["saved-searches"], queryFn: () => get("/saved-searches") });
  const refresh = () => client.invalidateQueries({ queryKey: ["saved-searches"] });
  const save = useMutation({ mutationFn: () => post("/saved-searches", { name, q, shared }), onSuccess: () => { setName(""); refresh(); } });
  const remove = useMutation({ mutationFn: (id: number) => del(`/saved-searches/${id}`), onSuccess: refresh });
  return (
    <div className="relative" ref={ref}>
      <Button onClick={() => setOpen(!open)} aria-expanded={open}><Bookmark size={13} />Saved</Button>
      {open && (
        <div className="absolute z-20 right-0 mt-1 w-[300px] bg-raised border border-line-2 rounded-md shadow-lg p-2.5">
          <form className="flex gap-1.5" onSubmit={(e) => { e.preventDefault(); if (name.trim()) save.mutate(); }}>
            <input value={name} onChange={(e) => setName(e.target.value)} placeholder="Name this search" aria-label="Saved search name" className="flex-1 h-7 rounded bg-sunken border border-line px-2 text-xs" />
            <Button type="submit" variant="primary" disabled={!q || !name.trim()}>Save</Button>
          </form>
          <label className="flex items-center gap-2 mt-1.5 text-xs text-ink-2"><input type="checkbox" checked={shared} onChange={(e) => setShared(e.target.checked)} />Share with everyone</label>
          <ul className="mt-2 border-t border-line pt-1 max-h-[260px] overflow-y-auto">
            {(data ?? []).length === 0 && <li className="text-ink-3 text-xs py-1">Nothing saved yet</li>}
            {(data ?? []).map((s: any) => (
              <li key={s.id} className="flex items-center gap-1 hover:bg-sunken rounded px-1">
                <button type="button" onClick={() => { onLoad(s.q); setOpen(false); }} className="flex-1 text-left py-1 truncate">
                  {s.name}{s.shared && <span className="text-ink-3 text-xs"> · shared</span>}
                </button>
                {s.mine && <button type="button" aria-label={`Delete ${s.name}`} onClick={() => remove.mutate(s.id)} className="text-ink-3 hover:text-crit p-1"><Trash2 size={12} /></button>}
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

export function SearchPage() {
  const search = useSearch({ strict: false }) as { q?: string; sort?: string; dir?: string; page?: string };
  const q = search.q ?? "";
  const { from, to, setRange } = useRange();
  const { tz, refreshMs, settings, prefs, savePrefs, density, can } = usePrefs();
  const navigate = useNavigate();
  const pageSize = prefs.searchPageSize ?? settings.search_page_size;
  const page = Number(search.page ?? 0);
  const facets = prefs.searchFacets ?? DEFAULT_FACETS;
  const columnIds = prefs.searchColumns ?? DEFAULT_COLUMNS;
  const columns = COLUMNS.filter((c) => columnIds.includes(c.id));
  const sort = search.sort ?? "start_ts";
  const dir = search.dir ?? "desc";

  const { data, error, isFetching } = useQuery({
    queryKey: ["search", q, from, to, pageSize, page, tz, sort, dir, facets.join(",")],
    queryFn: () => get(`/calls/search${qs({ q, from: from, to, tz, limit: pageSize, offset: page * pageSize, sort, dir, facets: facets.join(",") })}`),
    refetchInterval: to === "now" ? refreshMs : false,
    placeholderData: keepPreviousData,
  });

  const setQ = (next: string) => navigate({ to: "/search", search: (p: any) => ({ ...p, q: next || undefined, page: undefined }) } as any);
  const setSort = (id: string) => navigate({ to: "/search", search: (p: any) => ({ ...p, sort: id, dir: sort === id && dir === "desc" ? "asc" : "desc", page: undefined }) } as any);
  const setPage = (n: number) => navigate({ to: "/search", search: (p: any) => ({ ...p, page: n > 0 ? String(n) : undefined }) } as any);

  if (error) return <ErrorNote error={error} />;
  if (!data) return <Loading />;

  const problems = data.tokens.filter((t: any) => t.error);
  const detected = data.tokens.filter((t: any) => t.detected);
  const pad = density === "compact" ? "[&_td]:!py-[3px] [&_th]:!py-1" : "";
  const csvColumns = columnIds.join(",");
  const lastPage = Math.max(0, Math.ceil(data.total / pageSize) - 1);

  return (
    <div className={`flex gap-3 items-start ${isFetching ? "opacity-95" : ""}`}>
      <FilterPanel q={q} data={data} facets={facets} onChange={setQ} onFacets={(f) => savePrefs({ searchFacets: f })} />
      <div className="flex-1 min-w-0 space-y-3">
        <ActiveFilters q={q} onChange={setQ} />
        {(problems.length > 0 || detected.length > 0) && (
          <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs">
            {detected.map((t: any) => (
              <span key={t.raw} className="text-ink-2"><code className="font-mono">{t.raw}</code> read as {t.detected}</span>
            ))}
            {problems.map((t: any) => (
              <span key={t.raw} className="text-crit">
                <CircleAlert size={12} className="inline -mt-0.5 mr-1" aria-hidden />
                <code className="font-mono">{t.raw}</code>: {t.error}
              </span>
            ))}
          </div>
        )}

        {data.subscribers.map((s: any) => (
          <Card key={s.number}>
            <div className="flex items-center gap-3 px-3.5 py-2.5">
              <span className="text-ink-3 text-xs">Subscriber</span>
              <Link to="/subscribers/$number" params={{ number: s.number }} className="font-mono font-medium hover:underline">{s.number}</Link>
              <RegStatus status={s.status} />
              <span className="text-ink-2 text-xs">{s.ua_vendor} {s.ua_model} · {s.src_ip} · {s.group}</span>
            </div>
          </Card>
        ))}

        <Card title={<span>{data.total.toLocaleString()} calls</span>} right={<span className="tnum">drag to narrow the range · {tz}</span>}>
          <div className="px-2 pb-2">
            <HistogramStrip histogram={data.histogram} onZoom={(a, b) => setRange(rangeParam(a), rangeParam(Math.min(b, Date.now())))} />
          </div>
        </Card>

        <Card>
          <div className="flex items-center justify-end gap-2 px-3 py-2 border-b border-line">
            <SavedSearches q={q} onLoad={setQ} />
            <ColumnPicker columns={columnIds} onChange={(c) => savePrefs({ searchColumns: c })} />
            <Select label="Row density" value={density} onChange={(v) => savePrefs({ density: v as "compact" | "comfortable" })} options={[{ value: "comfortable", label: "Comfortable" }, { value: "compact", label: "Compact" }]} />
            <Select label="Rows per page" value={String(pageSize)} onChange={(v) => { savePrefs({ searchPageSize: Number(v) }); setPage(0); }} options={[25, 50, 100, 200, 500].filter((n) => n <= settings.search_max_page_size).map((n) => ({ value: String(n), label: `${n} rows` }))} />
            {can("export_bulk") && (
              <a className="inline-flex items-center gap-1.5 rounded-md px-2.5 h-7 text-[12.5px] font-medium bg-raised border border-line-2 hover:bg-sunken" href={`/api/v1/calls/export.csv${qs({ q, from: from, to, columns: csvColumns })}`} download>
                <Download size={13} />CSV
              </a>
            )}
          </div>
          {data.rows.length === 0 ? (
            <Empty>
              <div className="font-medium text-ink mb-1">No calls match</div>
              {data.hints.map((h: string) => <div key={h}>{h}</div>)}
            </Empty>
          ) : (
            <>
              <div className="overflow-x-auto">
                <table className={`data-table ${pad}`}>
                  <thead>
                    <tr>
                      {columns.map((c) => (
                        <th key={c.id} className={c.num ? "num" : ""} aria-sort={sort === c.id ? (dir === "asc" ? "ascending" : "descending") : undefined}>
                          {c.sort ? (
                            <button type="button" onClick={() => setSort(c.id)} className="inline-flex items-center gap-0.5 hover:text-ink">
                              {c.label}
                              {sort === c.id && (dir === "asc" ? <ArrowUp size={11} /> : <ArrowDown size={11} />)}
                            </button>
                          ) : c.label}
                        </th>
                      ))}
                    </tr>
                  </thead>
                  <tbody>
                    {data.rows.map((c: Call) => (
                      <tr key={c.call_uid} className="cursor-pointer" onClick={() => navigate({ to: "/calls/$uid", params: { uid: c.call_uid } } as any)}>
                        {columns.map((col, i) => (
                          <td key={col.id} className={col.num ? "num" : ""}>
                            {i === 0 ? <Link to="/calls/$uid" params={{ uid: c.call_uid }} onClick={(e) => e.stopPropagation()} className="hover:underline">{col.cell(c, tz)}</Link> : col.cell(c, tz)}
                          </td>
                        ))}
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              <div className="flex items-center justify-between px-3.5 py-2">
                <span className="text-ink-3 text-xs tnum">{(page * pageSize + 1).toLocaleString()}–{Math.min((page + 1) * pageSize, data.total).toLocaleString()} of {data.total.toLocaleString()}</span>
                <div className="flex gap-2">
                  <Button onClick={() => setPage(page - 1)} disabled={page <= 0}>Previous</Button>
                  <Button onClick={() => setPage(page + 1)} disabled={page >= lastPage}>Next</Button>
                </div>
              </div>
            </>
          )}
        </Card>
      </div>
    </div>
  );
}
