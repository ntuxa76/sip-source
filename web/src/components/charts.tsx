import { BarChart, HeatmapChart, LineChart } from "echarts/charts";
import { AxisPointerComponent, GridComponent, MarkAreaComponent, TooltipComponent, VisualMapComponent } from "echarts/components";
import * as echarts from "echarts/core";
import { CanvasRenderer } from "echarts/renderers";
import { useEffect, useMemo, useRef, useState } from "react";
import type { TimeSeriesResult } from "../lib/api";
import { usePrefs } from "../lib/context";
import { compact, fmtHM, fmtHMS, fmtMonthDay, metricText, tzOffsetMs } from "../lib/format";

echarts.use([LineChart, BarChart, HeatmapChart, GridComponent, TooltipComponent, AxisPointerComponent, MarkAreaComponent, VisualMapComponent, CanvasRenderer]);

// The validated categorical palette, one step per theme (CVD-checked in this order, light and
// dark). It leads with the brand blue. Slots are assigned by value, never rank.
const PALETTE = {
  light: ["#2a78d6", "#eb6834", "#1baf7a", "#eda100", "#e87ba4", "#008300", "#6250d6", "#e34948"],
  dark: ["#3987e5", "#d95926", "#199e70", "#c98500", "#d55181", "#008300", "#9085e9", "#e66767"],
};
const SLOT = { blue: 0, orange: 1, aqua: 2, yellow: 3, magenta: 4, green: 5, violet: 6, red: 7 };

const KNOWN_SLOTS: Record<string, number> = {};
for (const group of [
  ["Carrier-X", "Carrier-Y", "Mobile-A", "Mobile-B", "Intl-Transit"],
  ["outbound", "inbound", "onnet", "transit"],
  ["opensips-1", "opensips-2", "asterisk-1", "asterisk-2", "asterisk-3", "rtpengine-1", "rtpengine-2", "heplify-dmz"],
  ["FTTH Baghdad", "FTTH Basra", "FTTH Erbil", "FTTH Mosul"],
  ["INVITE", "ACK", "BYE", "CANCEL", "REGISTER", "OPTIONS"],
  ["overload", "bad_packet", "auth", "filter", "duplicate"],
]) {
  group.forEach((v, i) => (KNOWN_SLOTS[v] = i));
}

const CODE_SHADES: Record<string, string[]> = {
  "4": ["#eda100", "#b97c00", "#f5c451", "#8a5a00"],
  "5": ["#e34948", "#b02a2a", "#f08a89", "#7d1f1f"],
  "6": ["#6250d6", "#9085e9", "#4a3aa7", "#b3aaf0"],
};

function hashSlot(s: string): number {
  let h = 0;
  for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) >>> 0;
  return h % 8;
}

export function seriesColor(name: string, theme: "light" | "dark"): string {
  const p = PALETTE[theme];
  // SIP classes keep fixed meanings on every chart (FR-CH-15). Exact codes stay in their class
  // hue but step through shades, so 401 and 403 can still be told apart.
  const code = /^([1-6])(xx|\d\d)$/.exec(name);
  if (code) {
    const base = { "1": "#898781", "2": p[SLOT.green], "3": p[SLOT.blue], "4": p[SLOT.yellow], "5": p[SLOT.red], "6": p[SLOT.violet] }[code[1]]!;
    if (code[2] === "xx") return base;
    return CODE_SHADES[code[1]]?.[Number(name) % 4] ?? base;
  }
  if (name === "other") return "#898781";
  if (name === "value") return p[0];
  return p[KNOWN_SLOTS[name] ?? hashSlot(name)];
}

// The canvas cannot read CSS variables, so these repeat the tokens of index.css. Keep the two in step.
export function chrome(theme: "light" | "dark") {
  return theme === "dark"
    ? { surface: "#15181c", ink: "#e7eaee", ink2: "#b4bbc5", muted: "#8b939f", grid: "#262b31", axis: "#373e47", raised: "#1b1f24", shadow: "rgba(0,0,0,0.5)" }
    : { surface: "#ffffff", ink: "#15181c", ink2: "#47505c", muted: "#5f6979", grid: "#e4e7eb", axis: "#cdd3da", raised: "#ffffff", shadow: "rgba(21,24,28,0.12)" };
}

