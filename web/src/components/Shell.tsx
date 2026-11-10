import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, Outlet, useNavigate, useRouterState, useSearch } from "@tanstack/react-router";
import { BarChart3, Check, ChevronDown, ChevronLeft, ChevronRight, Compass, Database, LayoutDashboard, LogOut, Moon, Radio, RefreshCw, Search, Server, Settings, Sun, Users } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { FormEvent } from "react";
import { get, post } from "../lib/api";
import { rangeParam, resolveMs, usePrefs, useRange, useSource } from "../lib/context";
import { fmtDateTime, tzOffsetMs } from "../lib/format";
import { Button, Select } from "./ui";

const ADMIN_SECTIONS: [string, string][] = [
  ["users_manage", "users"],
  ["roles_manage", "roles"],
  ["settings_manage", "settings"],
  ["storage_manage", "storage"],
  ["system_view", "sources"],
  ["audit_read", "audit"],
];

function useNav() {
  const { can, settings } = usePrefs();
  const adminSection = ADMIN_SECTIONS.find(([perm]) => can(perm))?.[1];
  const items = [
    { id: "home", group: "Monitor", to: "/", label: "Home", icon: LayoutDashboard, show: can("dashboards_view") },
    { id: "dashboards", group: "Monitor", to: "/dashboards", label: "Dashboards", icon: BarChart3, show: can("dashboards_view") },
    { id: "explore", group: "Monitor", to: "/explore", label: "Explore", icon: Compass, show: can("explore") },
    { id: "live", group: "Monitor", to: "/live", label: "Live", icon: Radio, show: can("live_view") },
    { id: "search", group: "Troubleshoot", to: "/search", label: "Search", icon: Search, show: can("search_all") },
    { id: "subscribers", group: "Troubleshoot", to: "/subscribers", label: "Subscribers", icon: Users, show: can("subscribers_view") },
    { id: "sql", group: "Troubleshoot", to: "/sql", label: "SQL workbench", icon: Database, show: can("sql_workbench") && settings.sql_enabled },
    { id: "admin", group: "System", to: `/admin/${adminSection ?? "users"}`, label: "Admin", icon: Settings, show: Boolean(adminSection) },
  ].filter((n) => n.show);
  return ["Monitor", "Troubleshoot", "System"].map((g) => ({ group: g, items: items.filter((n) => n.group === g) })).filter((g) => g.items.length > 0);
}

function initials(name: string) {
  const parts = name.trim().split(/[\s._-]+/).filter(Boolean);
  return ((parts[0]?.[0] ?? "?") + (parts[1]?.[0] ?? "")).toUpperCase();
}

/** Capture health at a glance, for people who can see the agents page. */
function CaptureCard() {
  const { data } = useQuery({ queryKey: ["admin", "sources"], queryFn: () => get("/admin/sources"), refetchInterval: 30_000 });
  if (!data) return null;
  const active = data.filter((a: any) => a.status !== "silent").length;
  return (
    <Link to="/admin/$section" params={{ section: "sources" } as any} className="block mx-3 mb-2 rounded-xl border border-line bg-sunken/60 p-3 hover:border-line-2">
      <div className="flex items-center gap-2">
        <span className={`w-2 h-2 rounded-full ${active > 0 ? "bg-good" : "bg-ink-3"}`} aria-hidden />
        <span className="text-[12.5px] font-medium">Capture</span>
        <span className="ml-auto text-[11.5px] text-ink-3 tnum">{active}/{data.length} sources</span>
      </div>
      <p className="text-[11.5px] text-ink-3 mt-1 leading-snug">
        {data.length === 0 ? "No HEP received yet. Point your SIP servers at port 9060." : active === data.length ? "All sources are sending." : `${data.length - active} source${data.length - active === 1 ? " is" : "s are"} silent.`}
      </p>
    </Link>
  );
}

