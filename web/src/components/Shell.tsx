import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, Navigate, Outlet, useNavigate, useRouterState, useSearch } from "@tanstack/react-router";
import { BarChart3, Check, ChevronDown, ChevronLeft, ChevronRight, Clock, Compass, Database, LayoutDashboard, LogOut, Menu, Moon, PanelLeftClose, PanelLeftOpen, Radio, RefreshCw, Search, Server, Settings, Sun, UserRound, Users, X } from "lucide-react";
import { useCallback, useEffect, useId, useRef, useState } from "react";
import type { FormEvent } from "react";
import { get, post } from "../lib/api";
import { rangeParam, resolveMs, usePrefs, useRange, useSource } from "../lib/context";
import { fmtDateTime, tzOffsetMs } from "../lib/format";
import { adminHome, homePath, pathAllowed, seesTraffic } from "../lib/access";
import { Button, IconButton, Select, fieldClass, popoverClass, useDismiss } from "./ui";

function useNav() {
  const { can, settings } = usePrefs();
  const admin = adminHome(can);
  const items = [
    { id: "home", group: "Monitor", to: "/", label: "Home", icon: LayoutDashboard, show: can("dashboards_view") },
    { id: "dashboards", group: "Monitor", to: "/dashboards", label: "Dashboards", icon: BarChart3, show: can("dashboards_view") },
    { id: "explore", group: "Monitor", to: "/explore", label: "Explore", icon: Compass, show: can("explore") },
    { id: "live", group: "Monitor", to: "/live", label: "Live", icon: Radio, show: can("live_view") },
    { id: "search", group: "Troubleshoot", to: "/search", label: "Search", icon: Search, show: can("search_all") },
    { id: "subscribers", group: "Troubleshoot", to: "/subscribers", label: "Subscribers", icon: Users, show: can("subscribers_view") },
    { id: "sql", group: "Troubleshoot", to: "/sql", label: "SQL workbench", icon: Database, show: can("sql_workbench") && settings.sql_enabled },
    { id: "admin", group: "System", to: admin ?? "/admin/users", label: "Admin", icon: Settings, show: Boolean(admin) },
  ].filter((n) => n.show);
  return ["Monitor", "Troubleshoot", "System"].map((g) => ({ group: g, items: items.filter((n) => n.group === g) })).filter((g) => g.items.length > 0);
}

/** One row in a menu or picker. */
const menuItem = "w-full flex items-center gap-2 px-2 py-1.5 rounded-sm text-left hover:bg-hover focus-visible:-outline-offset-2";

/** Capture health at a glance, for people who can see the sources page. */
function CaptureStatus({ compact }: { compact: boolean }) {
  const { data } = useQuery({ queryKey: ["admin", "sources"], queryFn: () => get("/admin/sources"), refetchInterval: 30_000 });
  if (!data) return null;
  const active = data.filter((a: any) => a.status !== "silent").length;
  const silent = data.length - active;
  const summary = data.length === 0 ? "No HEP received yet. Point your SIP servers at port 9060." : silent === 0 ? "All sources are sending." : `${silent} source${silent === 1 ? " is" : "s are"} silent.`;
  if (compact) {
    return (
      <Link to="/admin/$section" params={{ section: "sources" } as any} title={`Capture: ${active}/${data.length} sources. ${summary}`} aria-label={`Capture: ${summary}`} className="grid place-items-center h-10 shrink-0 border-t border-line hover:bg-hover focus-visible:-outline-offset-2">
        <span className={`w-2 h-2 rounded-full ${active > 0 ? "bg-good" : "bg-ink-3"}`} aria-hidden />
      </Link>
    );
  }
  return (
    <Link to="/admin/$section" params={{ section: "sources" } as any} className="block shrink-0 px-4 py-2.5 border-t border-line hover:bg-hover focus-visible:-outline-offset-2">
      <div className="flex items-center gap-2">
        <span className={`w-1.5 h-1.5 shrink-0 rounded-full ${active > 0 ? "bg-good" : "bg-ink-3"}`} aria-hidden />
        <span className="font-medium">Capture</span>
        <span className="ml-auto text-xs text-ink-3 tnum">
          {active}/{data.length} sources
        </span>
      </div>
      <p className="mt-0.5 text-xs text-ink-3">{summary}</p>
    </Link>
  );
}

