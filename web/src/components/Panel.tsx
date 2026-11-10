import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { ArrowDownRight, ArrowUpRight, Code2, Copy, Download, Info, Loader2, Pencil, Table2, Trash2 } from "lucide-react";
import { useState } from "react";
import { ApiError, get, post, qs } from "../lib/api";
import type { Call, MetricQuery, TimeSeriesResult } from "../lib/api";
import { rangeParam, usePrefs, useRange, useSource } from "../lib/context";
import type { PanelDef } from "../lib/dashboard";
import { downloadBlob, fmtDateTime, fmtDuration, fmtHMS, formatMetric, metricText } from "../lib/format";
import { BarList, HeatChart, Histogram, MiniBars, TimeChart } from "./charts";
import { Card, Dialog, Empty, FindingBadge, Loading, MosCell, RegStatus, StatusChip } from "./ui";

export interface PanelContext {
  /** Set when the panel is saved on a dashboard, so a viewer can run its SQL without permission to write SQL. */
  dashboard?: string;
  group?: string;
  varFilter?: string;
  vars?: Record<string, string[]>;
  editing?: boolean;
  onEdit?: () => void;
  onDuplicate?: () => void;
  onDelete?: () => void;
}

const SHAPES: Record<string, string | undefined> = { stat: "stat", top: "top", table: "table", heatmap: "heatmap", histogram: "histogram" };

export function buildQuery(p: PanelDef, from: string, to: string, tz: string, ctx: PanelContext, extra: Partial<MetricQuery> = {}): MetricQuery {
  const q = p.query;
  const sqlMode = q.mode === "sql" && q.sql;
  const base: MetricQuery = {
    from,
    to,
    tz,
    shape: SHAPES[p.type],
    top: q.top ?? (p.type === "top" ? (p.options.limit ?? 8) : undefined),
    order: p.options.order,
    min_interval: q.minInterval,
    var_filter: ctx.varFilter,
    vars: ctx.vars,
    ...extra,
  };
  if (sqlMode) {
    const saved = ctx.dashboard && !ctx.editing;
    return { ...base, title: p.title, unit: p.options.unit, ...(saved ? { panel_ref: { dashboard: ctx.dashboard!, panel: p.id } } : { sql: q.sql }) };
  }
  return { ...base, metric: q.metric, metrics: q.metrics, group_by: q.groupBy, filter: q.filter };
}

function useMetric(q: MetricQuery, enabled = true) {
  const { source } = useSource();
  if (source) q = { ...q, var_filter: [q.var_filter, `source:${source}`].filter(Boolean).join(" ") };
  const { refreshMs } = usePrefs();
  return useQuery({
    queryKey: ["metric", q],
    queryFn: () => post<any>("/metrics/query", q),
    refetchInterval: refreshMs,
    placeholderData: keepPreviousData,
    enabled,
    retry: false,
  });
}

const DIM_TO_FIELD: Record<string, string> = {
  trunk: "trunk",
  direction: "direction",
  node: "node",
  pbx: "node",
  media_node: "node",
  codec: "codec",
  ua_vendor: "vendor",
  finding: "finding",
  ingress_group: "group",
  egress_group: "group",
  end_cause: "end",
  country: "country",
  status_class: "status",
  status: "status",
  caller: "caller",
  callee: "callee",
  src_ip: "ip",
  capture_id: "capture",
  backend: "backend",
};

const METRIC_TO_FILTER: Record<string, string> = {
  failures: "status>=400",
  scanner: "finding:SCANNER",
};

function drillQuery(p: PanelDef, seriesName: string | null, ctx: PanelContext): string {
  const q = p.query;
  const parts: string[] = [];
  if (q.filter) parts.push(q.filter);
  if (ctx.varFilter) parts.push(ctx.varFilter);
  if (q.metric && METRIC_TO_FILTER[q.metric]) parts.push(METRIC_TO_FILTER[q.metric]);
  if (seriesName && seriesName !== "other" && q.groupBy) {
    if (q.groupBy === "q850") parts.push(`cause:${seriesName.split(" ")[0]}`);
    else if (q.groupBy === "prefix") parts.push(`callee:${seriesName}*`);
    else if (DIM_TO_FIELD[q.groupBy]) parts.push(`${DIM_TO_FIELD[q.groupBy]}:${/\s/.test(seriesName) ? `"${seriesName}"` : seriesName}`);
  }
  return parts.join(" ");
}

