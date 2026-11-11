import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, Navigate, useNavigate, useSearch } from "@tanstack/react-router";
import { Braces, ChevronDown, Copy, Download, FileUp, History, Maximize2, Pencil, Plus, RotateCcw, Save, Settings2, Trash2, X } from "lucide-react";
import { Suspense, lazy, useCallback, useEffect, useMemo, useRef, useState } from "react";
import GridLayout, { useContainerWidth } from "react-grid-layout";
import { PanelView, TimePanelForExplore } from "../components/Panel";
import type { PanelContext } from "../components/Panel";
import { Button, Card, Dialog, Empty, ErrorNote, Field, IconButton, Loading, PageHeader, Select, Tabs, fieldClass, inputClass, popoverClass, useDismiss } from "../components/ui";
import { del, get, post, put } from "../lib/api";
import type { MetricInfo } from "../lib/api";
import { homePath } from "../lib/access";
import { usePrefs, useRange } from "../lib/context";
import { GRID_COLS, GRID_MARGIN, ROW_HEIGHT, VARIABLE_FIELDS, emptyDashboard, newPanel, nextPanelId, panelHeightPx, parseRefresh } from "../lib/dashboard";
import type { Dashboard, PanelDef, Variable } from "../lib/dashboard";
import { downloadBlob, fmtDateTime } from "../lib/format";

const PanelEditor = lazy(() => import("./PanelEditor"));

const input = inputClass;

export function DashboardList() {
  const { can } = usePrefs();
  const client = useQueryClient();
  const navigate = useNavigate();
  const { data, error } = useQuery({ queryKey: ["dashboards"], queryFn: () => get("/dashboards") });
  const [importing, setImporting] = useState(false);
  const [problem, setProblem] = useState("");
  const refresh = () => client.invalidateQueries({ queryKey: ["dashboards"] });

  const create = useMutation({
    mutationFn: () => post("/dashboards", { dashboard: emptyDashboard("New dashboard") }),
    onSuccess: (r) => navigate({ to: "/dashboards/$id", params: { id: r.uid }, search: { edit: "1" } } as any),
    onError: (e: Error) => setProblem(e.message),
  });
  const duplicate = useMutation({
    mutationFn: async (uid: string) => {
      const { dashboard } = await get(`/dashboards/${uid}`);
      const { uid: _drop, ...rest } = dashboard;
      return post("/dashboards", { dashboard: { ...rest, title: `${dashboard.title} copy` } });
    },
    onSuccess: refresh,
    onError: (e: Error) => setProblem(e.message),
  });
  const remove = useMutation({ mutationFn: (uid: string) => del(`/dashboards/${uid}`), onSuccess: refresh, onError: (e: Error) => setProblem(e.message) });
  const restore = useMutation({ mutationFn: () => post("/dashboards/restore-defaults"), onSuccess: refresh, onError: (e: Error) => setProblem(e.message) });

  const copyJson = async (uid: string) => {
    const { dashboard } = await get(`/dashboards/${uid}`);
    await navigator.clipboard?.writeText(JSON.stringify(dashboard, null, 2));
  };

  if (error) return <ErrorNote error={error} />;
  return (
    <div>
      <PageHeader
        title="Dashboards"
        description="Every dashboard is one JSON document: copy it to move a whole dashboard, paste it to import one."
        right={can("dashboards_edit") && (
          <div className="flex flex-wrap gap-2">
            <Button onClick={() => restore.mutate()} title="Bring back built-in dashboards that were deleted"><RotateCcw size={14} />Restore built-in</Button>
            <Button onClick={() => setImporting(true)}><FileUp size={14} />Import JSON</Button>
            <Button variant="primary" onClick={() => create.mutate()}><Plus size={14} />New dashboard</Button>
          </div>
        )}
      />
      {problem && <p className="text-crit mb-2">{problem}</p>}
      <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-4 gap-3">
        {(data ?? []).map((d: any) => (
          <div key={d.uid} className="group bg-surface border border-line rounded-lg p-3 hover:border-line-2 flex flex-col">
            <Link to="/dashboards/$id" params={{ id: d.uid }} search={(p: any) => ({ from: p.from, to: p.to })} className="flex-1">
              <div className="font-semibold group-hover:text-accent">{d.title}</div>
              <p className="text-ink-2 text-xs mt-0.5">{d.description}</p>
            </Link>
            <div className="flex items-center justify-between mt-2">
              <span className="text-ink-3 text-xs tnum">{d.panels} panels · v{d.version}</span>
              <span className="flex -mr-1.5 -mb-1">
                <IconButton label={`Copy JSON of ${d.title}`} onClick={() => copyJson(d.uid)}><Braces size={14} aria-hidden /></IconButton>
                {can("dashboards_edit") && <IconButton label={`Duplicate ${d.title}`} onClick={() => duplicate.mutate(d.uid)}><Copy size={14} aria-hidden /></IconButton>}
                {can("dashboards_delete") && <IconButton label={`Delete ${d.title}`} onClick={() => confirm(`Delete "${d.title}"? This can't be undone.`) && remove.mutate(d.uid)} className="hover:text-crit"><Trash2 size={14} aria-hidden /></IconButton>}
              </span>
            </div>
          </div>
        ))}
      </div>
      {importing && <ImportDialog onClose={() => setImporting(false)} onDone={(uid) => navigate({ to: "/dashboards/$id", params: { id: uid } } as any)} />}
    </div>
  );
}

