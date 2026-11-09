import { sql, StandardSQL } from "@codemirror/lang-sql";
import { useMutation, useQuery } from "@tanstack/react-query";
import { basicSetup, EditorView } from "codemirror";
import { ChevronRight, Play } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Button, Card, Empty, ErrorNote, Loading, PageHeader } from "../components/ui";
import { get, post } from "../lib/api";
import { usePrefs, useRange } from "../lib/context";
import { fmtDateTime } from "../lib/format";

export function WorkbenchPage() {
  const { from, to } = useRange();
  const { tz } = usePrefs();
  const schema = useQuery({ queryKey: ["schema"], queryFn: () => get("/queries/schema"), staleTime: Infinity });
  const library = useQuery({ queryKey: ["library"], queryFn: () => get("/queries/library"), staleTime: Infinity });
  const storage = useQuery({ queryKey: ["active-storage"], queryFn: () => get("/storages/active") });
  const editorEl = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const [openView, setOpenView] = useState<string | null>("v_calls");
  const [side, setSide] = useState<"library" | "schema">("library");
  const run = useMutation({ mutationFn: (text: string) => post("/queries/run", { sql: text, from, to, tz }) });
  const ready = Boolean(schema.data && library.data);

  // The editor's element only exists once the page has loaded past its spinner.
  useEffect(() => {
    if (!ready || !editorEl.current) return;
    const created = new EditorView({
      doc: library.data[0]?.sql ?? "",
      extensions: [basicSetup, sql({ dialect: StandardSQL })],
      parent: editorEl.current,
    });
    view.current = created;
    return () => created.destroy();
  }, [ready, library.data]);

  const load = (text: string) => {
    const v = view.current;
    if (v) v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: text } });
  };
  const execute = () => view.current && run.mutate(view.current.state.doc.toString());

  if (schema.error) return <ErrorNote error={schema.error} />;
  if (!schema.data || !library.data) return <Loading />;
  const categories: string[] = [...new Set<string>(library.data.map((l: any) => l.category))];
  const result = run.data;
  const cell = (v: unknown, type: string) => {
    if (v === null || v === undefined) return "NULL";
    if (type.startsWith("DateTime") && typeof v === "number") return fmtDateTime(v, tz);
    return Array.isArray(v) ? `[${v.join(", ")}]` : String(v);
  };

  return (
    <div>
      <PageHeader title="SQL workbench" description="Read-only SQL. Macros {{from}}, {{to}} and {{tz}} follow the time picker." />
      <div className="flex gap-3 items-start">
        <aside className="w-[260px] shrink-0 bg-surface border border-line rounded-lg">
          <div className="flex border-b border-line">
            {(["library", "schema"] as const).map((s) => (
              <button type="button" key={s} onClick={() => setSide(s)} className={`flex-1 py-2 capitalize text-[12.5px] ${side === s ? "font-medium border-b-2 border-accent -mb-px" : "text-ink-2"}`}>{s === "library" ? "Query library" : "Views"}</button>
            ))}
          </div>
          <div className="p-2 max-h-[calc(100vh-180px)] overflow-y-auto">
            {side === "library" && categories.map((c) => (
              <div key={c} className="mb-2">
                <h3 className="text-ink-3 text-[11.5px] font-medium px-1.5 mb-0.5">{c}</h3>
                {library.data.filter((l: any) => l.category === c).map((l: any) => (
                  <button type="button" key={l.id} onClick={() => { load(l.sql); run.reset(); }} className="block w-full text-left px-1.5 py-1 rounded hover:bg-sunken text-[12.5px]">{l.title}</button>
                ))}
              </div>
            ))}
            {side === "schema" && schema.data.map((v: any) => (
              <div key={v.name}>
                <button type="button" onClick={() => setOpenView(openView === v.name ? null : v.name)} aria-expanded={openView === v.name} className="flex items-center gap-1 w-full text-left px-1 py-1 rounded hover:bg-sunken">
                  <ChevronRight size={13} className={openView === v.name ? "rotate-90" : ""} aria-hidden />
                  <span className="font-mono text-xs">{v.name}</span>
                </button>
                {openView === v.name && (
                  <div className="ml-5 mb-1.5">
                    <p className="text-ink-3 text-xs mb-1">{v.description}</p>
                    {v.columns.map((c: any) => (
                      <div key={c.name} title={c.description} className="flex justify-between gap-2 text-[11.5px] py-px">
                        <span className="font-mono truncate">{c.name}</span>
                        <span className="text-ink-3 font-mono truncate text-right">{c.type}</span>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            ))}
          </div>
        </aside>
        <div className="flex-1 min-w-0 space-y-3">
          <Card>
            <div ref={editorEl} className="h-[220px] overflow-hidden rounded-t-lg" />
            <div className="flex items-center justify-between gap-3 px-3 py-2 border-t border-line">
              <span className="text-ink-3 text-xs">{`Running on ${storage.data?.name || "ClickHouse"} as read-only.`}</span>
              <Button variant="primary" onClick={execute} disabled={run.isPending}><Play size={13} />Run</Button>
            </div>
          </Card>
          <Card title="Results" right={result && <span className="tnum">{result.rows.length} rows{result.stats.truncated ? " (cut off)" : ""} · {result.stats.rows_read.toLocaleString()} rows read · {result.stats.duration_ms} ms</span>}>
            {run.isError ? <ErrorNote error={run.error} /> : !result ? <Empty>Pick a query or write your own, then run it.</Empty> : (
              <div className="overflow-auto max-h-[420px]">
                <table className="data-table">
                  <thead><tr>{result.columns.map((c: any) => <th key={c.name} title={c.type}>{c.name}</th>)}</tr></thead>
                  <tbody>{result.rows.map((r: unknown[], i: number) => <tr key={i}>{r.map((v, j) => <td key={j} className="font-mono text-xs">{cell(v, result.columns[j].type)}</td>)}</tr>)}</tbody>
                </table>
              </div>
            )}
          </Card>
        </div>
      </div>
    </div>
  );
}