function isDrillable(family?: string) {
  return family === "calls" || family === "occupancy";
}

export function PanelView({ panel, ctx, height }: { panel: PanelDef; ctx: PanelContext; height: number }) {
  switch (panel.type) {
    case "stat":
      return <StatPanel panel={panel} ctx={ctx} height={height} />;
    case "timeseries":
      return <TimePanel panel={panel} ctx={ctx} height={height} />;
    case "top":
      return <TopPanel panel={panel} ctx={ctx} height={height} />;
    case "table":
      return <TablePanel panel={panel} ctx={ctx} height={height} />;
    case "heatmap":
      return <HeatPanel panel={panel} ctx={ctx} height={height} />;
    case "histogram":
      return <HistogramPanel panel={panel} ctx={ctx} height={height} />;
    case "calls":
      return <CallsPanel panel={panel} ctx={ctx} height={height} />;
    case "failing_aors":
      return <FailingAorsPanel panel={panel} ctx={ctx} height={height} />;
    case "text":
      return (
        <PanelFrame panel={panel} ctx={ctx} height={height}>
          <div className="px-3.5 pb-3 whitespace-pre-wrap overflow-auto h-full">{panel.options.markdown ?? ""}</div>
        </PanelFrame>
      );
    default:
      return <Card title={panel.title}><Empty>Unknown panel type</Empty></Card>;
  }
}

function IconBtn({ label, onClick, active, children }: { label: string; onClick: () => void; active?: boolean; children: React.ReactNode }) {
  return (
    <button type="button" aria-label={label} title={label} onClick={onClick} onMouseDown={(e) => e.stopPropagation()} className={`panel-action inline-grid place-items-center w-6 h-6 rounded-sm hover:bg-hover hover:text-ink ${active ? "text-accent" : "text-ink-3"}`}>
      {children}
    </button>
  );
}

function EditActions({ ctx }: { ctx: PanelContext }) {
  if (!ctx.onEdit) return null;
  return (
    <>
      <IconBtn label="Edit panel" onClick={ctx.onEdit}><Pencil size={13} /></IconBtn>
      <IconBtn label="Duplicate panel" onClick={ctx.onDuplicate!}><Copy size={13} /></IconBtn>
      <IconBtn label="Delete panel" onClick={ctx.onDelete!}><Trash2 size={13} /></IconBtn>
    </>
  );
}

/** Why a panel has nothing to show, said quietly. A first start is not an error. */
function PanelNote({ error }: { error: unknown }) {
  const e = error as ApiError | Error;
  const starting = e instanceof ApiError && e.status === 503 && /starting|few seconds/i.test(e.message);
  return (
    <div className="flex items-start gap-1.5 text-ink-3 text-xs px-3 py-2.5">
      {starting ? <Loader2 size={13} className="animate-spin shrink-0 mt-px" aria-hidden /> : <Info size={13} className="shrink-0 mt-px" aria-hidden />}
      <span className="line-clamp-3">{starting ? "Storage is starting. Data appears in a few seconds." : e.message}</span>
    </div>
  );
}