function ImportDialog({ onClose, onDone }: { onClose: () => void; onDone: (uid: string) => void }) {
  const [text, setText] = useState("");
  const [problem, setProblem] = useState("");
  const [overwrite, setOverwrite] = useState(false);
  const run = useMutation({ mutationFn: (d: any) => post("/dashboards", { dashboard: d, overwrite }), onSuccess: (r) => onDone(r.uid), onError: (e: Error) => setProblem(e.message) });
  const go = () => {
    try {
      run.mutate(JSON.parse(text));
    } catch (e) {
      setProblem(`Not valid JSON: ${(e as Error).message}`);
    }
  };
  return (
    <Dialog title="Import dashboard JSON" onClose={onClose} width={860} footer={<Button variant="primary" onClick={go} disabled={!text.trim() || run.isPending}>Import</Button>}>
      <textarea autoFocus spellCheck={false} value={text} onChange={(e) => setText(e.target.value)} placeholder="Paste the dashboard JSON here" aria-label="Dashboard JSON" className={`${fieldClass} block w-full h-[380px] p-2.5 font-mono text-xs`} />
      <label className="flex items-center gap-2 mt-2 text-ink-2"><input type="checkbox" checked={overwrite} onChange={(e) => setOverwrite(e.target.checked)} />Replace the dashboard if this UID already exists</label>
      {problem && <p role="alert" className="text-crit mt-2">{problem}</p>}
    </Dialog>
  );
}

function VariableBar({ variables, values, onChange }: { variables: Variable[]; values: Record<string, string[]>; onChange: (name: string, v: string[]) => void }) {
  if (variables.length === 0) return null;
  return (
    <div className="flex flex-wrap items-center gap-2 mb-3">
      {variables.map((v) => <VariableSelect key={v.name} variable={v} value={values[v.name] ?? []} onChange={(x) => onChange(v.name, x)} />)}
    </div>
  );
}

