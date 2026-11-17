import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { LineChart } from "echarts/charts";
import { MarkAreaComponent } from "echarts/components";
import * as echarts from "echarts/core";
import { CheckCircle2, GraduationCap, Search, Users } from "lucide-react";
import { useMemo, useState } from "react";
import { FONT, chrome, escapeHtml, tooltipChrome, useEChart } from "../components/charts";
import { Card, Empty, ErrorNote, Loading, PageHeader, RefreshButton, Status, Tabs } from "../components/ui";
import { get } from "../lib/api";
import { usePrefs } from "../lib/context";
import { rangeParam } from "../lib/context";
import { fmtDateTime, fmtDuration, fmtHM } from "../lib/format";

echarts.use([LineChart, MarkAreaComponent]);

export interface Anomaly {
  id: number;
  dim: "all" | "trunk" | "source";
  value: string;
  label: string;
  metric: "volume" | "asr" | "fail5xx" | "pdd" | "mos" | "regfail";
  direction: "up" | "down";
  severity: "crit" | "serious" | "warn";
  current: number;
  baseline: number;
  worst: number;
  started_at: string;
  last_seen: string;
  resolved_at: string | null;
  details: { band: [number, number]; basis: string; samples: number; series: [number, number][] } | null;
}

const TITLE: Record<string, [string, string]> = {
  volume: ["Call volume dropped", "Call volume spiked"],
  asr: ["Answer rate dropped", "Answer rate rose"],
  fail5xx: ["Fewer 5xx failures", "More calls failing with 5xx"],
  pdd: ["Post-dial delay fell", "Post-dial delay went up"],
  mos: ["Voice quality dropped", "Voice quality rose"],
  regfail: ["Fewer failed registrations", "Registration failures went up"],
};

export function fmtMetric(metric: string, v: number): string {
  switch (metric) {
    case "volume":
      return `${Math.round(v).toLocaleString()} calls/h`;
    case "asr":
    case "fail5xx":
      return `${v.toFixed(v < 10 ? 1 : 0)}%`;
    case "pdd":
      return fmtDuration(Math.round(v));
    case "mos":
      return v.toFixed(2);
    case "regfail":
      return `${Math.round(v).toLocaleString()}/h`;
    default:
      return String(v);
  }
}

export function anomalyTitle(a: Anomaly) {
  return TITLE[a.metric]?.[a.direction === "up" ? 1 : 0] ?? a.metric;
}

function where(a: Anomaly) {
  if (a.dim === "all") return "all traffic";
  return `${a.dim} ${a.label}`;
}

/** The calls behind an anomaly, as a search. */
function searchFor(a: Anomaly): string {
  const parts: string[] = [];
  if (a.dim === "trunk") parts.push(`trunk:"${a.value}"`);
  if (a.dim === "source") parts.push(`source:${a.value}`);
  if (a.metric === "asr") parts.push("state:failed");
  if (a.metric === "fail5xx") parts.push("status:5xx");
  if (a.metric === "pdd") parts.push(`pdd>${Math.max(1, Math.round((a.details?.band[1] ?? 3000) / 1000))}s`);
  if (a.metric === "mos") parts.push("mos<3.6");
  return parts.join(" ");
}

const TONE: Record<string, "crit" | "serious" | "warn"> = { crit: "crit", serious: "serious", warn: "warn" };
const SEVERITY_LABEL: Record<string, string> = { crit: "Critical", serious: "Serious", warn: "Warning" };

