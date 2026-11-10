import { sql, StandardSQL } from "@codemirror/lang-sql";
import { syntaxHighlighting } from "@codemirror/language";
import { classHighlighter } from "@lezer/highlight";
import { useMutation, useQuery } from "@tanstack/react-query";
import { basicSetup, EditorView } from "codemirror";
import { ArrowLeft, Play } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { PanelView, buildQuery } from "../components/Panel";
import type { PanelContext } from "../components/Panel";
import { Button, Select, Tabs, Toggle, fieldClass, inputClass } from "../components/ui";
import { get, post } from "../lib/api";
import { usePrefs, useRange } from "../lib/context";
import { PANEL_TYPES, supportsSql, usesMetric } from "../lib/dashboard";
import type { PanelDef } from "../lib/dashboard";

const field = inputClass;

function Label({ text, children }: { text: string; children: React.ReactNode }) {
  return (
    <label className="block">
      <span className="block mb-1 text-xs font-medium text-ink-2">{text}</span>
      {children}
    </label>
  );
}

function SqlEditor({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const el = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const callback = useRef(onChange);
  callback.current = onChange;

  useEffect(() => {
    if (!el.current) return;
    const created = new EditorView({
      doc: value,
      extensions: [basicSetup, sql({ dialect: StandardSQL }), syntaxHighlighting(classHighlighter), EditorView.updateListener.of((u) => u.docChanged && callback.current(u.state.doc.toString()))],
      parent: el.current,
    });
    view.current = created;
    return () => created.destroy();
    // The editor owns its text after mounting. External changes are handled below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const v = view.current;
    if (v && v.state.doc.toString() !== value) v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: value } });
  }, [value]);

  return <div ref={el} className="h-[210px] overflow-hidden border border-line rounded-md" />;
}