/** The two directions of a media stream: caller to callee, then callee to caller. */
export const streamColors = (theme: "light" | "dark") => [PALETTE[theme][SLOT.blue], PALETTE[theme][SLOT.orange]];

/** MOS bands and the sequential ramp of the heatmap, per theme. */
const QUALITY = {
  light: { good: "#1a7a45", fair: "#b97c00", poor: "#ad4a17", bad: "#c0282f" },
  dark: { good: "#4fc27f", fair: "#f0b93f", poor: "#ec835a", bad: "#ef7377" },
};
const HEAT = {
  light: ["#e8f0fa", "#9dc0ea", "#3d7fcb", "#123f7a"],
  dark: ["#182640", "#285a9c", "#6ea8f5", "#cfe2fb"],
};

function withAlpha(hex: string, a: number) {
  const n = parseInt(hex.slice(1), 16);
  return `rgba(${(n >> 16) & 255},${(n >> 8) & 255},${n & 255},${a})`;
}

export function tooltipChrome(c: ReturnType<typeof chrome>) {
  return {
    backgroundColor: c.raised,
    borderColor: c.axis,
    borderWidth: 1,
    padding: [7, 9],
    extraCssText: `border-radius:3px;box-shadow:0 4px 12px ${c.shadow};`,
    textStyle: { color: c.ink, fontSize: 12, fontFamily: FONT },
  };
}

export const FONT = "'IBM Plex Sans Variable', 'IBM Plex Sans', system-ui, sans-serif";
const MONO = "'IBM Plex Mono', ui-monospace, monospace";

export function useEChart(option: any, group?: string, onReady?: (c: echarts.ECharts) => void) {
  const ref = useRef<HTMLDivElement>(null);
  const chart = useRef<echarts.ECharts | null>(null);
  const ready = useRef(onReady);
  ready.current = onReady;

  useEffect(() => {
    if (!ref.current) return;
    const c = echarts.init(ref.current, undefined, { renderer: "canvas" });
    chart.current = c;
    if (group) {
      c.group = group;
      echarts.connect(group);
    }
    ready.current?.(c);
    const ro = new ResizeObserver(() => c.resize());
    ro.observe(ref.current);
    return () => {
      ro.disconnect();
      c.dispose();
      chart.current = null;
    };
  }, [group]);

  useEffect(() => {
    chart.current?.setOption(option, { notMerge: true, lazyUpdate: true });
  }, [option]);

  return { ref, chart };
}

const TICK_STEPS = [60e3, 300e3, 600e3, 900e3, 1800e3, 3600e3, 3 * 3600e3, 6 * 3600e3, 12 * 3600e3, 864e5, 2 * 864e5];

function tickLabeller(buckets: number[], tz: string, width: number) {
  const span = buckets.length ? buckets[buckets.length - 1] - buckets[0] : 0;
  const want = Math.max(2, Math.floor(width / 92));
  const step = TICK_STEPS.find((s) => span / s <= want) ?? 864e5 * 7;
  const offset = buckets.length ? tzOffsetMs(buckets[0], tz) : 0;
  const isTick = (ms: number) => (ms + offset) % step === 0;
  const dayStart = (ms: number) => (ms + offset) % 864e5 === 0;
  const label = (ms: number) => {
    if (span < 15 * 60e3) return fmtHMS(ms, tz);
    if (span < 2 * 864e5) return dayStart(ms) ? `${fmtMonthDay(ms, tz)}` : fmtHM(ms, tz);
    if (span < 14 * 864e5) return dayStart(ms) ? fmtMonthDay(ms, tz) : fmtHM(ms, tz);
    return fmtMonthDay(ms, tz);
  };
  return { isTick, label };
}

export interface ChartEvents {
  onZoom?: (fromMs: number, toMs: number) => void;
  onResetZoom?: () => void;
  onClick?: (bucketMs: number, bucketEndMs: number, series: string | null) => void;
}

export type Viz = "line" | "area" | "stacked" | "bars";