function Spark({ a }: { a: Anomaly }) {
  const { theme, tz } = usePrefs();
  const option = useMemo(() => {
    const c = chrome(theme);
    const color = a.severity === "crit" ? "#d03b3b" : a.severity === "serious" ? "#c2611b" : "#b97c00";
    const series = a.details?.series ?? [];
    const band = a.details?.band ?? [a.baseline, a.baseline];
    return {
      animationDuration: 600,
      textStyle: { fontFamily: FONT },
      grid: { left: 4, right: 4, top: 6, bottom: 4 },
      xAxis: { type: "time", show: false },
      yAxis: { type: "value", show: false, scale: true },
      tooltip: {
        trigger: "axis",
        confine: true,
        ...tooltipChrome(c),
        formatter: (p: any[]) => `${fmtHM(p[0].value[0], tz)}<br/><b>${escapeHtml(fmtMetric(a.metric, p[0].value[1]))}</b><br/><span style="color:${c.muted}">normal ${escapeHtml(fmtMetric(a.metric, band[0]))} – ${escapeHtml(fmtMetric(a.metric, band[1]))}</span>`,
      },
      series: [
        {
          type: "line",
          showSymbol: false,
          smooth: 0.25,
          lineStyle: { width: 2, color },
          itemStyle: { color },
          areaStyle: { color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [{ offset: 0, color: `${color}40` }, { offset: 1, color: `${color}00` }]) },
          data: series,
          markArea: { silent: true, itemStyle: { color: theme === "dark" ? "rgba(79,194,127,0.12)" : "rgba(26,122,69,0.09)" }, data: [[{ yAxis: band[0] }, { yAxis: band[1] }]] },
        },
      ],
    };
  }, [a, theme, tz]);
  const { ref } = useEChart(option);
  return <div ref={ref} className="h-[64px] w-full" />;
}

export function AnomalyCard({ a, compact }: { a: Anomaly; compact?: boolean }) {
  const { tz } = usePrefs();
  const open = !a.resolved_at;
  const startMs = Date.parse(a.started_at);
  const endMs = a.resolved_at ? Date.parse(a.resolved_at) : Date.now();
  const color = a.severity === "crit" ? "var(--crit)" : a.severity === "serious" ? "var(--serious)" : "var(--warn)";
  const q = searchFor(a);
  return (
    <section className={`anomaly-card bg-surface border border-line rounded-lg overflow-hidden ${open ? "" : "opacity-80"}`} style={{ borderLeft: `4px solid ${open ? color : "var(--line-2)"}` }}>
      <div className={`grid ${compact ? "" : "md:grid-cols-[1fr_260px]"} gap-3 p-3`}>
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-x-2.5 gap-y-1">
            <Status tone={open ? TONE[a.severity] : "neutral"} live={open && a.severity === "crit"}>{open ? SEVERITY_LABEL[a.severity] : "Resolved"}</Status>
            <h3 className="font-semibold">{anomalyTitle(a)}</h3>
            <span className="text-ink-2">on {where(a)}</span>
          </div>
          <p className="mt-1.5 text-md">
            <span className="font-semibold" style={{ color: open ? color : undefined }}>{fmtMetric(a.metric, a.current)}</span>
            <span className="text-ink-2"> now, normally {fmtMetric(a.metric, a.baseline)}</span>
            {a.worst !== a.current && <span className="text-ink-3 text-sm"> · worst {fmtMetric(a.metric, a.worst)}</span>}
          </p>
          <p className="text-ink-3 text-xs mt-1">
            Since {fmtDateTime(a.started_at, tz)} · {open ? "ongoing for" : "lasted"} {fmtDuration(endMs - startMs)}
            {a.details?.basis ? ` · normal is learned from ${a.details.basis} (${a.details.samples} samples)` : ""}
          </p>
          {!compact && (
            <div className="flex flex-wrap gap-2 mt-2.5">
              {a.metric === "regfail" ? (
                <Link to="/subscribers" className="inline-flex items-center gap-1.5 text-accent hover:underline text-sm"><Users size={14} aria-hidden />Subscribers</Link>
              ) : (
                <Link
                  to="/search"
                  search={{ q: q || undefined, from: rangeParam(startMs - 30 * 60_000), to: a.resolved_at ? rangeParam(endMs) : "now", src: undefined } as any}
                  className="inline-flex items-center gap-1.5 text-accent hover:underline text-sm"
                >
                  <Search size={14} aria-hidden />
                  Show the calls
                </Link>
              )}
            </div>
          )}
        </div>
        {!compact && a.details && a.details.series.length > 1 && (
          <div className="min-w-0">
            <Spark a={a} />
            <div className="text-2xs text-ink-3 text-right">last 24 hours · green band is normal</div>
          </div>
        )}
      </div>
    </section>
  );
}