export function Shell() {
  const { can, theme, toggleTheme, me, settings } = usePrefs();
  const client = useQueryClient();
  const nav = useNav();
  const path = useRouterState({ select: (s) => s.location.pathname });
  const search = useSearch({ strict: false }) as { from?: string; to?: string; src?: string; kiosk?: string };
  const storage = useQuery({ queryKey: ["active-storage"], queryFn: () => get("/storages/active"), staleTime: 30_000 });
  const logout = useMutation({ mutationFn: () => post("/auth/logout"), onSuccess: () => client.invalidateQueries() });
  const active = (to: string) => (to === "/" ? path === "/" : path.startsWith(to.split("/").slice(0, 2).join("/")));
  const noStorage = storage.data?.type === "none";

  if (search.kiosk) {
    return (
      <main className="min-h-full p-4">
        <Outlet />
      </main>
    );
  }

  return (
    <div className="flex h-full">
      <aside className="w-[232px] shrink-0 bg-surface border-r border-line flex flex-col">
        <div className="h-[60px] flex items-center gap-2.5 px-4 shrink-0">
          <img src="/logo.png" alt="" width={30} height={30} className="shrink-0" />
          <span className="font-semibold text-[15px] tracking-tight truncate">{settings.app_name}</span>
        </div>
        <nav className="flex-1 px-3 pb-3 overflow-y-auto">
          {nav.map((g) => (
            <div key={g.group} className="mt-3 first:mt-1">
              <div className="px-2.5 pb-1.5 text-[10.5px] font-medium uppercase tracking-[0.06em] text-ink-3">{g.group}</div>
              <div className="space-y-0.5">
                {g.items.map((n) => (
                  <Link
                    key={n.id}
                    to={n.to}
                    search={{ from: search.from, to: search.to, src: search.src } as any}
                    className={`flex items-center gap-2.5 px-2.5 h-9 rounded-lg ${active(n.to) ? "bg-sunken text-ink font-medium" : "text-ink-2 hover:bg-sunken hover:text-ink"}`}
                  >
                    <n.icon size={16} aria-hidden className={active(n.to) ? "text-ink" : "text-ink-3"} />
                    <span className="flex-1">{n.label}</span>
                  </Link>
                ))}
              </div>
            </div>
          ))}
        </nav>
        {can("system_view") && <CaptureCard />}
        <div className="px-3 py-3 border-t border-line">
          <div className="flex items-center gap-1">
            <Link to="/preferences" className="flex-1 min-w-0 flex items-center gap-2.5 px-1.5 py-1 rounded-lg hover:bg-sunken" title="Preferences">
              <span className="w-8 h-8 shrink-0 rounded-full bg-accent-wash text-accent text-[12px] font-semibold flex items-center justify-center" aria-hidden>{initials(me.display_name || me.username)}</span>
              <span className="min-w-0">
                <span className="block truncate font-medium text-[12.5px] leading-4">{me.display_name || me.username}</span>
                <span className="block truncate text-[11px] text-ink-3 leading-4">{me.role}</span>
              </span>
            </Link>
            <button type="button" onClick={toggleTheme} aria-label="Switch theme" title="Switch theme" className="p-1.5 rounded-lg hover:bg-sunken text-ink-2">
              {theme === "dark" ? <Sun size={15} /> : <Moon size={15} />}
            </button>
            <button type="button" onClick={() => logout.mutate()} aria-label="Sign out" title="Sign out" className="p-1.5 rounded-lg hover:bg-sunken text-ink-2"><LogOut size={15} /></button>
          </div>
        </div>
      </aside>
      <div className="flex-1 min-w-0 flex flex-col">
        <TopBar />
        {settings.banner_text && <div className="bg-warn-wash text-warn text-xs px-5 py-1.5 shrink-0">{settings.banner_text}</div>}
        {noStorage && <div className="bg-warn-wash text-warn text-xs px-5 py-1.5 shrink-0">No ClickHouse is selected, so captured traffic is not being stored. {can("storage_manage") ? <Link to="/admin/$section" params={{ section: "storage" } as any} className="underline">Select one on the Storage page</Link> : "Ask an administrator to select one."}.</div>}
        <main className="flex-1 min-h-0 overflow-auto p-5" id="main-scroll">
          <Outlet />
        </main>
      </div>
    </div>
  );
}