/** The one time-series chart in the product: fixed axes, partial-bucket and low-sample rules. */
export function TimeChart({ data, viz, height, group, yMin, yMax, events, legend = true }: { data: TimeSeriesResult; viz: Viz; height: number; group?: string; yMin?: number; yMax?: number; events?: ChartEvents; legend?: boolean }) {
  const { theme, tz } = usePrefs();
  const [hidden, setHidden] = useState<Set<string>>(new Set());
  const [width, setWidth] = useState(600);
  const zoomBox = useRef<HTMLDivElement>(null);
  const handlers = useRef(events);
  handlers.current = events;
  const dataRef = useRef(data);
  dataRef.current = data;

  const option = useMemo(() => buildTimeOption(data, viz, theme, tz, width, hidden, yMin, yMax), [data, viz, theme, tz, width, hidden, yMin, yMax]);

  const { ref } = useEChart(option, group, (chart) => {
    const dom = chart.getDom();
    let start: number | null = null;
    let seriesClick = false;

    const bucketAt = (x: number) => {
      const idx = chart.convertFromPixel({ xAxisIndex: 0 }, x) as unknown as number;
      const b = dataRef.current.buckets;
      return Math.max(0, Math.min(b.length - 1, Math.round(idx)));
    };

    dom.addEventListener("pointerdown", (e) => {
      if (e.button === 0) start = e.offsetX;
    });
    dom.addEventListener("pointermove", (e) => {
      if (start === null || Math.abs(e.offsetX - start) < 6 || !zoomBox.current) return;
      zoomBox.current.style.display = "block";
      zoomBox.current.style.left = `${Math.min(start, e.offsetX)}px`;
      zoomBox.current.style.width = `${Math.abs(e.offsetX - start)}px`;
    });
    window.addEventListener("pointerup", (e) => {
      if (start === null) return;
      const rect = dom.getBoundingClientRect();
      const end = e.clientX - rect.left;
      if (zoomBox.current) zoomBox.current.style.display = "none";
      if (Math.abs(end - start) >= 6) {
        const d = dataRef.current;
        const [a, b] = [bucketAt(Math.min(start, end)), bucketAt(Math.max(start, end))];
        handlers.current?.onZoom?.(d.buckets[a], d.buckets[b] + d.interval_s * 1000);
      }
      start = null;
    });
    dom.addEventListener("dblclick", () => handlers.current?.onResetZoom?.());

    chart.on("click", (p: any) => {
      seriesClick = true;
      const d = dataRef.current;
      const ms = d.buckets[p.dataIndex];
      if (ms !== undefined) handlers.current?.onClick?.(ms, ms + d.interval_s * 1000, p.seriesName === "value" ? null : p.seriesName);
    });
    chart.getZr().on("click", (e: any) => {
      setTimeout(() => {
        if (!seriesClick) {
          const d = dataRef.current;
          const ms = d.buckets[bucketAt(e.offsetX)];
          if (ms !== undefined) handlers.current?.onClick?.(ms, ms + d.interval_s * 1000, null);
        }
        seriesClick = false;
      }, 0);
    });
    new ResizeObserver(() => setWidth(dom.clientWidth)).observe(dom);
  });

  const allNames = data.series.map((s) => s.name);
  const multi = data.series.length > 1 && legend;

  return (
    <div>
      <div className="relative" style={{ height }}>
        <div ref={ref} className="absolute inset-0" />
        <div ref={zoomBox} className="absolute top-0 bottom-5 pointer-events-none bg-accent/10 border-x border-accent hidden" />
      </div>
      {multi && (
        <Legend
          data={data}
          hidden={hidden}
          onToggle={(name, isolate) =>
            setHidden((prev) => {
              if (isolate) {
                const only = prev.size === allNames.length - 1 && !prev.has(name);
                return only ? new Set() : new Set(allNames.filter((n) => n !== name));
              }
              const next = new Set(prev);
              if (next.has(name)) next.delete(name);
              else next.add(name);
              return next;
            })
          }
        />
      )}
    </div>
  );
}

function Legend({ data, hidden, onToggle }: { data: TimeSeriesResult; hidden: Set<string>; onToggle: (name: string, isolate: boolean) => void }) {
  const { theme } = usePrefs();
  const settled = data.partial_from ?? data.buckets.length;
  const unit = data.metric.unit;
  return (
    <div className="flex flex-wrap gap-x-3.5 gap-y-0.5 px-1 pt-1 max-h-[42px] overflow-y-auto">
      {data.series.map((s) => {
        const last = [...s.values.slice(0, settled)].reverse().find((v) => v !== null && v !== undefined);
        const off = hidden.has(s.name);
        return (
          <button
            key={s.name}
            type="button"
            title="Click to toggle, Alt+click to isolate"
            onClick={(e) => onToggle(s.name, e.altKey)}
            className={`flex items-center gap-1.5 text-xs leading-[18px] ${off ? "opacity-45" : ""}`}
          >
            <span className="inline-block w-2 h-2 rounded-xs" style={{ background: seriesColor(s.name, theme) }} />
            <span className="text-ink-2">{s.name}</span>
            <span className="text-ink-3 tnum">{metricText(last, unit)}</span>
          </button>
        );
      })}
    </div>
  );
}