function VariableSelect({ variable, value, onChange }: { variable: Variable; value: string[]; onChange: (v: string[]) => void }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const { data } = useQuery({ queryKey: ["field-values", variable.field], queryFn: () => get(`/fields/${variable.field}/values`), staleTime: 60_000 });
  useDismiss(ref, open, useCallback(() => setOpen(false), []));
  const toggle = (x: string) => {
    if (!variable.multi) {
      onChange(value.includes(x) ? [] : [x]);
      setOpen(false);
    } else onChange(value.includes(x) ? value.filter((y) => y !== x) : [...value, x]);
  };
  return (
    <div className="relative" ref={ref}>
      <button type="button" onClick={() => setOpen(!open)} aria-expanded={open} className="flex items-center h-8 rounded-md border border-line-2 bg-raised hover:bg-hover">
        <span className="pl-2.5 text-ink-3">{variable.label}</span>
        <span className="px-1.5 font-medium max-w-[220px] truncate">{value.length === 0 ? "All" : value.join(", ")}</span>
        <ChevronDown size={14} className="mr-2 text-ink-3" aria-hidden />
      </button>
      {open && (
        <div className={`absolute z-30 mt-1 min-w-[240px] max-h-[320px] overflow-y-auto p-1 ${popoverClass}`}>
          <button type="button" onClick={() => { onChange([]); setOpen(false); }} className="w-full text-left px-2 py-1.5 rounded-sm hover:bg-hover text-ink-2">All</button>
          {(data ?? []).map((o: any) => (
            <label key={o.value} className="flex items-center gap-2 px-2 py-1.5 rounded-sm hover:bg-hover cursor-pointer">
              <input type="checkbox" checked={value.includes(o.value)} onChange={() => toggle(o.value)} />
              <span className="truncate">{o.label}</span>
            </label>
          ))}
        </div>
      )}
    </div>
  );
}