export function Shell() {
  const { can, theme, toggleTheme, me, settings, prefs, savePrefs } = usePrefs();
  const client = useQueryClient();
  const nav = useNav();
  const path = useRouterState({ select: (s) => s.location.pathname });
  const search = useSearch({ strict: false }) as { from?: string; to?: string; src?: string; kiosk?: string };
  const storage = useQuery({ queryKey: ["active-storage"], queryFn: () => get("/storages/active"), staleTime: 30_000 });
  const logout = useMutation({ mutationFn: () => post("/auth/logout"), onSuccess: () => client.invalidateQueries() });
  const active = (to: string) => (to === "/" ? path === "/" : path.startsWith(to.split("/").slice(0, 2).join("/")));
  const noStorage = storage.data?.type === "none";
  // Below 1024 px the sidebar is a drawer opened from the top bar.
  const [menuOpen, setMenuOpen] = useState(false);
  const navRef = useRef<HTMLElement>(null);
  // On wide screens the sidebar can shrink to icons. The choice is kept per user; the phone
  // drawer always opens in full.
  const collapsed = Boolean(prefs.sidebarCollapsed) && !menuOpen;

  useEffect(() => setMenuOpen(false), [path]);

  useEffect(() => {
    if (!menuOpen) return;
    navRef.current?.querySelector("a")?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      setMenuOpen(false);
      document.getElementById("menu-button")?.focus();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [menuOpen]);

  const skipToContent = (e: React.MouseEvent) => {
    e.preventDefault();
    document.getElementById("main-scroll")?.focus();
  };

  // A page the account can't open, such as one left in the address bar by the previous user,
  // sends it to its own start page instead of showing an error.
  const page = pathAllowed(path, can, settings.sql_enabled) ? <Outlet /> : <Navigate to={homePath(can, settings.sql_enabled)} replace />;

  if (search.kiosk) {
    return <main className="min-h-full p-4">{page}</main>;
  }

  const banner = "shrink-0 px-3 sm:px-4 py-1.5 bg-warn-wash border-b border-line text-warn text-xs";

  return (
    <div className="flex h-full">
      <a href="#main-scroll" onClick={skipToContent} className={`sr-only focus:not-sr-only focus:fixed focus:top-2 focus:left-2 focus:z-50 focus:px-3 focus:py-2 ${popoverClass}`}>
        Skip to content
      </a>
      {menuOpen && <div className="lg:hidden fixed inset-0 z-30 bg-scrim" onClick={() => setMenuOpen(false)} aria-hidden />}
      <aside
        id="sidebar"
        className={`${menuOpen ? "max-lg:fixed max-lg:inset-y-0 max-lg:left-0 max-lg:z-40 max-lg:shadow-pop" : "max-lg:hidden"} ${collapsed ? "w-14" : "w-56"} shrink-0 flex flex-col bg-sidebar border-r border-line transition-[width] duration-150`}
      >
        <div className={`h-12 shrink-0 flex items-center border-b border-line ${collapsed ? "justify-center" : "gap-2.5 px-4"}`}>
          <Link to={homePath(can, settings.sql_enabled)} title="Home" aria-label={collapsed ? `${settings.app_name} home` : undefined} className="flex items-center gap-2.5 min-w-0 -mx-1 px-1 py-1 rounded-md hover:bg-hover">
            <img src="/logo.png" alt="" width={24} height={24} className="shrink-0" />
            {!collapsed && <span className="text-sm font-semibold truncate">{settings.app_name}</span>}
          </Link>
          <IconButton label="Close menu" onClick={() => setMenuOpen(false)} className="lg:hidden ml-auto -mr-2">
            <X size={16} aria-hidden />
          </IconButton>
        </div>
        <nav ref={navRef} aria-label="Main" className="flex-1 min-h-0 py-1 overflow-y-auto overflow-x-hidden">
          {nav.map((g, i) => (
            <div key={g.group}>
              {collapsed ? (
                i > 0 && <div className="mx-3 my-2 border-t border-line" aria-hidden />
              ) : (
                <div className="px-4 pt-3 pb-1 text-2xs font-semibold uppercase tracking-[0.06em] text-ink-3">{g.group}</div>
              )}
              {g.items.map((n) => {
                const on = active(n.to);
                return (
                  <Link
                    key={n.id}
                    to={n.to}
                    search={{ from: search.from, to: search.to, src: search.src } as any}
                    aria-current={on ? "page" : undefined}
                    title={collapsed ? n.label : undefined}
                    className={`flex items-center h-8 my-px rounded-md focus-visible:-outline-offset-2 ${collapsed ? "justify-center w-10 mx-auto" : "gap-2.5 mx-2 px-2.5"} ${
                      on ? "bg-sidebar-active text-ink font-medium shadow-[inset_0_0_0_1px_var(--line)]" : "text-ink-2 hover:bg-hover hover:text-ink"
                    }`}
                  >
                    <n.icon size={16} aria-hidden className={`shrink-0 ${on ? "text-accent" : "text-ink-3"}`} />
                    {collapsed ? <span className="sr-only">{n.label}</span> : <span className="truncate">{n.label}</span>}
                  </Link>
                );
              })}
            </div>
          ))}
        </nav>
        {can("system_view") && <CaptureStatus compact={collapsed} />}
        {collapsed ? (
          <div className="shrink-0 flex flex-col items-center gap-0.5 py-2 border-t border-line">
            <Link to="/preferences" title={`${me.display_name || me.username}: preferences`} aria-label="Preferences" className="grid place-items-center w-7 h-7 rounded-md text-ink-2 hover:bg-hover hover:text-ink">
              <UserRound size={15} aria-hidden />
            </Link>
            <IconButton label="Switch theme" onClick={toggleTheme}>
              {theme === "dark" ? <Sun size={15} aria-hidden /> : <Moon size={15} aria-hidden />}
            </IconButton>
            <IconButton label="Sign out" onClick={() => logout.mutate()}>
              <LogOut size={15} aria-hidden />
            </IconButton>
            <IconButton label="Expand sidebar" onClick={() => savePrefs({ sidebarCollapsed: false })} className="max-lg:hidden">
              <PanelLeftOpen size={15} aria-hidden />
            </IconButton>
          </div>
        ) : (
          <div className="shrink-0 flex items-center gap-0.5 p-2 border-t border-line">
            <Link to="/preferences" title="Preferences" className="flex-1 min-w-0 px-2 py-1 rounded-md hover:bg-hover">
              <span className="block truncate font-medium leading-[18px]">{me.display_name || me.username}</span>
              <span className="block truncate text-xs text-ink-3">{me.role}</span>
            </Link>
            <IconButton label="Switch theme" onClick={toggleTheme}>
              {theme === "dark" ? <Sun size={15} aria-hidden /> : <Moon size={15} aria-hidden />}
            </IconButton>
            <IconButton label="Sign out" onClick={() => logout.mutate()}>
              <LogOut size={15} aria-hidden />
            </IconButton>
            <IconButton label="Collapse sidebar" onClick={() => savePrefs({ sidebarCollapsed: true })} className="max-lg:hidden">
              <PanelLeftClose size={15} aria-hidden />
            </IconButton>
          </div>
        )}
      </aside>
      <div className="flex-1 min-w-0 flex flex-col">
        <TopBar menuOpen={menuOpen} onMenu={() => setMenuOpen(true)} />
        {settings.banner_text && <div className={banner}>{settings.banner_text}</div>}
        {noStorage && (
          <div className={banner}>
            No ClickHouse is selected, so captured traffic is not being stored.{" "}
            {can("storage_manage") ? (
              <Link to="/admin/$section" params={{ section: "storage" } as any} className="underline">
                Select one on the Storage page
              </Link>
            ) : (
              "Ask an administrator to select one"
            )}
            .
          </div>
        )}
        <main className="flex-1 min-h-0 overflow-auto p-3 sm:p-4 focus:outline-none" id="main-scroll" tabIndex={-1}>
          {page}
        </main>
      </div>
    </div>
  );
}

function TopBar({ menuOpen, onMenu }: { menuOpen: boolean; onMenu: () => void }) {
  const { can } = usePrefs();
  const path = useRouterState({ select: (s) => s.location.pathname });
  // The time range and source only apply to pages that show captured traffic.
  const traffic = seesTraffic(can) && !path.startsWith("/admin") && path !== "/preferences";
  const lookup = can("search_all") || can("subscribers_view");
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
    <header className="shrink-0 flex flex-wrap lg:flex-nowrap items-center gap-x-3 gap-y-2 px-3 sm:px-4 py-2 lg:py-0 lg:h-12 bg-surface border-b border-line">
      <IconButton id="menu-button" label="Open menu" onClick={onMenu} aria-expanded={menuOpen} aria-controls="sidebar" className="lg:hidden -ml-1">
        <Menu size={18} aria-hidden />
      </IconButton>
      {lookup ? <form onSubmit={submit} role="search" className="relative flex-1 min-w-[200px] max-w-[560px]">
        <Search size={14} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-ink-3 pointer-events-none" aria-hidden />
        <input
          ref={input}
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onFocus={() => setFocus(true)}
          onBlur={() => setTimeout(() => setFocus(false), 120)}
          spellCheck={false}
          aria-label="Universal search"
          placeholder={browse ? "Search number, Call-ID or IP" : "Customer's number"}
          className={`${fieldClass} w-full h-8 pl-8 pr-8 font-mono text-xs placeholder:font-sans placeholder:text-[13px]`}
        />
        <kbd className="max-sm:hidden absolute right-2 top-1/2 -translate-y-1/2 px-1 rounded-xs border border-line-2 font-mono text-2xs text-ink-3 pointer-events-none">/</kbd>
        {suggestions.length > 0 && (
          <ul className={`absolute z-30 top-full mt-1 left-0 w-[380px] max-w-[calc(100vw-24px)] p-1 ${popoverClass}`}>
            {suggestions.map((f: any) => (
              <li key={f.field}>
                <button
                  type="button"
                  className={`${menuItem} gap-3`}
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
      </form> : <div className="flex-1" />}
      <div className="hidden lg:block flex-1" />
      {traffic && <SourcePicker />}
      {traffic && <TimePicker />}
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
  const alignLeft = useOpensLeft(ref, open, 280);
  useDismiss(ref, open, useCallback(() => setOpen(false), []));
  const list: any[] = data ?? [];
  const picked = list.find((x) => x.ip === source);
  const pick = (ip: string) => {
    setSource(ip);
    setOpen(false);
  };
  return (
    <div className="relative" ref={ref}>
      <Button onClick={() => setOpen((v) => !v)} aria-expanded={open} aria-haspopup="listbox" title="Show one HEP source">
        <Server size={14} className="text-ink-3" aria-hidden />
        <span className="max-w-[160px] truncate">{source ? (picked?.label ?? source) : "All sources"}</span>
        <ChevronDown size={14} className="text-ink-3 -mr-1" aria-hidden />
      </Button>
      {open && (
        <div className={`absolute z-30 ${alignLeft ? "left-0" : "right-0"} mt-1 w-[280px] max-w-[calc(100vw-24px)] p-1 ${popoverClass}`} role="listbox" aria-label="HEP source">
          <button type="button" role="option" aria-selected={!source} onClick={() => pick("")} className={`${menuItem} ${!source ? "font-medium" : ""}`}>
            <span className="flex-1">All sources</span>
            {!source && <Check size={14} className="text-accent" aria-hidden />}
          </button>
          {list.length > 0 && <div className="my-1 border-t border-line" />}
          <div className="max-h-[300px] overflow-y-auto">
            {list.map((x) => (
              <button type="button" role="option" aria-selected={x.ip === source} key={x.ip} onClick={() => pick(x.ip)} className={menuItem}>
                <span className={`w-1.5 h-1.5 shrink-0 rounded-full ${x.status === "silent" ? "bg-ink-3" : "bg-good"}`} aria-hidden />
                <span className="min-w-0 flex-1">
                  <span className={`block truncate ${x.ip === source ? "font-medium" : ""}`}>{x.label}</span>
                  {x.alias && <span className="block font-mono text-2xs text-ink-3">{x.ip}</span>}
                </span>
                {x.ip === source && <Check size={14} className="text-accent shrink-0" aria-hidden />}
              </button>
            ))}
          </div>
          {list.length === 0 && <p className="px-2 py-2 text-xs text-ink-3">No HEP received yet. Sources appear here once your SIP servers send to port 9060.</p>}
          {can("system_view") && (
            <Link to="/admin/$section" params={{ section: "sources" } as any} onClick={() => setOpen(false)} className="block mt-1 px-2 pt-2 pb-1.5 border-t border-line text-xs text-accent hover:underline">
              Name your sources
            </Link>
          )}
        </div>
      )}
    </div>
  );
}

/** A right-aligned dropdown runs off the left edge when its button sits near the left, as on phones. */
function useOpensLeft(ref: React.RefObject<HTMLElement | null>, open: boolean, width: number) {
  const [left, setLeft] = useState(false);
  useEffect(() => {
    if (open && ref.current) setLeft(ref.current.getBoundingClientRect().right < width + 12);
  }, [open, ref, width]);
  return left;
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
  return `${short(from)} – ${short(to)}`;
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
  const rangeRef = useRef<HTMLDivElement>(null);
  const alignLeft = useOpensLeft(rangeRef, open, 320);
  const fromId = useId();
  const toId = useId();
  const now = Date.now();
  const [a, setA] = useState("");
  const [b, setB] = useState("");
  const zones = [...new Set([tz, ...settings.time_zones.split(",").map((s) => s.trim()).filter(Boolean)])];

  useDismiss(ref, open, useCallback(() => setOpen(false), []));

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

  const quick = "h-7 px-2 rounded-sm text-left hover:bg-hover focus-visible:-outline-offset-2";

  return (
    <div className="flex items-center gap-1" ref={ref}>
      <Button variant="ghost" size="icon" onClick={() => step(-1)} aria-label="Step back one range" title="Step back one range">
        <ChevronLeft size={16} aria-hidden />
      </Button>
      <div className="relative" ref={rangeRef}>
        <Button onClick={() => setOpen((v) => !v)} aria-expanded={open} aria-haspopup="dialog">
          <Clock size={14} className="text-ink-3" aria-hidden />
          <span className="tnum">{rangeLabel(from, to, tz)}</span>
          <span className="text-ink-3 font-normal max-sm:hidden">{tz}</span>
        </Button>
        {open && (
          <div role="dialog" aria-label="Time range" className={`absolute z-30 ${alignLeft ? "left-0" : "right-0"} mt-1 w-[320px] max-w-[calc(100vw-24px)] p-2 ${popoverClass}`}>
            <div className="grid grid-cols-2 gap-x-1">
              {QUICK.map((q) => (
                <button
                  key={q.from}
                  type="button"
                  className={`${quick} ${q.from === from && to === "now" ? "bg-accent-wash text-ink font-medium" : "text-ink-2"}`}
                  onClick={() => {
                    setRange(q.from, "now");
                    setOpen(false);
                  }}
                >
                  {q.label}
                </button>
              ))}
              <button type="button" className={`${quick} text-ink-2`} onClick={() => dayRange(0)}>
                Today
              </button>
              <button type="button" className={`${quick} text-ink-2`} onClick={() => dayRange(1)}>
                Yesterday
              </button>
            </div>
            <div className="grid grid-cols-[auto_1fr] items-center gap-x-3 gap-y-1.5 mt-2 pt-2 px-1 border-t border-line">
              <label htmlFor={fromId} className="text-xs font-medium text-ink-2">
                From
              </label>
              <input id={fromId} type="datetime-local" value={a} onChange={(e) => setA(e.target.value)} className={`${fieldClass} w-full h-7 px-1.5 tnum`} />
              <label htmlFor={toId} className="text-xs font-medium text-ink-2">
                To
              </label>
              <input id={toId} type="datetime-local" value={b} onChange={(e) => setB(e.target.value)} className={`${fieldClass} w-full h-7 px-1.5 tnum`} />
            </div>
            <div className="flex items-center justify-between gap-2 mt-2 px-1 pb-1">
              <Select label="Time zone" value={tz} onChange={(v) => savePrefs({ tz: v })} options={zones.map((t) => ({ value: t, label: t }))} className="min-w-0" />
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
        )}
      </div>
      <Button variant="ghost" size="icon" onClick={() => step(1)} aria-label="Step forward one range" title="Step forward one range" disabled={to === "now"}>
        <ChevronRight size={16} aria-hidden />
      </Button>
      <span className="flex items-center gap-1.5 ml-1 text-ink-3">
        <RefreshCw size={14} aria-hidden />
        <Select label="Auto refresh" value={String(refreshMs || 0)} onChange={(v) => setRefreshMs(v === "0" ? false : Number(v))} options={REFRESH} />
      </span>
    </div>
  );
}