function PanelFrame({ panel, ctx, height, info, actions, children }: { panel: PanelDef; ctx: PanelContext; height: number; info?: string; actions?: React.ReactNode; children: React.ReactNode }) {
  return (
    <section className={`group/panel bg-surface border rounded-lg flex flex-col overflow-hidden ${ctx.onEdit ? "border-line-2" : "border-line"}`} style={{ height }} title={panel.options.description}>
      <header className={`flex items-center justify-between gap-3 h-9 px-3 shrink-0 ${ctx.onEdit ? "panel-drag cursor-move" : ""}`}>
        <h2 className="font-semibold truncate">{panel.title}</h2>
        <div className="flex items-center gap-1 text-ink-3 text-xs shrink-0">
          <span className={`items-center gap-0.5 ${ctx.onEdit ? "flex" : "hidden group-hover/panel:flex group-focus-within/panel:flex"}`}>
            {actions}
            <EditActions ctx={ctx} />
          </span>
          {info && <span className="tnum whitespace-nowrap">{info}</span>}
        </div>
      </header>
      <div className="flex-1 min-h-0">{children}</div>
    </section>
  );
}

function StatPanel({ panel, ctx, height }: { panel: PanelDef; ctx: PanelContext; height: number }) {
  const { from, to } = useRange();
  const { tz } = usePrefs();
  const { data, error } = useMetric(buildQuery(panel, from, to, tz, ctx, { compare: true, max_points: 40 }));
  const d = data as TimeSeriesResult | undefined;
  const unit = d?.metric.unit ?? "";
  // Before any traffic there is nothing to compare: show zero, not a gap.
  const f = d ? formatMetric(d.total ?? 0, unit) : null;
  const delta = d && d.total !== null && d.previous !== null && d.previous !== 0 ? ((d.total - d.previous) / Math.abs(d.previous)) * 100 : null;
  const good = delta === null || !d || d.metric.better === "none" || Math.abs(delta) < 0.5 ? null : delta > 0 === (d.metric.better === "higher");
  const spark = d?.series.length ? d.buckets.map((_, i) => d.series[0].values[i]).slice(0, d.partial_from ?? undefined) : [];
  const DeltaIcon = delta !== null && delta < 0 ? ArrowDownRight : ArrowUpRight;
  const deltaTone = good === null ? "text-ink-3" : good ? "text-good" : "text-crit";
  // Tall cards put the bar strip under the number, as wide as the card.
  const tall = height >= 110;
  return (
    <section className={`group/panel relative bg-surface border rounded-lg px-3 pt-2.5 pb-3 overflow-hidden flex flex-col ${ctx.onEdit ? "border-line-2" : "border-line"}`} style={{ height }} title={panel.options.description ?? d?.metric.description}>
      <div className={`flex items-center justify-between gap-2 ${ctx.onEdit ? "panel-drag cursor-move" : ""}`}>
        <div className="text-xs font-medium text-ink-2 truncate">{panel.title}</div>
        <span className={`items-center gap-0.5 -my-1 ${ctx.onEdit ? "flex" : "hidden"}`}><EditActions ctx={ctx} /></span>
      </div>
      {error ? (
        <div className="mt-1"><PanelNote error={error} /></div>
      ) : (
        <div className={tall ? "flex flex-col flex-1 min-h-0" : "flex items-end gap-3 flex-1 min-h-0"}>
          <div className={`min-w-0 ${tall ? "flex items-baseline gap-2.5 flex-wrap" : "flex-1"}`}>
            <div className="text-2xl font-semibold tnum whitespace-nowrap mt-0.5">
              {f?.value ?? "…"}
              {f?.unit && <span className="text-sm font-normal text-ink-3 ml-1">{f.unit}</span>}
            </div>
            <div className="flex items-center gap-1 text-xs tnum whitespace-nowrap min-w-0 h-4">
              {delta !== null && (
                <>
                  <DeltaIcon size={14} aria-hidden className={`shrink-0 -mr-0.5 ${deltaTone}`} />
                  <span className="font-medium text-ink">{Math.abs(delta).toFixed(1)}%</span>
                  <span className="text-ink-3 truncate">vs previous</span>
                </>
              )}
            </div>
          </div>
          <div className={tall ? "mt-auto pt-2" : "w-[112px] shrink-0 pb-0.5"}>
            <MiniBars values={spark} unit={unit} count={tall ? 12 : 8} height={tall ? Math.min(40, height - 96) : 30} />
          </div>
        </div>
      )}
    </section>
  );
}