/** Dashboard settings: the general fields, and the whole dashboard as JSON to copy, download or replace. */
function SettingsDialog({ board, onChange, onClose, canDelete, onDelete }: { board: Dashboard; onChange: (d: Dashboard) => void; onClose: () => void; canDelete: boolean; onDelete: () => void }) {
  const [tab, setTab] = useState("general");
  const [text, setText] = useState(() => JSON.stringify(board, null, 2));
  const [problem, setProblem] = useState("");
  const [copied, setCopied] = useState(false);
  const set = (patch: Partial<Dashboard>) => onChange({ ...board, ...patch });
  const setVar = (i: number, patch: Partial<Variable>) => set({ variables: board.variables.map((v, j) => (j === i ? { ...v, ...patch } : v)) });
  const switchTab = (t: string) => {
    // The JSON always shows the dashboard as it is now, including changes on the other tab.
    if (t === "json") {
      setText(JSON.stringify(board, null, 2));
      setProblem("");
      setCopied(false);
    }
    setTab(t);
  };
  const apply = () => {
    try {
      const parsed = JSON.parse(text);
      if (!parsed || typeof parsed !== "object" || !Array.isArray(parsed.panels)) throw new Error('"panels" must be a list');
      onChange({ ...parsed, uid: board.uid });
      onClose();
    } catch (e) {
      setProblem(`Not applied: ${(e as Error).message}`);
    }
  };
  const fileName = `${(board.title || "dashboard").replace(/[^\w.-]+/g, "-").toLowerCase()}.json`;
  return (
    <Dialog
      title="Dashboard settings"
      onClose={onClose}
      width={720}
      footer={
        tab === "json" ? (
          <>
            <span className="mr-auto text-xs text-ink-3">Apply replaces the dashboard being edited. Save it afterwards to keep it.</span>
            <Button onClick={async () => { await navigator.clipboard?.writeText(text); setCopied(true); }}><Copy size={14} />{copied ? "Copied" : "Copy"}</Button>
            <Button onClick={() => downloadBlob(fileName, text, "application/json")}><Download size={14} />Download</Button>
            <Button variant="primary" onClick={apply}>Apply</Button>
          </>
        ) : undefined
      }
    >
      <div className="-mx-4 -mt-4 mb-4">
        <Tabs tabs={[{ id: "general", label: "General" }, { id: "json", label: "JSON" }]} active={tab} onChange={switchTab} />
      </div>
      {tab === "json" ? (
        <>
          <p className="text-ink-2 mb-2">The whole dashboard as one JSON document. Copy it to move the dashboard to another system, or paste a dashboard here to replace this one.</p>
          <textarea spellCheck={false} value={text} onChange={(e) => setText(e.target.value)} aria-label="Dashboard JSON" className={`${fieldClass} block w-full h-[420px] p-2.5 font-mono text-xs`} />
          {problem && <p role="alert" className="text-crit mt-2">{problem}</p>}
        </>
      ) : (
      <div className="space-y-3">
        <Field label="Name"><input value={board.title} onChange={(e) => set({ title: e.target.value })} className={input} /></Field>
        <Field label="Description"><input value={board.description} onChange={(e) => set({ description: e.target.value })} className={input} /></Field>
        <Field label="Tags, comma separated"><input value={board.tags.join(", ")} onChange={(e) => set({ tags: e.target.value.split(",").map((t) => t.trim()).filter(Boolean) })} className={input} /></Field>
        <div className="grid grid-cols-3 gap-2">
          <Field label="Default from"><input value={board.time.from} onChange={(e) => set({ time: { ...board.time, from: e.target.value } })} className={input} /></Field>
          <Field label="Default to"><input value={board.time.to} onChange={(e) => set({ time: { ...board.time, to: e.target.value } })} className={input} /></Field>
          <Field label="Auto refresh"><input value={board.refresh} onChange={(e) => set({ refresh: e.target.value })} placeholder="30s, 1m, or off" className={input} /></Field>
        </div>
        <Field label="Link charts on hover">
          <Select className="w-full" value={board.sharedCrosshair === null ? "default" : String(board.sharedCrosshair)} onChange={(v) => set({ sharedCrosshair: v === "default" ? null : v === "true" })} options={[{ value: "default", label: "Follow user preference" }, { value: "true", label: "Always on for this dashboard" }, { value: "false", label: "Always off for this dashboard" }]} />
        </Field>
        <div>
          <div className="flex items-center justify-between mb-1">
            <span className="font-semibold">Filters shown on this dashboard</span>
            <Button size="sm" onClick={() => set({ variables: [...board.variables, { name: `var${board.variables.length + 1}`, label: "Trunk", type: "field", field: "trunk", multi: true }] })}><Plus size={14} />Add</Button>
          </div>
          {board.variables.length === 0 && <p className="text-ink-3 text-xs">No filters. Add one so everyone can narrow all panels at once, for example by capture ID.</p>}
          {board.variables.map((v, i) => (
            <div key={i} className="flex gap-2 mb-1.5 items-center">
              <input aria-label="Filter label" value={v.label} onChange={(e) => setVar(i, { label: e.target.value })} className={input} />
              <input aria-label="Variable name" value={v.name} onChange={(e) => setVar(i, { name: e.target.value.replace(/\W/g, "_") })} className={`${input} font-mono text-xs`} />
              <Select label="Field" value={v.field} onChange={(x) => setVar(i, { field: x })} options={VARIABLE_FIELDS} />
              <IconButton label="Remove filter" onClick={() => set({ variables: board.variables.filter((_, j) => j !== i) })} className="hover:text-crit"><Trash2 size={14} aria-hidden /></IconButton>
            </div>
          ))}
        </div>
        {canDelete && (
          <div className="pt-3 border-t border-line">
            <Button variant="danger" onClick={onDelete}><Trash2 size={14} />Delete this dashboard</Button>
          </div>
        )}
      </div>
      )}
    </Dialog>
  );
}

function VersionsDialog({ uid, tz, onClose, onRestored }: { uid: string; tz: string; onClose: () => void; onRestored: () => void }) {
  const { data } = useQuery({ queryKey: ["versions", uid], queryFn: () => get(`/dashboards/${uid}/versions`) });
  const restore = useMutation({ mutationFn: (v: number) => post(`/dashboards/${uid}/restore/${v}`), onSuccess: onRestored });
  return (
    <Dialog title="Version history" onClose={onClose} width={640}>
      <div className="-m-4 overflow-x-auto"><table className="data-table">
        <thead><tr><th className="num">Version</th><th>By</th><th>When</th><th>Note</th><th /></tr></thead>
        <tbody>
          {(data ?? []).map((v: any, i: number) => (
            <tr key={v.version}>
              <td className="num">{v.version}</td><td>{v.created_by}</td><td>{fmtDateTime(v.created_at, tz).slice(5, 16)}</td><td className="text-ink-2 whitespace-normal">{v.message}</td>
              <td className="num">{i > 0 && <Button size="sm" onClick={() => restore.mutate(v.version)}>Restore</Button>}</td>
            </tr>
          ))}
        </tbody>
      </table></div>
    </Dialog>
  );
}