export default function PanelEditor({ panel, ctx, onClose, onApply }: { panel: PanelDef; ctx: PanelContext; onClose: () => void; onApply: (p: PanelDef) => void }) {
  const { can, tz } = usePrefs();
  const { from, to } = useRange();
  const [p, setP] = useState<PanelDef>(() => structuredClone(panel));
  const [preview, setPreview] = useState<PanelDef>(() => structuredClone(panel));
  const catalogue = useQuery({ queryKey: ["catalogue"], queryFn: () => get("/metrics/catalogue") });
  const metrics: any[] = catalogue.data?.metrics ?? [];
  const dims: any[] = catalogue.data?.dimensions ?? [];
  const mode = p.query.mode ?? "metric";
  const sqlAllowed = supportsSql(p.type) && can("dashboards_sql") && p.query.metrics === undefined;

  // Builder changes show straight away. SQL waits for Run, since half-typed SQL can't execute.
  const update = (next: PanelDef) => {
    setP(next);
    if ((next.query.mode ?? "metric") === "metric") setPreview(next);
  };
  const setQ = (patch: Partial<PanelDef["query"]>) => update({ ...p, query: { ...p.query, ...patch } });
  const setO = (patch: Partial<PanelDef["options"]>) => update({ ...p, options: { ...p.options, ...patch } });
  const setPreviewOptions = (patch: Partial<PanelDef>) => {
    const next = { ...p, ...patch };
    setP(next);
    setPreview((old) => ({ ...old, ...patch }));
  };

  const generated = useMutation({
    mutationFn: () => post("/metrics/query", buildQuery({ ...p, query: { ...p.query, mode: "metric" } }, from, to, tz, { ...ctx, editing: true })),
    onSuccess: (r) => {
      const text = r.sql || "";
      const next: PanelDef = { ...p, query: { ...p.query, mode: "sql", sql: text } };
      setP(next);
      setPreview(next);
    },
  });

  const toggleMode = (m: string) => {
    if (m === "sql" && !p.query.sql) return generated.mutate();
    update({ ...p, query: { ...p.query, mode: m as "metric" | "sql" } });
    if (m === "sql") setPreview({ ...p, query: { ...p.query, mode: "sql" } });
  };

  const typeChanged = (type: string) => {
    const keepsSql = supportsSql(type);
    update({ ...p, type, query: keepsSql ? p.query : { ...p.query, mode: "metric" } });
    setPreview({ ...p, type, query: keepsSql ? p.query : { ...p.query, mode: "metric" } });
  };

  const previewCtx: PanelContext = { ...ctx, editing: true, group: undefined, onEdit: undefined, onDelete: undefined, onDuplicate: undefined };
  const tabs = [{ id: "metric", label: "Builder" }, ...(sqlAllowed ? [{ id: "sql", label: "SQL" }] : [])];

  return (
    <div className="fixed inset-0 z-30 bg-page flex flex-col">
      <header className="h-12 shrink-0 bg-surface border-b border-line flex items-center gap-2 px-3 sm:px-4">
        <Button variant="ghost" onClick={onClose}><ArrowLeft size={14} />Back</Button>
        <h1 className="text-sm font-semibold">Edit panel</h1>
        <div className="flex-1" />
        <Button onClick={onClose}>Discard</Button>
        <Button variant="primary" onClick={() => onApply(p)}>Apply</Button>
      </header>
      <div className="flex-1 min-h-0 flex">
        <div className="flex-1 min-w-0 overflow-y-auto p-4 space-y-3">
          <PanelView key={`${preview.type}-${preview.query.mode}`} panel={preview} ctx={previewCtx} height={330} />
          {usesMetric(p.type) && (
            <div className="bg-surface border border-line rounded-lg">
              <div><Tabs tabs={tabs} active={mode} onChange={toggleMode} /></div>
              <div className="p-3">
                {mode === "metric" ? (
                  <div className="grid md:grid-cols-2 gap-3">
                    {p.type === "table" ? (
                      <Label text="Columns (hold Ctrl to pick several)">
                        <select multiple value={p.query.metrics ?? []} onChange={(e) => setQ({ metrics: Array.from(e.target.selectedOptions).map((o) => o.value) })} className={`${fieldClass} w-full h-32 p-1`}>
                          {metrics.map((m) => <option key={m.id} value={m.id}>{m.name}</option>)}
                        </select>
                      </Label>
                    ) : (
                      <Label text="Metric">
                        <Select className="w-full" value={p.query.metric ?? ""} onChange={(v) => setQ({ metric: v })} options={metrics.map((m) => ({ value: m.id, label: m.name }))} />
                      </Label>
                    )}
                    {p.type !== "stat" && p.type !== "histogram" && (
                      <Label text="Group by">
                        <Select className="w-full" value={p.query.groupBy ?? ""} onChange={(v) => setQ({ groupBy: v || undefined })} options={[{ value: "", label: "No breakdown" }, ...dims.map((d) => ({ value: d.id, label: d.name }))]} />
                      </Label>
                    )}
                    <div className="md:col-span-2">
                      <Label text="Filter, in search syntax">
                        <input value={p.query.filter ?? ""} onChange={(e) => setQ({ filter: e.target.value || undefined })} spellCheck={false} placeholder="capture:201 direction:outbound status:5xx" className={`${field} font-mono text-xs`} />
                      </Label>
                    </div>
                    {p.type === "timeseries" && (
                      <>
                        <Label text="Max series">
                          <input type="number" min={1} max={8} value={p.query.top ?? ""} onChange={(e) => setQ({ top: e.target.value ? Number(e.target.value) : undefined })} placeholder="8" className={field} />
                        </Label>
                        <Label text="Minimum bucket (seconds)">
                          <input type="number" min={0} step={60} value={p.query.minInterval ?? ""} onChange={(e) => setQ({ minInterval: e.target.value ? Number(e.target.value) : undefined })} placeholder="automatic" className={field} />
                        </Label>
                      </>
                    )}
                    {supportsSql(p.type) && !sqlAllowed && p.type !== "table" && <p className="text-ink-3 text-xs md:col-span-2">Your account can't write SQL in panels.</p>}
                  </div>
                ) : (
                  <div>
                    <SqlEditor value={p.query.sql ?? ""} onChange={(v) => setP((old) => ({ ...old, query: { ...old.query, sql: v } }))} />
                    <div className="flex items-start justify-between gap-3 mt-2">
                      <p className="text-ink-3 text-xs">
                        Macros: <code className="font-mono">{"{{from}} {{to}} {{interval}} {{tz}} {{filters}}"}</code>, plus any dashboard filter by name.{" "}
                        Return a time column named <code className="font-mono">bucket</code>, an optional text column for series names, and numeric values.
                      </p>
                      <div className="flex gap-2 shrink-0">
                        <Button onClick={() => generated.mutate()} disabled={generated.isPending}>Reset from builder</Button>
                        <Button variant="primary" onClick={() => setPreview(p)}><Play size={14} />Run</Button>
                      </div>
                    </div>
                  </div>
                )}
              </div>
            </div>
          )}
          {p.type === "calls" && (
            <div className="bg-surface border border-line rounded-lg p-3">
              <Label text="Calls to list, in search syntax">
                <input value={p.query.q ?? ""} onChange={(e) => setQ({ q: e.target.value })} spellCheck={false} placeholder="mos<3 capture:201" className={`${field} font-mono text-xs`} />
              </Label>
            </div>
          )}
          {p.type === "text" && (
            <div className="bg-surface border border-line rounded-lg p-3">
              <Label text="Text">
                <textarea value={p.options.markdown ?? ""} onChange={(e) => setO({ markdown: e.target.value })} className={`${fieldClass} block w-full h-40 p-2.5`} />
              </Label>
            </div>
          )}
        </div>
        <aside className="w-[300px] shrink-0 bg-surface border-l border-line overflow-y-auto p-3 space-y-3">
          <h2 className="font-semibold">Panel options</h2>
          <Label text="Title"><input value={p.title} onChange={(e) => setPreviewOptions({ title: e.target.value })} className={field} /></Label>
          <Label text="Description (shown on hover)"><input value={p.options.description ?? ""} onChange={(e) => setO({ description: e.target.value || undefined })} className={field} /></Label>
          <Label text="Visualization">
            <Select className="w-full" value={p.type} onChange={typeChanged} options={PANEL_TYPES} />
          </Label>
          {p.type === "timeseries" && (
            <>
              <Label text="Style">
                <Select className="w-full" value={p.options.viz ?? "line"} onChange={(v) => setO({ viz: v as PanelDef["options"]["viz"] })} options={[{ value: "line", label: "Line" }, { value: "area", label: "Area" }, { value: "stacked", label: "Stacked bars" }, { value: "bars", label: "Bars" }]} />
              </Label>
              <div className="grid grid-cols-2 gap-2">
                <Label text="Y axis min"><input type="number" value={p.options.yMin ?? ""} onChange={(e) => setO({ yMin: e.target.value === "" ? undefined : Number(e.target.value) })} placeholder="auto" className={field} /></Label>
                <Label text="Y axis max"><input type="number" value={p.options.yMax ?? ""} onChange={(e) => setO({ yMax: e.target.value === "" ? undefined : Number(e.target.value) })} placeholder="auto" className={field} /></Label>
              </div>
              <Toggle checked={p.options.legend !== false} onChange={(v) => setO({ legend: v })}>Show legend</Toggle>
            </>
          )}
          {p.type === "top" && (
            <div className="grid grid-cols-2 gap-2">
              <Label text="Rows"><input type="number" min={1} max={100} value={p.options.limit ?? 8} onChange={(e) => setO({ limit: Number(e.target.value) })} className={field} /></Label>
              <Label text="Order">
                <Select className="w-full" value={p.options.order ?? "desc"} onChange={(v) => setO({ order: v })} options={[{ value: "desc", label: "Highest first" }, { value: "asc", label: "Lowest first" }]} />
              </Label>
            </div>
          )}
          {mode === "sql" && (
            <Label text="Unit of the result">
              <Select className="w-full" value={p.options.unit ?? ""} onChange={(v) => setO({ unit: v })} options={[{ value: "", label: "None" }, { value: "%", label: "Percent" }, { value: "ms", label: "Milliseconds" }, { value: "s", label: "Seconds" }, { value: "/s", label: "Per second" }, { value: "mos", label: "MOS" }]} />
            </Label>
          )}
        </aside>
      </div>
    </div>
  );
}