/** One line per anomaly, for Home. */
export function AnomalyRow({ a }: { a: Anomaly }) {
  const color = a.severity === "crit" ? "var(--crit)" : a.severity === "serious" ? "var(--serious)" : "var(--warn)";
  return (
    <li>
      <Link to="/anomalies" className="flex flex-wrap items-center gap-x-3 gap-y-0.5 px-3 py-2 hover:bg-hover">
        <span className="w-1 self-stretch rounded-full" style={{ background: color }} aria-hidden />
        <span className="font-medium">{anomalyTitle(a)}</span>
        <span className="text-ink-2">on {where(a)}</span>
        <span className="ml-auto tnum">
          <span className="font-semibold" style={{ color }}>{fmtMetric(a.metric, a.current)}</span>
          <span className="text-ink-3"> vs normal {fmtMetric(a.metric, a.baseline)}</span>
        </span>
      </Link>
    </li>
  );
}

export function useAnomalies(state: "open" | "recent") {
  const { refreshMs } = usePrefs();
  return useQuery<{ enabled: boolean; learned_hours: number; items: Anomaly[] }>({
    queryKey: ["anomalies", state],
    queryFn: () => get(`/anomalies?state=${state}`),
    refetchInterval: Math.max(refreshMs || 60_000, 30_000),
  });
}

export function AnomaliesPage() {
  const client = useQueryClient();
  const [tab, setTab] = useState<"open" | "recent">("open");
  const { data, error, isFetching } = useAnomalies(tab);
  const items = data?.items ?? [];

  return (
    <div>
      <PageHeader
        title="Anomalies"
        description="Traffic that is out of line with what is normal for this hour, learned from your own calls. Checked every five minutes, overall, per trunk and per source."
        right={<RefreshButton onClick={() => client.refetchQueries({ queryKey: ["anomalies"] })} busy={isFetching} />}
      />
      {data && data.learned_hours < 72 && data.enabled && (
        <div className="mb-3 flex items-start gap-2.5 rounded-lg border border-info/30 bg-info-wash px-3 py-2.5 text-info">
          <GraduationCap size={18} className="shrink-0 mt-0.5" aria-hidden />
          <div>
            <div className="font-medium">Learning your traffic: {data.learned_hours} hour{data.learned_hours === 1 ? "" : "s"} so far</div>
            <div className="text-ink-2 text-sm">
              Checks start after 12 hours of traffic, compare each hour with the same hour on previous days after 3 days, and with the same weekday after 3 weeks.
            </div>
            <div className="mt-1.5 h-1.5 w-full max-w-[420px] rounded-full bg-surface overflow-hidden">
              <div className="h-full rounded-full bg-info path-bar" style={{ width: `${Math.min(100, (data.learned_hours / 72) * 100)}%` }} />
            </div>
          </div>
        </div>
      )}
      {data && !data.enabled && <p className="mb-3 text-warn">Anomaly detection is turned off in Settings, under Anomalies.</p>}
      <Card>
        <Tabs
          tabs={[
            { id: "open", label: tab === "open" && data ? `Open (${items.length})` : "Open" },
            { id: "recent", label: "Last 7 days" },
          ]}
          active={tab}
          onChange={(t) => setTab(t as "open" | "recent")}
        />
        <div className="p-3 space-y-2.5">
          {error ? (
            <ErrorNote error={error} />
          ) : !data ? (
            <Loading work="query" label="Comparing traffic with normal…" />
          ) : items.length === 0 ? (
            <Empty>
              <CheckCircle2 size={28} className="mx-auto mb-2 text-good" aria-hidden />
              <div className="font-medium text-ink">{tab === "open" ? "Everything looks normal" : "No anomalies in the last 7 days"}</div>
              <div>Call volume, answer rate, failures, post-dial delay, voice quality and registrations are within their usual range.</div>
            </Empty>
          ) : (
            items.map((a) => <AnomalyCard key={a.id} a={a} />)
          )}
        </div>
      </Card>
    </div>
  );
}