function Grid({ board, editing, ctx, onLayout, onEdit, onDuplicate, onDelete }: { board: Dashboard; editing: boolean; ctx: PanelContext; onLayout: (l: readonly any[]) => void; onEdit: (id: number) => void; onDuplicate: (id: number) => void; onDelete: (id: number) => void }) {
  const { width, containerRef, mounted } = useContainerWidth();
  const layout = useMemo(() => board.panels.map((p) => ({ i: String(p.id), ...p.gridPos, minW: 2, minH: 2 })), [board.panels]);
  // On narrow screens the 24-column grid squeezes panels to a few pixels, so they are read top to bottom instead.
  // Small stat panels go two to a row. Editing keeps the grid.
  const stacked = mounted && width < 768 && !editing;
  const ordered = useMemo(() => [...board.panels].sort((a, b) => a.gridPos.y - b.gridPos.y || a.gridPos.x - b.gridPos.x), [board.panels]);
  return (
    <div ref={containerRef}>
      {stacked && (
        <div className="grid grid-cols-2 gap-2.5">
          {ordered.map((p) => (
            <div key={String(p.id)} className={p.gridPos.w <= 6 ? "" : "col-span-2"}>
              <PanelSized panel={p} ctx={ctx} />
            </div>
          ))}
        </div>
      )}
      {mounted && !stacked && (
        <GridLayout
          layout={layout}
          width={width}
          gridConfig={{ cols: GRID_COLS, rowHeight: ROW_HEIGHT, margin: [GRID_MARGIN, GRID_MARGIN], containerPadding: [0, 0] }}
          dragConfig={{ enabled: editing, handle: ".panel-drag", cancel: ".panel-action" }}
          resizeConfig={{ enabled: editing, handles: ["se"] }}
          onLayoutChange={(l) => editing && onLayout(l)}
        >
          {board.panels.map((p) => (
            <div key={String(p.id)}>
              <PanelSized panel={p} ctx={editing ? { ...ctx, editing: true, onEdit: () => onEdit(p.id), onDuplicate: () => onDuplicate(p.id), onDelete: () => onDelete(p.id) } : ctx} />
            </div>
          ))}
        </GridLayout>
      )}
    </div>
  );
}

function PanelSized({ panel, ctx }: { panel: PanelDef; ctx: PanelContext }) {
  return <PanelView panel={panel} ctx={ctx} height={panelHeightPx(panel.gridPos.h)} />;
}

function varParam(name: string) {
  return `var_${name}`;
}