function TimePanel({ panel, ctx, height }: { panel: PanelDef; ctx: PanelContext; height: number }) {
  const { from, to, setRange, defaultFrom } = useRange();
  const { tz } = usePrefs();
  const navigate = useNavigate();
  const [mode, setMode] = useState<"chart" | "table">("chart");
  const [showSql, setShowSql] = useState(false);
  const { data, error, isLoading } = useMetric(buildQuery(panel, from, to, tz, ctx, { max_points: 280 }));
  const d = data as TimeSeriesResult | undefined;
  const legend = panel.options.legend !== false;
  const hasLegend = legend && (d?.series.length ?? 0) > 1;
  const body = height - 36 - 10;
  const chartHeight = hasLegend ? body - 26 : body;

  const csv = () => {
    if (!d) return;
    const header = ["bucket", ...d.series.map((s) => s.name)].join(",");
    const lines = d.buckets.map((b, i) => [new Date(b).toISOString(), ...d.series.map((s) => s.values[i] ?? "")].join(","));
    downloadBlob(`${panel.title || "panel"}.csv`, [header, ...lines].join("\n"), "text/csv");
  };

  return (
    <PanelFrame
      panel={panel}
      ctx={ctx}
      height={height}
      info={d ? `${d.interval_label} · ${d.tz}` : undefined}
      actions={
        <>
          <IconBtn label="Table view" active={mode === "table"} onClick={() => setMode(mode === "table" ? "chart" : "table")}><Table2 size={13} /></IconBtn>
          <IconBtn label="Show query" onClick={() => setShowSql(true)}><Code2 size={13} /></IconBtn>
          <IconBtn label="Export CSV" onClick={csv}><Download size={13} /></IconBtn>
        </>
      }
    >
      <div className="px-2 pb-2 h-full">
        {error ? (
          <PanelNote error={error} />
        ) : isLoading || !d ? (
          <Loading />
        ) : mode === "table" ? (
          <SeriesTable data={d} height={body} />
        ) : d.series.length === 0 ? (
          <Empty>No data yet</Empty>
        ) : (
          <TimeChart
            data={d}
            viz={panel.options.viz ?? "line"}
            height={chartHeight}
            group={ctx.group}
            yMin={panel.options.yMin}
            yMax={panel.options.yMax}
            legend={legend}
            events={ctx.editing ? undefined : {
              onZoom: (a, b) => setRange(rangeParam(a), rangeParam(Math.min(b, Date.now()))),
              onResetZoom: () => setRange(defaultFrom, "now"),
              onClick: isDrillable(d.metric.family)
                ? (a, b, series) => navigate({ to: "/search", search: { from: rangeParam(a), to: rangeParam(b), q: drillQuery(panel, series, ctx) } } as any)
                : undefined,
            }}
          />
        )}
      </div>
      {showSql && d && <SqlDialog sql={d.sql} source={d.source} onClose={() => setShowSql(false)} />}
    </PanelFrame>
  );
}

function SqlDialog({ sql, source, onClose }: { sql: string; source: string; onClose: () => void }) {
  return (
    <Dialog title="Query" onClose={onClose} width={680}>
      <pre className="font-mono text-xs bg-sunken border border-line rounded-md p-3 overflow-x-auto whitespace-pre-wrap">{sql || "This panel has no SQL form."}</pre>
      <p className="text-ink-3 text-xs mt-2">Source: {source}. To change it, edit the dashboard and switch the panel to SQL.</p>
    </Dialog>
  );
}

