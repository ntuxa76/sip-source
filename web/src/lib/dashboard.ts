/** The dashboard document. One JSON value holds everything, so copy and paste recreates it. */

export interface PanelQuery {
  mode?: "metric" | "sql";
  metric?: string;
  metrics?: string[];
  groupBy?: string;
  filter?: string;
  top?: number;
  minInterval?: number;
  q?: string;
  sql?: string;
}

export interface PanelOptions {
  viz?: "line" | "area" | "stacked" | "bars";
  yMin?: number;
  yMax?: number;
  limit?: number;
  order?: string;
  legend?: boolean;
  unit?: string;
  description?: string;
  markdown?: string;
}

export interface PanelDef {
  id: number;
  type: string;
  title: string;
  gridPos: { x: number; y: number; w: number; h: number };
  query: PanelQuery;
  options: PanelOptions;
}

export interface Variable {
  name: string;
  label: string;
  type: "field";
  field: string;
  multi: boolean;
}

export interface Dashboard {
  schemaVersion: number;
  uid?: string;
  title: string;
  description: string;
  tags: string[];
  time: { from: string; to: string };
  refresh: string;
  sharedCrosshair: boolean | null;
  variables: Variable[];
  panels: PanelDef[];
}

export const GRID_COLS = 24;
export const ROW_HEIGHT = 22;
export const GRID_MARGIN = 10;

export function panelHeightPx(h: number): number {
  return h * ROW_HEIGHT + (h - 1) * GRID_MARGIN;
}

export const PANEL_TYPES = [
  { value: "timeseries", label: "Time series" },
  { value: "stat", label: "Stat" },
  { value: "top", label: "Bar list (top N)" },
  { value: "table", label: "Table" },
  { value: "heatmap", label: "Heatmap" },
  { value: "histogram", label: "Histogram" },
  { value: "calls", label: "Call list" },
  { value: "failing_aors", label: "Failing AoRs" },
  { value: "text", label: "Text" },
];

export function usesMetric(type: string): boolean {
  return ["timeseries", "stat", "top", "heatmap", "histogram", "table"].includes(type);
}

export function supportsSql(type: string): boolean {
  return ["timeseries", "stat", "top", "table"].includes(type);
}

export function emptyDashboard(title: string): Dashboard {
  return {
    schemaVersion: 1,
    title,
    description: "",
    tags: [],
    time: { from: "now-6h", to: "now" },
    refresh: "30s",
    sharedCrosshair: null,
    variables: [{ name: "capture_id", label: "Capture ID", type: "field", field: "capture", multi: true }],
    panels: [],
  };
}

export function nextPanelId(d: Dashboard): number {
  return d.panels.reduce((m, p) => Math.max(m, p.id), 0) + 1;
}

export function newPanel(d: Dashboard): PanelDef {
  const bottom = d.panels.reduce((m, p) => Math.max(m, p.gridPos.y + p.gridPos.h), 0);
  return {
    id: nextPanelId(d),
    type: "timeseries",
    title: "New panel",
    gridPos: { x: 0, y: bottom, w: 12, h: 9 },
    query: { mode: "metric", metric: "attempts", groupBy: "direction" },
    options: { viz: "line" },
  };
}

export function parseRefresh(r: string): number | false {
  const m = /^(\d+)(s|m|h)$/.exec(r ?? "");
  if (!m) return false;
  return Number(m[1]) * { s: 1000, m: 60000, h: 3600000 }[m[2] as "s" | "m" | "h"];
}

export const VARIABLE_FIELDS = [
  { value: "capture", label: "Capture ID" },
  { value: "backend", label: "Backend type" },
  { value: "trunk", label: "Trunk" },
  { value: "direction", label: "Direction" },
  { value: "node", label: "SIP node" },
  { value: "pbx", label: "PBX" },
  { value: "group", label: "Ingress group" },
  { value: "codec", label: "Codec" },
  { value: "vendor", label: "UA vendor" },
  { value: "country", label: "Country" },
];