export function DashboardView({ id }: { id: string }) {
  const client = useQueryClient();
  const navigate = useNavigate();
  const { can, crosshair, setRefreshMs, tz } = usePrefs();
  const { from, setRange } = useRange();
  const search = useSearch({ strict: false }) as Record<string, string | undefined>;
  const { data, error } = useQuery({ queryKey: ["dashboard", id], queryFn: () => get(`/dashboards/${id}`) });
  const [draft, setDraft] = useState<Dashboard | null>(null);
  const [panelEdit, setPanelEdit] = useState<number | null>(null);
  const [dialog, setDialog] = useState<"settings" | "versions" | null>(null);
  const [problem, setProblem] = useState("");
  const applied = useRef("");

  // Starting edit mode from a link, right after "New dashboard".
  useEffect(() => {
    if (search.edit && data && !draft) {
      setDraft(data.dashboard);
      navigate({ to: ".", search: (p: any) => ({ ...p, edit: undefined }), replace: true } as any);
    }
  }, [search.edit, data, draft, navigate]);

  // A dashboard's own time range and refresh apply once when it opens.
  useEffect(() => {
    if (!data || applied.current === id) return;
    applied.current = id;
    const d: Dashboard = data.dashboard;
    if (!search.from && d.time?.from && d.time.from !== from) setRange(d.time.from, d.time.to ?? "now");
    if (d.refresh) setRefreshMs(parseRefresh(d.refresh));
  }, [data, id, search.from, from, setRange, setRefreshMs]);

  const save = useMutation({
    mutationFn: () => put(`/dashboards/${id}`, { dashboard: draft, version: data.meta.version }),
    onSuccess: async () => {
      await client.invalidateQueries({ queryKey: ["dashboard", id] });
      await client.invalidateQueries({ queryKey: ["dashboards"] });
      setDraft(null);
      setProblem("");
    },
    onError: (e: Error) => setProblem(e.message),
  });
  const remove = useMutation({
    mutationFn: () => del(`/dashboards/${id}`),
    onSuccess: async () => {
      await client.invalidateQueries({ queryKey: ["dashboards"] });
      navigate({ to: "/dashboards" } as any);
    },
    onError: (e: Error) => setProblem(e.message),
  });

  const board: Dashboard | undefined = draft ?? data?.dashboard;
  const values = useMemo(() => {
    const out: Record<string, string[]> = {};
    for (const v of board?.variables ?? []) {
      const raw = search[varParam(v.name)];
      if (raw) out[v.name] = String(raw).split(",");
    }
    return out;
  }, [board, search]);

  if (error) return <ErrorNote error={error} />;
  if (!board) return <Loading />;

  const varFilter = board.variables.filter((v) => values[v.name]?.length).map((v) => `${v.field}:${values[v.name].join(",")}`).join(" ");
  const linked = board.sharedCrosshair ?? crosshair;
  const ctx: PanelContext = { dashboard: id, group: linked && !draft ? `dash-${id}` : undefined, varFilter: varFilter || undefined, vars: values };
  const setVar = (name: string, v: string[]) => navigate({ to: ".", search: (p: any) => ({ ...p, [varParam(name)]: v.length ? v.join(",") : undefined }) } as any);
  const update = (fn: (d: Dashboard) => Dashboard) => setDraft((d) => (d ? fn(d) : d));

  const toggleKiosk = () => {
    if (search.kiosk) document.exitFullscreen?.().catch(() => {});
    else document.documentElement.requestFullscreen?.().catch(() => {});
    navigate({ to: ".", search: (p: any) => ({ ...p, kiosk: p.kiosk ? undefined : "1" }) } as any);
  };

  const editingPanel = panelEdit !== null ? board.panels.find((p) => p.id === panelEdit) : undefined;

  return (
    <div>
      <PageHeader
        title={board.title}
        description={board.description}
        right={
          draft ? (
            <div className="flex flex-wrap gap-2">
              <Button onClick={() => { const p = newPanel(draft); update((d) => ({ ...d, panels: [...d.panels, p] })); setPanelEdit(p.id); }}><Plus size={14} />Add panel</Button>
              <Button onClick={() => setDialog("settings")}><Settings2 size={14} />Settings</Button>
              <Button onClick={() => { setDraft(null); setProblem(""); }}>Discard</Button>
              <Button variant="primary" onClick={() => save.mutate()} disabled={save.isPending}><Save size={14} />Save</Button>
            </div>
          ) : (
            <div className="flex flex-wrap gap-2">
              {can("dashboards_edit") && <Button onClick={() => setDialog("versions")}><History size={14} />History</Button>}
              <Button onClick={toggleKiosk}><Maximize2 size={14} />{search.kiosk ? "Exit full screen" : "Full screen"}</Button>
              {can("dashboards_edit") && <Button variant="primary" onClick={() => setDraft(data.dashboard)}><Pencil size={14} />Edit</Button>}
            </div>
          )
        }
      />
      {problem && <p className="text-crit mb-2">{problem}</p>}
      <VariableBar variables={board.variables} values={values} onChange={setVar} />
      {board.panels.length === 0 ? (
        <Card><Empty>{draft ? "Add a panel to get started." : "This dashboard has no panels."}</Empty></Card>
      ) : (
        <Grid
          board={board}
          editing={Boolean(draft)}
          ctx={ctx}
          onLayout={(l) => update((d) => ({ ...d, panels: d.panels.map((p) => { const g = l.find((x: any) => x.i === String(p.id)); return g ? { ...p, gridPos: { x: g.x, y: g.y, w: g.w, h: g.h } } : p; }) }))}
          onEdit={setPanelEdit}
          onDuplicate={(pid) => update((d) => { const src = d.panels.find((p) => p.id === pid)!; const bottom = d.panels.reduce((m, p) => Math.max(m, p.gridPos.y + p.gridPos.h), 0); return { ...d, panels: [...d.panels, { ...structuredClone(src), id: nextPanelId(d), title: `${src.title} copy`, gridPos: { ...src.gridPos, y: bottom } }] }; })}
          onDelete={(pid) => update((d) => ({ ...d, panels: d.panels.filter((p) => p.id !== pid) }))}
        />
      )}
      {!draft && <p className="text-ink-3 text-xs mt-3">Drag across a chart to zoom, double-click to reset, click a bar or point to open the matching calls. Dashed lines and hatched bars are still settling.</p>}

      {editingPanel && (
        <Suspense fallback={<Loading />}>
          <PanelEditor panel={editingPanel} ctx={ctx} onClose={() => setPanelEdit(null)} onApply={(p) => { update((d) => ({ ...d, panels: d.panels.map((x) => (x.id === p.id ? p : x)) })); setPanelEdit(null); }} />
        </Suspense>
      )}
      {dialog === "settings" && draft && <SettingsDialog board={draft} onChange={setDraft} onClose={() => setDialog(null)} canDelete={can("dashboards_delete")} onDelete={() => confirm(`Delete "${draft.title}"? This can't be undone.`) && remove.mutate()} />}
      {dialog === "versions" && <VersionsDialog uid={id} tz={tz} onClose={() => setDialog(null)} onRestored={() => { setDialog(null); client.invalidateQueries({ queryKey: ["dashboard", id] }); }} />}
    </div>
  );
}