function buildTimeOption(data: TimeSeriesResult, viz: Viz, theme: "light" | "dark", tz: string, width: number, hidden: Set<string>, yMin?: number, yMax?: number) {
  const c = chrome(theme);
  const { isTick, label } = tickLabeller(data.buckets, tz, width);
  const categories = data.buckets.map(String);
  const unit = data.metric.unit;
  const partial = data.partial_from;
  const isBars = viz === "stacked" || viz === "bars";
  const stack = viz === "stacked" || (viz === "area" && data.series.length > 1) ? "total" : undefined;
  const series: any[] = [];

  for (const s of data.series) {
    if (hidden.has(s.name)) continue;
    const color = seriesColor(s.name, theme);
    if (isBars) {
      series.push({
        name: s.name,
        type: "bar",
        stack,
        barMaxWidth: 24,
        itemStyle: { color, borderColor: c.surface, borderWidth: data.buckets.length > 150 ? 0 : 1, },
        emphasis: { disabled: true },
        data: s.values.map((v, i) =>
          partial !== null && i >= partial
            ? { value: v, itemStyle: { color, opacity: 0.55, decal: { symbol: "rect", dashArrayX: [1, 0], dashArrayY: [2, 3], rotation: -Math.PI / 4, color: c.surface } } }
            : v,
        ),
        animation: false,
      });
      continue;
    }
    const lowSample = (i: number) => data.metric.min_samples > 0 && s.samples[i] > 0 && s.samples[i] < data.metric.min_samples;
    const settled = s.values.map((v, i) => {
      if (partial !== null && i >= partial) return null;
      return lowSample(i) ? { value: v, symbol: "circle", symbolSize: 4, itemStyle: { color: c.muted } } : v;
    });
    const tail = s.values.map((v, i) => (partial !== null && i >= partial - 1 ? v : null));
    const base = { name: s.name, type: "line", showSymbol: false, symbolSize: 6, connectNulls: false, stack, animation: false, emphasis: { disabled: true } };
    // Flat fills only: bands in a stacked area, a light wash under a lone line.
    const area = viz === "area" ? { color, opacity: stack ? 0.3 : 0.14 } : data.series.length === 1 ? { color: withAlpha(color, 0.08) } : undefined;
    series.push({
      ...base,
      data: settled,
      lineStyle: { width: 1.5, color, join: "round" },
      itemStyle: { color, borderColor: c.surface, borderWidth: 1 },
      areaStyle: area,
    });
    if (partial !== null) {
      series.push({ ...base, stack: stack ? "tail" : undefined, data: tail, lineStyle: { width: 1.5, color, type: [4, 3] }, itemStyle: { color } });
    }
  }

  return {
    animation: false,
    textStyle: { fontFamily: FONT },
    grid: { left: 46, right: 12, top: 8, bottom: 22 },
    xAxis: {
      type: "category",
      data: categories,
      boundaryGap: isBars,
      axisTick: { show: false },
      axisLine: { lineStyle: { color: c.axis } },
      axisLabel: {
        color: c.muted,
        fontSize: 11,
        interval: (i: number) => isTick(data.buckets[i]),
        formatter: (v: string) => label(Number(v)),
        hideOverlap: true,
      },
      axisPointer: { type: "line", lineStyle: { color: c.muted, width: 1, type: "solid" } },
    },
    yAxis: {
      type: "value",
      min: yMin,
      max: yMax,
      interval: yMax !== undefined ? (yMax - (yMin ?? 0)) / 4 : undefined,
      splitNumber: 3,
      axisLine: { show: false },
      axisTick: { show: false },
      splitLine: { lineStyle: { color: c.grid, width: 1 } },
      axisLabel: { color: c.muted, fontSize: 11, formatter: (v: number) => axisValue(v, unit) },
    },
    tooltip: {
      trigger: "axis",
      confine: true,
      transitionDuration: 0,
      ...tooltipChrome(c),
      formatter: (params: any[]) => tooltipHtml(params, data, theme, tz),
    },
    series,
  };
}

