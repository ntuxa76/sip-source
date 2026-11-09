export interface Call {
  call_uid: string;
  state: string;
  start_ts: string;
  ring_ts: string | null;
  answer_ts: string | null;
  end_ts: string | null;
  pdd_ms: number | null;
  setup_ms: number | null;
  talk_ms: number;
  egress_pdd_ms: number | null;
  caller: string;
  caller_n: string;
  callee: string;
  callee_n: string;
  direction: string;
  backend: string;
  capture_ids: number[];
  ingress_group: string;
  egress_group: string;
  trunk: string;
  node: string;
  pbx: string;
  media_node: string;
  final_status: number;
  final_reason: string;
  egress_status: number;
  q850_cause: number;
  end_cause: string;
  hangup_side: string;
  root_call_id: string;
  b_call_id: string | null;
  codecs: string[];
  ua_caller: string;
  ua_callee: string;
  ua_vendor: string;
  ua_model: string;
  caller_ip: string;
  callee_ip: string;
  caller_country: string;
  min_mos: number | null;
  max_loss_pct: number | null;
  max_jitter_ms: number | null;
  max_rtt_ms: number | null;
  media_a_to_b: boolean;
  media_b_to_a: boolean;
  findings: string[];
  msg_count: number;
}

export interface Column {
  name: string;
  ip: string;
  role: string;
  group: string;
  capture_id: number | null;
}

export interface Sdp {
  ip: string;
  port: number;
  codecs: string[];
  direction: string;
  srtp: boolean;
}

export interface DiffRow {
  op: "added" | "removed" | "changed";
  name: string;
  old: string;
  new: string;
}

export interface Msg {
  idx: number;
  ts: string;
  leg: "A" | "B";
  call_id: string;
  src: number;
  dst: number;
  src_addr: string;
  dst_addr: string;
  transport: string;
  label: string;
  method: string;
  status: number;
  reason: string;
  cseq: number;
  cseq_method: string;
  class: string;
  retx: number;
  has_sdp: boolean;
  captures: { point: string; capture_id: number; ts: string }[];
  hop_of: number | null;
  size: number;
  q850: number;
  headers: [string, string][];
  raw: string;
  sdp: Sdp | null;
  diff: DiffRow[];
}

export interface Flow {
  columns: Column[];
  messages: Msg[];
}

export interface MetricInfo {
  id: string;
  name: string;
  description: string;
  unit: string;
  kind: string;
  better: string;
  min_samples: number;
  family: string;
  formula: string;
}

export interface TimeSeriesResult {
  shape: "timeseries";
  metric: MetricInfo;
  interval_s: number;
  interval_label: string;
  tz: string;
  source: string;
  now: number;
  data_from: number;
  buckets: number[];
  partial_from: number | null;
  total: number | null;
  samples: number;
  previous: number | null;
  series: { name: string; values: (number | null)[]; samples: number[] }[];
  sql: string;
}

export interface MetricQuery {
  metric?: string;
  metrics?: string[];
  from: string;
  to: string;
  group_by?: string;
  filter?: string;
  tz: string;
  top?: number;
  shape?: string;
  compare?: boolean;
  order?: string;
  max_points?: number;
  interval?: number;
  min_interval?: number;
  var_filter?: string;
  vars?: Record<string, string[]>;
  sql?: string;
  panel_ref?: { dashboard: string; panel: number };
  title?: string;
  unit?: string;
}

export interface RegState {
  number: string;
  aor: string;
  group: string;
  status: "registered" | "failing" | "not_registered";
  contact: {
    contact: string;
    src_ip: string;
    src_port: number;
    transport: string;
    user_agent: string;
    ua_vendor: string;
    ua_model: string;
    ua_firmware: string;
    country: string;
    nat: boolean;
    expires_at: string;
    node: string;
  } | null;
  last_success: string | null;
  last_failure: { ts: string; code: number; result: string } | null;
  consecutive_failures: number;
  failing_since: string | null;
  first_seen: string;
  ua_vendor: string;
  ua_model: string;
  src_ip: string;
  nat: boolean;
}

export class ApiError extends Error {
  constructor(
    message: string,
    public status: number,
  ) {
    super(message);
  }
}

async function handle<T>(res: Response): Promise<T> {
  if (!res.ok) {
    if (res.status === 401 && !res.url.includes("/auth/")) window.dispatchEvent(new Event("vp-signed-out"));
    const problem = await res.json().catch(() => null);
    throw new ApiError(problem?.detail ?? res.statusText, res.status);
  }
  return res.json();
}

export function get<T = any>(path: string): Promise<T> {
  return fetch(`/api/v1${path}`).then((r) => handle<T>(r));
}

function send<T>(method: string, path: string, body?: unknown): Promise<T> {
  return fetch(`/api/v1${path}`, {
    method,
    headers: { "Content-Type": "application/json", "X-VP-Request": "1" },
    body: body === undefined ? undefined : JSON.stringify(body),
  }).then((r) => handle<T>(r));
}

export const post = <T = any>(path: string, body?: unknown) => send<T>("POST", path, body ?? {});
export const put = <T = any>(path: string, body?: unknown) => send<T>("PUT", path, body ?? {});
export const del = <T = any>(path: string) => send<T>("DELETE", path);

export function qs(params: Record<string, string | number | undefined>): string {
  const p = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) {
    if (v !== undefined && v !== "") p.set(k, String(v));
  }
  const s = p.toString();
  return s ? `?${s}` : "";
}