export function HomePage() {
  const { can, settings } = usePrefs();
  if (!can("dashboards_view")) return <Navigate to={homePath(can, settings.sql_enabled)} replace />;
  return (
    <>
      <GettingStarted />
      <DashboardView key={settings.home_dashboard} id={settings.home_dashboard} />
    </>
  );
}

/** Shown on Home until the first HEP packet arrives. */
function GettingStarted() {
  const { data } = useQuery({ queryKey: ["sources"], queryFn: () => get("/sources"), refetchInterval: 15_000 });
  const [hidden, setHidden] = useState(() => {
    try {
      return localStorage.getItem("ss-onboarding-hidden") === "1";
    } catch {
      return false;
    }
  });
  const [copied, setCopied] = useState(false);
  if (!data || data.length > 0 || hidden) return null;
  const host = window.location.hostname;
  const conf = `[general]\nenabled=yes\ncapture_address=${host}:9060\ncapture_id=2001\nuuid_type=call-id`;
  const hide = () => {
    setHidden(true);
    try {
      localStorage.setItem("ss-onboarding-hidden", "1");
    } catch {
      // Hidden for this visit only.
    }
  };
  return (
    <Card
      className="mb-3"
      title="Connect your first SIP server"
      right={
        <IconButton label="Hide" onClick={hide} className="-mr-1.5">
          <X size={16} aria-hidden />
        </IconButton>
      }
    >
      <div className="p-3 max-w-[92ch]">
        <p className="text-ink-2">Calls, registrations and quality appear here as soon as a server sends HEP. Everything is stored on this server until you add a ClickHouse.</p>
        <ol className="mt-3 space-y-3 list-decimal pl-5 marker:font-semibold marker:text-ink-2">
          <li>
            <span className="font-semibold">Send HEP to port 9060.</span>{" "}
            <span className="text-ink-2">UDP or TCP, HEP version 2 or 3, to <span className="font-mono text-xs">{host}:9060</span>. Each sender appears under Sources by its address.</span>
          </li>
          <li>
            <span className="font-semibold">On Asterisk, put this in <span className="font-mono text-xs">hep.conf</span>,</span>{" "}
            <span className="text-ink-2">then load res_hep, res_hep_pjsip and res_hep_rtcp.</span>
            <div className="relative mt-1.5 max-w-[420px]">
              <pre className="rounded-md bg-sunken border border-line px-2.5 py-2 font-mono text-xs leading-5 overflow-x-auto">{conf}</pre>
              <Button size="sm" className="absolute top-1.5 right-1.5" onClick={() => navigator.clipboard?.writeText(conf).then(() => setCopied(true))}>
                {copied ? "Copied" : "Copy"}
              </Button>
            </div>
          </li>
          <li>
            <span className="font-semibold">Name your sources.</span>{" "}
            <span className="text-ink-2">OpenSIPS, Kamailio, FreeSWITCH and heplify work the same way. Give each sender an alias under Admin, Sources, then pick it from the source menu at the top.</span>
          </li>
        </ol>
      </div>
    </Card>
  );
}