function axisValue(v: number, unit: string): string {
  if (unit === "ms") return v >= 1000 ? `${compact(v / 1000)} s` : `${compact(v)}`;
  if (unit === "%") return `${compact(v)}%`;
  return compact(v);
}

function escapeHtml(s: string) {
  return s.replace(/[&<>"]/g, (ch) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[ch]!);
}

function tooltipHtml(params: any[], data: TimeSeriesResult, theme: "light" | "dark", tz: string): string {
  const c = chrome(theme);
  const i = params[0]?.dataIndex;
  if (i === undefined) return "";
  const start = data.buckets[i];
  const end = start + data.interval_s * 1000;
  const fmt = data.interval_s < 60 ? fmtHMS : fmtHM;
  const rows = data.series
    .map((s) => ({ name: s.name, v: s.values[i], n: s.samples[i] }))
    .filter((r) => r.v !== null && r.v !== undefined && params.some((p) => p.seriesName === r.name))
    .sort((a, b) => (b.v as number) - (a.v as number));
  const partial = data.partial_from !== null && i >= data.partial_from;
  const single = data.series.length === 1;
  const body = rows.length
    ? rows
        .map((r) => {
          const low = data.metric.min_samples > 0 && r.n < data.metric.min_samples;
          const sample = ["ratio", "latency", "quality"].includes(data.metric.kind) && data.metric.family !== "synth" ? `<span style="color:${c.muted}"> n=${r.n}${low ? " · low sample" : ""}</span>` : "";
          return `<div style="display:flex;justify-content:space-between;gap:18px"><span><i style="display:inline-block;width:8px;height:8px;border-radius:1px;margin-right:7px;background:${seriesColor(r.name, theme)}"></i>${single ? escapeHtml(data.metric.name) : escapeHtml(r.name)}</span><span style="font-variant-numeric:tabular-nums"><b style="font-weight:600">${metricText(r.v, data.metric.unit)}</b>${sample}</span></div>`;
        })
        .join("")
    : `<div style="color:${c.muted}">No samples in this bucket</div>`;
  return `<div style="color:${c.ink2};font-size:11px;font-weight:500;margin-bottom:5px">${fmtMonthDay(start, tz)} ${fmt(start, tz)} – ${fmt(end, tz)}${partial ? " · still settling" : ""}</div>${body}`;
}

/** The small bar strip under a stat: grey history, with the latest settled bucket in the accent colour. */
export function MiniBars({ values, unit, count = 12, height = 30 }: { values: (number | null)[]; unit: string; count?: number; height?: number }) {
  const size = Math.max(1, Math.ceil(values.length / count));
  const bars: (number | null)[] = [];
  for (let i = 0; i < values.length; i += size) {
    const chunk = values.slice(i, i + size).filter((v): v is number => v !== null && v !== undefined);
    bars.push(chunk.length ? chunk.reduce((x, y) => x + y, 0) / chunk.length : null);
  }
  const max = Math.max(0, ...bars.map((v) => v ?? 0));
  if (bars.length === 0) return <div style={{ height }} />;
  return (
    <div className="flex items-end gap-[3px]" style={{ height }} aria-hidden>
      {bars.map((v, i) => {
        const pct = v === null || max === 0 ? 0 : (v / max) * 100;
        const last = i === bars.length - 1;
        return <div key={i} title={metricText(v, unit)} className={`flex-1 ${last ? "bg-accent" : "bg-line-2"}`} style={{ height: `${Math.max(pct, 6)}%` }} />;
      })}
    </div>
  );
}

export function BarList({ rows, unit, color, onClick }: { rows: { key: string; value: number; samples?: number }[]; unit: string; color?: string; onClick?: (key: string) => void }) {
  const max = Math.max(...rows.map((r) => r.value), 1e-9);
  return (
    <div className="px-3 pb-3 space-y-1.5">
      {rows.map((r) => (
        <button key={r.key} type="button" disabled={!onClick} onClick={() => onClick?.(r.key)} className="w-full text-left group block disabled:cursor-default">
          <div className="flex justify-between gap-2 text-xs">
            <span className="truncate text-ink-2 group-hover:text-ink">{r.key}</span>
            <span className="tnum text-ink font-medium">{metricText(r.value, unit)}</span>
          </div>
          <div className="h-1 mt-1 bg-sunken">
            <div className="h-1" style={{ width: `${Math.max((r.value / max) * 100, 1.5)}%`, background: color ?? "var(--accent)" }} />
          </div>
        </button>
      ))}
    </div>
  );
}

export function HeatChart({ data, height, onClick }: { data: any; height: number; onClick?: (bucketMs: number, endMs: number, key: string) => void }) {
  const { theme, tz } = usePrefs();
  const c = chrome(theme);
  const handlers = useRef(onClick);
  handlers.current = onClick;
  const dataRef = useRef(data);
  dataRef.current = data;
  const [width, setWidth] = useState(800);
  const option = useMemo(() => {
    const { isTick, label } = tickLabeller(data.buckets, tz, width);
    const max = Math.max(1, ...data.cells.map((x: number[]) => x[2]));
    return {
      animation: false,
      textStyle: { fontFamily: FONT },
      grid: { left: 46, right: 10, top: 6, bottom: 20 },
      xAxis: { type: "category", data: data.buckets.map(String), axisTick: { show: false }, axisLine: { lineStyle: { color: c.axis } }, splitArea: { show: false }, axisLabel: { color: c.muted, fontSize: 11, interval: (i: number) => isTick(data.buckets[i]), formatter: (v: string) => label(Number(v)), hideOverlap: true } },
      yAxis: { type: "category", data: data.keys, axisTick: { show: false }, axisLine: { show: false }, axisLabel: { color: c.ink2, fontSize: 11, fontFamily: MONO } },
      visualMap: { show: false, min: 0, max, inRange: { color: HEAT[theme] } },
      tooltip: {
        confine: true,
        ...tooltipChrome(c),
        formatter: (p: any) => `${fmtMonthDay(data.buckets[p.value[0]], tz)} ${fmtHM(data.buckets[p.value[0]], tz)}<br/>Status <b>${data.keys[p.value[1]]}</b>: <b>${p.value[2]}</b> calls`,
      },
      series: [{ type: "heatmap", data: data.cells, itemStyle: { borderColor: c.surface, borderWidth: 1 }, emphasis: { disabled: true } }],
    };
  }, [data, theme, tz, width, c.axis, c.muted, c.ink2, c.raised, c.ink, c.surface]);
  const { ref } = useEChart(option, undefined, (chart) => {
    chart.on("click", (p: any) => {
      const d = dataRef.current;
      const ms = d.buckets[p.value[0]];
      handlers.current?.(ms, ms + d.interval_s * 1000, d.keys[p.value[1]]);
    });
    new ResizeObserver(() => setWidth(chart.getDom().clientWidth)).observe(chart.getDom());
  });
  return <div ref={ref} style={{ height }} />;
}

export function Histogram({ bins, unit, height }: { bins: { from: number; to: number; count: number }[]; unit: string; height: number }) {
  const { theme } = usePrefs();
  const c = chrome(theme);
  const option = useMemo(
    () => ({
      animation: false,
      textStyle: { fontFamily: FONT },
      grid: { left: 40, right: 8, top: 8, bottom: 20 },
      xAxis: { type: "category", data: bins.map((b) => b.from.toFixed(unit === "mos" ? 2 : 0)), axisTick: { show: false }, axisLine: { lineStyle: { color: c.axis } }, axisLabel: { color: c.muted, fontSize: 11, hideOverlap: true, interval: 3 } },
      yAxis: { type: "value", splitNumber: 3, axisLine: { show: false }, axisTick: { show: false }, splitLine: { lineStyle: { color: c.grid } }, axisLabel: { color: c.muted, fontSize: 11, formatter: compact } },
      tooltip: { trigger: "axis", ...tooltipChrome(c), formatter: (p: any[]) => { const b = bins[p[0].dataIndex]; return `MOS ${b.from.toFixed(2)}–${b.to.toFixed(2)}: <b>${b.count.toLocaleString()}</b> calls`; } },
      series: [{ type: "bar", barMaxWidth: 24, data: bins.map((b) => ({ value: b.count, itemStyle: { color: mosBarColor(b.from, unit, theme) } })) }],
    }),
    [bins, unit, theme, c.axis, c.muted, c.grid, c.raised, c.ink],
  );
  const { ref } = useEChart(option);
  return <div ref={ref} style={{ height }} />;
}

function mosBarColor(from: number, unit: string, theme: "light" | "dark") {
  if (unit !== "mos") return PALETTE[theme][0];
  const q = QUALITY[theme];
  if (from >= 4.0) return q.good;
  if (from >= 3.6) return q.fair;
  if (from >= 3.1) return q.poor;
  return q.bad;
}

export function HistogramStrip({ histogram, height = 70, onZoom }: { histogram: any; height?: number; onZoom?: (from: number, to: number) => void }) {
  const { theme, tz } = usePrefs();
  const c = chrome(theme);
  const handlers = useRef(onZoom);
  handlers.current = onZoom;
  const dataRef = useRef(histogram);
  dataRef.current = histogram;
  const box = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(900);
  const option = useMemo(() => {
    const { isTick, label } = tickLabeller(histogram.buckets, tz, width);
    const colors: Record<string, string> = { "2xx": seriesColor("2xx", theme), "3xx": seriesColor("3xx", theme), "4xx": seriesColor("4xx", theme), "5xx": seriesColor("5xx", theme), "6xx": seriesColor("6xx", theme), "in progress": seriesColor("1xx", theme) };
    return {
      animation: false,
      textStyle: { fontFamily: FONT },
      grid: { left: 36, right: 6, top: 4, bottom: 18 },
      xAxis: { type: "category", data: histogram.buckets.map(String), axisTick: { show: false }, axisLine: { lineStyle: { color: c.axis } }, axisLabel: { color: c.muted, fontSize: 11, interval: (i: number) => isTick(histogram.buckets[i]), formatter: (v: string) => label(Number(v)), hideOverlap: true } },
      yAxis: { type: "value", splitNumber: 2, axisLine: { show: false }, axisTick: { show: false }, splitLine: { lineStyle: { color: c.grid } }, axisLabel: { color: c.muted, fontSize: 11, formatter: compact } },
      tooltip: {
        trigger: "axis",
        confine: true,
        ...tooltipChrome(c),
        formatter: (p: any[]) => `${fmtHMS(histogram.buckets[p[0].dataIndex], tz)}<br/>${p.filter((x) => x.value).map((x) => `${x.seriesName}: <b>${x.value}</b>`).join("<br/>") || "No calls"}`,
      },
      series: histogram.series.map((s: any) => ({ name: s.name, type: "bar", stack: "t", barMaxWidth: 24, itemStyle: { color: colors[s.name] }, data: s.values, emphasis: { disabled: true } })),
    };
  }, [histogram, theme, tz, width, c.axis, c.muted, c.grid, c.raised, c.ink]);
  const { ref } = useEChart(option, undefined, (chart) => {
    const dom = chart.getDom();
    let start: number | null = null;
    const idx = (x: number) => {
      const b = dataRef.current.buckets;
      return Math.max(0, Math.min(b.length - 1, Math.round(chart.convertFromPixel({ xAxisIndex: 0 }, x) as unknown as number)));
    };
    dom.addEventListener("pointerdown", (e) => (start = e.offsetX));
    dom.addEventListener("pointermove", (e) => {
      if (start === null || Math.abs(e.offsetX - start) < 6 || !box.current) return;
      box.current.style.display = "block";
      box.current.style.left = `${Math.min(start, e.offsetX)}px`;
      box.current.style.width = `${Math.abs(e.offsetX - start)}px`;
    });
    window.addEventListener("pointerup", (e) => {
      if (start === null) return;
      const end = e.clientX - dom.getBoundingClientRect().left;
      if (box.current) box.current.style.display = "none";
      if (Math.abs(end - start) >= 6) {
        const h = dataRef.current;
        handlers.current?.(h.buckets[idx(Math.min(start, end))], h.buckets[idx(Math.max(start, end))] + h.interval_s * 1000);
      }
      start = null;
    });
    new ResizeObserver(() => setWidth(dom.clientWidth)).observe(dom);
  });
  return (
    <div className="relative" style={{ height }}>
      <div ref={ref} className="absolute inset-0" />
      <div ref={box} className="absolute top-0 bottom-4 pointer-events-none bg-accent/10 border-x border-accent hidden" />
    </div>
  );
}