function TopBar() {
  const { can } = usePrefs();
  const navigate = useNavigate();
  const search = useSearch({ strict: false }) as { q?: string; from?: string; to?: string };
  const [q, setQ] = useState(search.q ?? "");
  const [focus, setFocus] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  const meta = useQuery({ queryKey: ["meta"], queryFn: () => get("/meta"), staleTime: 300_000 });
  const browse = can("search_all");

  useEffect(() => setQ(search.q ?? ""), [search.q]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      if (e.key === "/" && !["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName) && !target.isContentEditable) {
        e.preventDefault();
        input.current?.focus();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    input.current?.blur();
    if (!browse) {
      if (q.trim()) navigate({ to: "/subscribers/$number", params: { number: q.trim() } } as any);
      return;
    }
    navigate({ to: "/search", search: (p: any) => ({ from: p.from, to: p.to, q }) } as any);
  };

  const lastToken = q.split(/\s+/).pop() ?? "";
  const suggestions = focus && lastToken.length > 0 && /^[a-z]+$/i.test(lastToken)
    ? (meta.data?.search_fields ?? []).filter((f: any) => f.field.startsWith(lastToken.toLowerCase())).slice(0, 7)
    : [];

  return (
    <header className="h-[60px] shrink-0 bg-surface border-b border-line flex items-center gap-3 px-5">
      <form onSubmit={submit} className="relative flex-1 max-w-[720px]">
        <Search size={15} className="absolute left-3 top-1/2 -translate-y-1/2 text-ink-3" aria-hidden />
        <input
          ref={input}
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onFocus={() => setFocus(true)}
          onBlur={() => setTimeout(() => setFocus(false), 120)}
          spellCheck={false}
          aria-label="Universal search"
          placeholder={browse ? 'Paste a number, Call-ID or IP, or filter: capture:201 trunk:"Carrier-X" status:5xx' : "Enter the customer's number"}
          className="w-full h-9 rounded-lg bg-sunken border border-line pl-9 pr-9 font-mono text-[12.5px] placeholder:font-sans placeholder:text-ink-3 focus:bg-raised focus:border-accent focus:outline-none"
        />
        <kbd className="absolute right-2.5 top-1/2 -translate-y-1/2 text-[11px] text-ink-3 bg-raised border border-line-2 rounded-md px-1.5 leading-4">/</kbd>
        {suggestions.length > 0 && (
          <ul className="absolute z-30 top-full mt-1.5 left-0 w-[380px] bg-raised border border-line rounded-xl shadow-lg p-1">
            {suggestions.map((f: any) => (
              <li key={f.field}>
                <button
                  type="button"
                  className="w-full text-left px-2.5 py-1.5 rounded-lg hover:bg-sunken flex gap-3"
                  onMouseDown={(e) => {
                    e.preventDefault();
                    setQ(q.slice(0, q.length - lastToken.length) + f.field + ":");
                  }}
                >
                  <span className="font-mono text-xs w-[72px] shrink-0">{f.field}</span>
                  <span className="text-ink-3 text-xs truncate">{f.description}</span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </form>
      <div className="flex-1" />
      <SourcePicker />
      <TimePicker />
    </header>
  );
}

/** Which HEP sender to look at. Lists every source by alias, or by address when it has none. */
function SourcePicker() {
  const { can } = usePrefs();
  const { source, setSource } = useSource();
  const { data } = useQuery({ queryKey: ["sources"], queryFn: () => get("/sources"), refetchInterval: 30_000 });
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const click = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", click);
    return () => document.removeEventListener("mousedown", click);
  }, []);
  const list: any[] = data ?? [];
  const picked = list.find((x) => x.ip === source);
  const dot = (status: string) => <span className={`w-1.5 h-1.5 rounded-full shrink-0 ${status === "silent" ? "bg-ink-3" : "bg-good"}`} aria-hidden />;
  return (
    <div className="relative" ref={ref}>
      <Button onClick={() => setOpen((v) => !v)} aria-expanded={open} aria-haspopup="listbox" title="Show one HEP source">
        <Server size={14} className="text-ink-3" aria-hidden />
        <span className="max-w-[160px] truncate">{source ? (picked?.label ?? source) : "All sources"}</span>
        <ChevronDown size={13} className="text-ink-3" aria-hidden />
      </Button>
      {open && (
        <div className="absolute z-30 right-0 mt-1.5 w-[280px] bg-raised border border-line rounded-xl shadow-lg p-1" role="listbox">
          <button type="button" role="option" aria-selected={!source} onClick={() => { setSource(""); setOpen(false); }} className={`w-full flex items-center gap-2 px-2.5 py-1.5 rounded-lg text-left hover:bg-sunken ${!source ? "font-medium" : ""}`}>
            <span className="flex-1">All sources</span>
            {!source && <Check size={14} className="text-accent" aria-hidden />}
          </button>
          {list.length > 0 && <div className="my-1 border-t border-line" />}
          <div className="max-h-[300px] overflow-y-auto">
            {list.map((x) => (
              <button type="button" role="option" aria-selected={x.ip === source} key={x.ip} onClick={() => { setSource(x.ip); setOpen(false); }} className="w-full flex items-center gap-2 px-2.5 py-1.5 rounded-lg text-left hover:bg-sunken">
                {dot(x.status)}
                <span className="min-w-0 flex-1">
                  <span className={`block truncate ${x.ip === source ? "font-medium" : ""}`}>{x.label}</span>
                  {x.alias && <span className="block font-mono text-[11px] text-ink-3">{x.ip}</span>}
                </span>
                {x.ip === source && <Check size={14} className="text-accent shrink-0" aria-hidden />}
              </button>
            ))}
          </div>
          {list.length === 0 && <p className="px-2.5 py-2 text-xs text-ink-3">No HEP received yet. Sources appear here once your SIP servers send to port 9060.</p>}
          {can("system_view") && (
            <Link to="/admin/$section" params={{ section: "sources" } as any} onClick={() => setOpen(false)} className="block mt-1 border-t border-line px-2.5 pt-2 pb-1.5 text-xs text-accent hover:underline">
              Name your sources
            </Link>
          )}
        </div>
      )}
    </div>
  );
}

const QUICK = [
  { label: "Last 5 minutes", from: "now-5m" },
  { label: "Last 15 minutes", from: "now-15m" },
  { label: "Last 1 hour", from: "now-1h" },
  { label: "Last 6 hours", from: "now-6h" },
  { label: "Last 24 hours", from: "now-24h" },
  { label: "Last 7 days", from: "now-7d" },
];

function rangeLabel(from: string, to: string, tz: string): string {
  const q = QUICK.find((x) => x.from === from && to === "now");
  if (q) return q.label;
  const short = (v: string) => (/^now/.test(v) ? v : fmtDateTime(resolveMs(v, Date.now()), tz).slice(5, 16));
  return `${short(from)} → ${short(to)}`;
}

function toLocalInput(ms: number, tz: string) {
  return new Date(ms + tzOffsetMs(ms, tz)).toISOString().slice(0, 16);
}

function fromLocalInput(v: string, tz: string) {
  const guess = Date.parse(`${v}:00Z`);
  return guess - tzOffsetMs(guess, tz);
}

const REFRESH = [
  { value: "0", label: "Off" },
  { value: "5000", label: "5 s" },
  { value: "10000", label: "10 s" },
  { value: "30000", label: "30 s" },
  { value: "60000", label: "1 m" },
  { value: "300000", label: "5 m" },
];

export function TimePicker() {
  const { from, to, setRange } = useRange();
  const { tz, savePrefs, refreshMs, setRefreshMs, settings } = usePrefs();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const now = Date.now();
  const [a, setA] = useState("");
  const [b, setB] = useState("");
  const zones = [...new Set([tz, ...settings.time_zones.split(",").map((s) => s.trim()).filter(Boolean)])];

  useEffect(() => {
    const click = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", click);
    return () => document.removeEventListener("mousedown", click);
  }, []);

  useEffect(() => {
    if (open) {
      setA(toLocalInput(resolveMs(from, Date.now()), tz));
      setB(toLocalInput(resolveMs(to, Date.now()), tz));
    }
  }, [open, from, to, tz]);

  const step = (dir: number) => {
    const [f, t] = [resolveMs(from, now), resolveMs(to, now)];
    const span = t - f;
    setRange(rangeParam(f + dir * span), rangeParam(Math.min(t + dir * span, now)));
  };

  const dayRange = (daysAgo: number) => {
    const offset = tzOffsetMs(now, tz);
    const start = Math.floor((now + offset) / 864e5) * 864e5 - offset - daysAgo * 864e5;
    setRange(rangeParam(start), daysAgo === 0 ? "now" : rangeParam(start + 864e5));
    setOpen(false);
  };

  return (
    <div className="flex items-center gap-1.5" ref={ref}>
      <Button variant="ghost" className="!px-1" onClick={() => step(-1)} aria-label="Step back one range">
        <ChevronLeft size={15} />
      </Button>
      <div className="relative">
        <Button onClick={() => setOpen((v) => !v)} aria-expanded={open}>
          <span>{rangeLabel(from, to, tz)}</span>
          <span className="text-ink-3 font-normal">{tz}</span>
        </Button>
        {open && (
          <div className="absolute z-30 right-0 mt-1.5 w-[320px] bg-raised border border-line rounded-xl shadow-lg p-2.5">
            <div className="grid grid-cols-2 gap-px">
              {QUICK.map((q) => (
                <button
                  key={q.from}
                  type="button"
                  className={`text-left px-2 py-1.5 rounded-lg hover:bg-sunken ${q.from === from && to === "now" ? "font-semibold" : "text-ink-2"}`}
                  onClick={() => {
                    setRange(q.from, "now");
                    setOpen(false);
                  }}
                >
                  {q.label}
                </button>
              ))}
              <button type="button" className="text-left px-2 py-1.5 rounded-lg hover:bg-sunken text-ink-2" onClick={() => dayRange(0)}>Today</button>
              <button type="button" className="text-left px-2 py-1.5 rounded-lg hover:bg-sunken text-ink-2" onClick={() => dayRange(1)}>Yesterday</button>
            </div>
            <div className="border-t border-line mt-2 pt-2 space-y-1.5">
              <label className="flex items-center justify-between gap-2 text-xs text-ink-2">
                From
                <input type="datetime-local" value={a} onChange={(e) => setA(e.target.value)} className="h-7 rounded bg-sunken border border-line px-1 text-ink" />
              </label>
              <label className="flex items-center justify-between gap-2 text-xs text-ink-2">
                To
                <input type="datetime-local" value={b} onChange={(e) => setB(e.target.value)} className="h-7 rounded bg-sunken border border-line px-1 text-ink" />
              </label>
              <div className="flex items-center justify-between gap-2">
                <Select label="Time zone" value={tz} onChange={(v) => savePrefs({ tz: v })} options={zones.map((t) => ({ value: t, label: t }))} />
                <Button
                  variant="primary"
                  onClick={() => {
                    setRange(rangeParam(fromLocalInput(a, tz)), rangeParam(fromLocalInput(b, tz)));
                    setOpen(false);
                  }}
                >
                  Apply
                </Button>
              </div>
            </div>
          </div>
        )}
      </div>
      <Button variant="ghost" className="!px-1" onClick={() => step(1)} aria-label="Step forward one range" disabled={to === "now"}>
        <ChevronRight size={15} />
      </Button>
      <span className="flex items-center gap-1 text-ink-3">
        <RefreshCw size={13} aria-hidden />
        <Select label="Auto refresh" value={String(refreshMs || 0)} onChange={(v) => setRefreshMs(v === "0" ? false : Number(v))} options={REFRESH} />
      </span>
    </div>
  );
}