const VIZ = [
  { value: "line", label: "Line" },
  { value: "area", label: "Area" },
  { value: "stacked", label: "Stacked bars" },
  { value: "bars", label: "Bars" },
];

export function ExplorePage() {
  const { data } = useQuery({ queryKey: ["catalogue"], queryFn: () => get("/metrics/catalogue") });
  const [metric, setMetric] = useState("attempts");
  const [dim, setDim] = useState("direction");
  const [viz, setViz] = useState("stacked");
  const [filter, setFilter] = useState("");
  const [applied, setApplied] = useState("");

  const metrics: MetricInfo[] = data?.metrics ?? [];
  const current = metrics.find((m) => m.id === metric);
  const panel = useMemo<PanelDef>(() => ({ id: 1, type: "timeseries", title: current?.name ?? "", gridPos: { x: 0, y: 0, w: 24, h: 18 }, query: { mode: "metric", metric, groupBy: dim || undefined, filter: applied || undefined }, options: { viz: viz as PanelDef["options"]["viz"] } }), [current, metric, dim, applied, viz]);
  if (!data) return <Loading />;

  return (
    <div>
      <PageHeader title="Explore" description="Pick a metric and a breakdown. No SQL needed." />
      <div className="flex flex-wrap items-center gap-2 mb-3">
        <Select label="Metric" value={metric} onChange={setMetric} options={metrics.map((m) => ({ value: m.id, label: m.name }))} />
        <Select label="Group by" value={dim} onChange={setDim} options={[{ value: "", label: "No breakdown" }, ...data.dimensions.map((d: any) => ({ value: d.id, label: d.name }))]} />
        <Select label="Visualization" value={viz} onChange={setViz} options={VIZ} />
        <form className="flex-1 min-w-[260px]" onSubmit={(e) => { e.preventDefault(); setApplied(filter); }}>
          <input value={filter} onChange={(e) => setFilter(e.target.value)} onBlur={() => setApplied(filter)} spellCheck={false} aria-label="Filter" placeholder="Filter, e.g. capture:201 direction:outbound" className={`${inputClass} font-mono text-xs`} />
        </form>
      </div>
      {current && (
        <p className="text-ink-2 mb-2">
          {current.description}. <span className="text-ink-3">Formula: </span>
          <code className="font-mono text-xs">{current.formula}</code>
        </p>
      )}
      <TimePanelForExplore key={`${metric}-${dim}`} panel={panel} />
    </div>
  );
}