function SeriesTable({ data, height }: { data: TimeSeriesResult; height: number }) {
  const { tz } = usePrefs();
  return (
    <div className="overflow-auto" style={{ height }}>
      <table className="data-table text-xs">
        <thead>
          <tr>
            <th>Bucket</th>
            {data.series.map((s) => <th className="num" key={s.name}>{s.name === "value" ? data.metric.name : s.name}</th>)}
          </tr>
        </thead>
        <tbody>
          {data.buckets.map((b, i) => (
            <tr key={b}>
              <td>{fmtDateTime(b, tz).slice(5, 16)}</td>
              {data.series.map((s) => <td className="num" key={s.name}>{metricText(s.values[i], data.metric.unit)}</td>)}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function TopPanel({ panel, ctx, height }: { panel: PanelDef; ctx: PanelContext; height: number }) {
  const { from, to } = useRange();
  const { tz } = usePrefs();
  const navigate = useNavigate();
  const { data, error } = useMetric(buildQuery(panel, from, to, tz, ctx));
  const rows = (data?.rows ?? []).filter((r: any) => r.value !== null);
  const g = panel.query.groupBy ?? "";
  const drill = isDrillable(data?.metric.family) && (g === "q850" || g === "prefix" || DIM_TO_FIELD[g]);
  return (
    <PanelFrame panel={panel} ctx={ctx} height={height}>
      <div className="overflow-y-auto h-full">
        {error ? <PanelNote error={error} /> : !data ? <Loading /> : rows.length === 0 ? <Empty>No data yet</Empty> : (
          <BarList
            rows={rows}
            unit={data.metric.unit}
            onClick={drill && !ctx.editing ? (k) => navigate({ to: "/search", search: (p: any) => ({ from: p.from, to: p.to, q: drillQuery(panel, k, ctx) }) } as any) : undefined}
          />
        )}
      </div>
    </PanelFrame>
  );
}

function TablePanel({ panel, ctx, height }: { panel: PanelDef; ctx: PanelContext; height: number }) {
  const { from, to } = useRange();
  const { tz } = usePrefs();
  const { data, error } = useMetric(buildQuery(panel, from, to, tz, ctx));
  return (
    <PanelFrame panel={panel} ctx={ctx} height={height}>
      <div className="h-full overflow-auto border-t border-line">
        {error ? <PanelNote error={error} /> : !data ? <Loading /> : (
          <table className="data-table">
            <thead>
              <tr>
                <th>{data.group_by}</th>
                {data.columns.map((c: any) => <th className="num" key={c.id} title={c.description}>{c.name}</th>)}
              </tr>
            </thead>
            <tbody>
              {data.rows.map((r: any) => (
                <tr key={r.key}>
                  <td className="font-medium">
                    {panel.query.mode === "sql" || !DIM_TO_FIELD[panel.query.groupBy ?? ""] ? r.key : (
                      <Link to="/search" search={(p: any) => ({ from: p.from, to: p.to, q: `${DIM_TO_FIELD[panel.query.groupBy!]}:${r.key}` })} className="hover:underline">{r.key}</Link>
                    )}
                  </td>
                  {data.columns.map((c: any) => <td className="num" key={c.id}>{typeof r.values?.[c.id] === "number" ? metricText(r.values[c.id], c.unit) : String(r.values?.[c.id] ?? "–")}</td>)}
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </PanelFrame>
  );
}

function HeatPanel({ panel, ctx, height }: { panel: PanelDef; ctx: PanelContext; height: number }) {
  const { from, to } = useRange();
  const { tz } = usePrefs();
  const navigate = useNavigate();
  const { data, error } = useMetric(buildQuery(panel, from, to, tz, ctx, { max_points: 90 }));
  return (
    <PanelFrame panel={panel} ctx={ctx} height={height} info={data ? `${data.interval_label} · ${data.tz}` : undefined}>
      <div className="px-2 pb-2 h-full">
        {error ? <PanelNote error={error} /> : !data ? <Loading /> : (
          <HeatChart data={data} height={height - 46} onClick={ctx.editing ? undefined : (a, b, key) => navigate({ to: "/search", search: { from: rangeParam(a), to: rangeParam(b), q: `status:${key}` } } as any)} />
        )}
      </div>
    </PanelFrame>
  );
}

function HistogramPanel({ panel, ctx, height }: { panel: PanelDef; ctx: PanelContext; height: number }) {
  const { from, to } = useRange();
  const { tz } = usePrefs();
  const { data, error } = useMetric(buildQuery(panel, from, to, tz, ctx));
  return (
    <PanelFrame panel={panel} ctx={ctx} height={height}>
      <div className="px-2 pb-2 h-full">
        {error ? <PanelNote error={error} /> : !data ? <Loading /> : <Histogram bins={data.bins} unit={data.metric.unit} height={height - 46} />}
      </div>
    </PanelFrame>
  );
}

function CallsPanel({ panel, ctx, height }: { panel: PanelDef; ctx: PanelContext; height: number }) {
  const { from, to } = useRange();
  const { tz, refreshMs, can } = usePrefs();
  const q = [panel.query.q, ctx.varFilter].filter(Boolean).join(" ");
  const { data, error } = useQuery({
    queryKey: ["panel-calls", q, from, to],
    queryFn: () => get(`/calls/search${qs({ q, from, to, limit: 40, tz })}`),
    refetchInterval: refreshMs,
    placeholderData: keepPreviousData,
    enabled: can("search_all"),
  });
  return (
    <PanelFrame panel={panel} ctx={ctx} height={height} info={data ? `${data.total.toLocaleString()} calls` : undefined}>
      <div className="h-full overflow-auto border-t border-line">
        {!can("search_all") ? <Empty>Your account can't browse calls.</Empty> : error ? <PanelNote error={error} /> : !data ? <Loading /> : data.rows.length === 0 ? <Empty>No calls in range</Empty> : (
          <table className="data-table text-xs">
            <thead>
              <tr><th>Time</th><th>Caller</th><th>Callee</th><th>Trunk</th><th>Status</th><th className="num">Talk</th><th className="num">MOS</th><th>Findings</th></tr>
            </thead>
            <tbody>
              {data.rows.map((c: Call) => (
                <tr key={c.call_uid}>
                  <td><Link to="/calls/$uid" params={{ uid: c.call_uid }} className="hover:underline">{fmtHMS(c.start_ts, tz)}</Link></td>
                  <td className="font-mono">{c.caller_n}</td>
                  <td className="font-mono">{c.callee_n}</td>
                  <td>{c.trunk || "–"}</td>
                  <td><StatusChip status={c.final_status} reason={c.final_reason} /></td>
                  <td className="num">{fmtDuration(c.talk_ms)}</td>
                  <td className="num"><MosCell mos={c.min_mos} /></td>
                  <td className="space-x-1">{c.findings.slice(0, 2).map((f) => <FindingBadge code={f} key={f} />)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </PanelFrame>
  );
}

function FailingAorsPanel({ panel, ctx, height }: { panel: PanelDef; ctx: PanelContext; height: number }) {
  const { tz, refreshMs } = usePrefs();
  const { data, error } = useQuery({ queryKey: ["failing"], queryFn: () => get("/registrations/failing"), refetchInterval: refreshMs, placeholderData: keepPreviousData });
  return (
    <PanelFrame panel={panel} ctx={ctx} height={height}>
      <div className="h-full overflow-auto border-t border-line">
        {error ? <PanelNote error={error} /> : !data ? <Loading /> : (
          <table className="data-table text-xs">
            <thead><tr><th>Number</th><th>State</th><th>Last failure</th><th className="num">Failures</th><th>Handset</th></tr></thead>
            <tbody>
              {data.map((r: any) => (
                <tr key={r.number}>
                  <td className="font-mono"><Link to="/subscribers/$number" params={{ number: r.number }} className="hover:underline">{r.number}</Link></td>
                  <td><RegStatus status={r.status} /></td>
                  <td>{r.last_failure ? `${r.last_failure.code} at ${fmtHMS(r.last_failure.ts, tz)}` : "–"}</td>
                  <td className="num">{r.consecutive_failures}</td>
                  <td>{r.ua_vendor} {r.ua_model}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </PanelFrame>
  );
}

/** A standalone time-series panel, for the Explore page. */
export function TimePanelForExplore({ panel }: { panel: PanelDef }) {
  return <TimePanel panel={panel} ctx={{}} height={560} />;
}
