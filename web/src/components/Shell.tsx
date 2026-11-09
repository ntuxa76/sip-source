import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, Outlet, useNavigate, useRouterState, useSearch } from "@tanstack/react-router";
import { BarChart3, ChevronLeft, ChevronRight, CircleUserRound, Compass, Database, LayoutDashboard, LogOut, Moon, Radio, RefreshCw, Search, Settings, Sun, Users } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { FormEvent } from "react";
import { get, post } from "../lib/api";
import { rangeParam, resolveMs, usePrefs, useRange } from "../lib/context";
import { fmtDateTime, tzOffsetMs } from "../lib/format";
import { Button, Select } from "./ui";

const ADMIN_SECTIONS: [string, string][] = [
  ["users_manage", "users"],
  ["roles_manage", "roles"],
  ["settings_manage", "settings"],
  ["storage_manage", "storage"],
  ["system_view", "agents"],
  ["audit_read", "audit"],
];

function useNav() {
  const { can, settings } = usePrefs();
  const adminSection = ADMIN_SECTIONS.find(([perm]) => can(perm))?.[1];
  return [
    { id: "home", to: "/", label: "Home", icon: LayoutDashboard, show: can("dashboards_view") },
    { id: "search", to: "/search", label: "Search", icon: Search, show: can("search_all") },
    { id: "live", to: "/live", label: "Live", icon: Radio, show: can("live_view") },
    { id: "dashboards", to: "/dashboards", label: "Dashboards", icon: BarChart3, show: can("dashboards_view") },
    { id: "explore", to: "/explore", label: "Explore", icon: Compass, show: can("explore") },
    { id: "subscribers", to: "/subscribers", label: "Subscribers", icon: Users, show: can("subscribers_view") },
    { id: "sql", to: "/sql", label: "SQL workbench", icon: Database, show: can("sql_workbench") && settings.sql_enabled },
    { id: "admin", to: `/admin/${adminSection ?? "users"}`, label: "Admin", icon: Settings, show: Boolean(adminSection) },
  ].filter((n) => n.show);
}

export function Shell() {
  const { can, theme, toggleTheme, me, settings } = usePrefs();
  const client = useQueryClient();
  const nav = useNav();
  const path = useRouterState({ select: (s) => s.location.pathname });
  const search = useSearch({ strict: false }) as { from?: string; to?: string; kiosk?: string };
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
      <aside className="w-[196px] shrink-0 bg-surface border-r border-line flex flex-col">
        <div className="h-[50px] flex items-center gap-2 px-3.5 border-b border-line">
          <svg width="20" height="20" viewBox="0 0 32 32" aria-hidden>
            <rect width="32" height="32" rx="7" fill="var(--accent)" />
            <path d="M8 9v14M16 9v14M24 9v14M8 13h16M24 19H8" stroke="white" strokeWidth="2" strokeLinecap="round" />
          </svg>
          <span className="font-semibold tracking-tight truncate">{settings.app_name}</span>
        </div>
        <nav className="flex-1 p-2 space-y-px overflow-y-auto">
          {nav.map((n) => (
            <Link
              key={n.id}
              to={n.to}
              search={{ from: search.from, to: search.to } as any}
              className={`flex items-center gap-2.5 px-2.5 h-[30px] rounded-md ${active(n.to) ? "bg-accent-wash text-ink font-medium" : "text-ink-2 hover:bg-sunken hover:text-ink"}`}
            >
              <n.icon size={15} aria-hidden />
              <span className="flex-1">{n.label}</span>
            </Link>
          ))}
        </nav>
        <div className="p-2 border-t border-line">
          <div className="flex items-center gap-1">
            <Link to="/preferences" className="flex-1 min-w-0 flex items-center gap-2 px-1.5 py-1 rounded-md hover:bg-sunken" title="Preferences">
              <CircleUserRound size={18} className="text-ink-2 shrink-0" aria-hidden />
              <span className="min-w-0">
                <span className="block truncate font-medium text-[12.5px] leading-4">{me.display_name || me.username}</span>
                <span className="block truncate text-[11px] text-ink-3 leading-4">{me.role}</span>
              </span>
            </Link>
            <button type="button" onClick={toggleTheme} aria-label="Switch theme" title="Switch theme" className="p-1 rounded hover:bg-sunken text-ink-2">
              {theme === "dark" ? <Sun size={15} /> : <Moon size={15} />}
            </button>
            <button type="button" onClick={() => logout.mutate()} aria-label="Sign out" title="Sign out" className="p-1 rounded hover:bg-sunken text-ink-2"><LogOut size={15} /></button>
          </div>
        </div>
      </aside>
      <div className="flex-1 min-w-0 flex flex-col">
        <TopBar />
        {settings.banner_text && <div className="bg-warn-wash text-warn text-xs px-4 py-1 shrink-0">{settings.banner_text}</div>}
        {noStorage && <div className="bg-warn-wash text-warn text-xs px-4 py-1 shrink-0">No ClickHouse is selected, so captured traffic is not being stored. {can("storage_manage") ? <Link to="/admin/$section" params={{ section: "storage" } as any} className="underline">Select one on the Storage page</Link> : "Ask an administrator to select one."}.</div>}
        <main className="flex-1 min-h-0 overflow-auto p-4" id="main-scroll">
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
    <header className="h-[50px] shrink-0 bg-surface border-b border-line flex items-center gap-3 px-4">
      <form onSubmit={submit} className="relative flex-1 max-w-[720px]">
        <Search size={15} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-ink-3" aria-hidden />
        <input
          ref={input}
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onFocus={() => setFocus(true)}
          onBlur={() => setTimeout(() => setFocus(false), 120)}
          spellCheck={false}
          aria-label="Universal search"
          placeholder={browse ? 'Paste a number, Call-ID or IP, or filter: capture:201 trunk:"Carrier-X" status:5xx' : "Enter the customer's number"}
          className="w-full h-8 rounded-md bg-sunken border border-line pl-8 pr-8 font-mono text-[12.5px] placeholder:font-sans placeholder:text-ink-3 focus:bg-raised focus:border-accent focus:outline-none"
        />
        <kbd className="absolute right-2 top-1/2 -translate-y-1/2 text-[11px] text-ink-3 border border-line-2 rounded px-1">/</kbd>
        {suggestions.length > 0 && (
          <ul className="absolute z-30 top-full mt-1 left-0 w-[380px] bg-raised border border-line-2 rounded-md shadow-lg py-1">
            {suggestions.map((f: any) => (
              <li key={f.field}>
                <button
                  type="button"
                  className="w-full text-left px-2.5 py-1 hover:bg-sunken flex gap-3"
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
      <TimePicker />
    </header>
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
          <div className="absolute z-30 right-0 mt-1 w-[320px] bg-raised border border-line-2 rounded-lg shadow-lg p-2.5">
            <div className="grid grid-cols-2 gap-px">
              {QUICK.map((q) => (
                <button
                  key={q.from}
                  type="button"
                  className={`text-left px-2 py-1 rounded hover:bg-sunken ${q.from === from && to === "now" ? "font-semibold" : "text-ink-2"}`}
                  onClick={() => {
                    setRange(q.from, "now");
                    setOpen(false);
                  }}
                >
                  {q.label}
                </button>
              ))}
              <button type="button" className="text-left px-2 py-1 rounded hover:bg-sunken text-ink-2" onClick={() => dayRange(0)}>Today</button>
              <button type="button" className="text-left px-2 py-1 rounded hover:bg-sunken text-ink-2" onClick={() => dayRange(1)}>Yesterday</button>
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
