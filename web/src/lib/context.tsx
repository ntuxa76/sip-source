import { useNavigate, useRouterState, useSearch } from "@tanstack/react-router";
import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";
import { put } from "./api";

export interface Me {
  id: number;
  username: string;
  display_name: string;
  role: string;
  permissions: string[];
  prefs: UserPrefs;
  settings: AppSettings;
}

export interface UserPrefs {
  theme?: "light" | "dark" | "system";
  tz?: string;
  density?: "compact" | "comfortable";
  crosshair?: boolean;
  searchColumns?: string[];
  searchFacets?: string[];
  searchPageSize?: number;
}

export interface AppSettings {
  app_name: string;
  accent_color: string;
  default_theme: string;
  login_message: string;
  banner_text: string;
  default_tz: string;
  time_zones: string;
  home_dashboard: string;
  default_range: string;
  default_refresh_seconds: number;
  shared_crosshair: boolean;
  search_default_range: string;
  search_page_size: number;
  search_max_page_size: number;
  live_enabled: boolean;
  live_idle_minutes: number;
  sql_enabled: boolean;
  password_min_length: number;
}

interface Prefs {
  me: Me;
  settings: AppSettings;
  prefs: UserPrefs;
  savePrefs: (p: Partial<UserPrefs>) => void;
  theme: "light" | "dark";
  toggleTheme: () => void;
  tz: string;
  crosshair: boolean;
  density: "compact" | "comfortable";
  can: (permission: string) => boolean;
  refreshMs: number | false;
  setRefreshMs: (v: number | false) => void;
}

const Ctx = createContext<Prefs | null>(null);

function systemTheme(): "light" | "dark" {
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

export function resolveTheme(choice: string | undefined): "light" | "dark" {
  return choice === "light" || choice === "dark" ? choice : systemTheme();
}

/** Black or white, whichever reads better on `hex`. */
function readableOn(hex: string): string {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) return "#ffffff";
  const n = parseInt(m[1], 16);
  const lin = (v: number) => (v / 255 <= 0.03928 ? v / 255 / 12.92 : ((v / 255 + 0.055) / 1.055) ** 2.4);
  const l = 0.2126 * lin((n >> 16) & 255) + 0.7152 * lin((n >> 8) & 255) + 0.0722 * lin(n & 255);
  return l > 0.179 ? "#0e1013" : "#ffffff";
}

export function applyTheme(theme: "light" | "dark", accent?: string) {
  const root = document.documentElement;
  root.dataset.theme = theme;
  // An accent chosen by the administrator replaces the built-in one, with text on it kept readable.
  if (accent) {
    root.style.setProperty("--accent", accent);
    root.style.setProperty("--on-accent", readableOn(accent));
  } else {
    root.style.removeProperty("--accent");
    root.style.removeProperty("--on-accent");
  }
  try {
    localStorage.setItem("vp-theme", theme);
  } catch {
    // Only used to avoid a color flash on the next load.
  }
}

export function PrefsProvider({ me, children }: { me: Me; children: ReactNode }) {
  const [prefs, setPrefs] = useState<UserPrefs>(me.prefs ?? {});
  const settings = me.settings;
  const [refreshMs, setRefreshMs] = useState<number | false>(settings.default_refresh_seconds ? settings.default_refresh_seconds * 1000 : false);
  const theme = resolveTheme(prefs.theme ?? settings.default_theme);
  const perms = useMemo(() => new Set(me.permissions), [me.permissions]);

  useEffect(() => applyTheme(theme, settings.accent_color), [theme, settings.accent_color]);
  useEffect(() => {
    document.title = settings.app_name;
  }, [settings.app_name]);

  const value = useMemo<Prefs>(() => {
    const savePrefs = (p: Partial<UserPrefs>) => {
      setPrefs((old) => {
        const next = { ...old, ...p };
        void put("/auth/me/prefs", next);
        return next;
      });
    };
    return {
      me,
      settings,
      prefs,
      savePrefs,
      theme,
      toggleTheme: () => savePrefs({ theme: theme === "dark" ? "light" : "dark" }),
      tz: prefs.tz ?? settings.default_tz,
      crosshair: prefs.crosshair ?? settings.shared_crosshair,
      density: prefs.density ?? "comfortable",
      can: (permission: string) => perms.has(permission),
      refreshMs,
      setRefreshMs,
    };
  }, [me, settings, prefs, theme, perms, refreshMs]);

  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function usePrefs(): Prefs {
  const v = useContext(Ctx);
  if (!v) throw new Error("usePrefs outside provider");
  return v;
}

export interface RangeSearch {
  from?: string;
  to?: string;
}

/** The global time range. It lives in the URL, so every view is a deep link (FR-CH-08). */
export function useRange() {
  const search = useSearch({ strict: false }) as RangeSearch;
  const { settings } = usePrefs();
  const navigate = useNavigate();
  const path = useRouterState({ select: (s) => s.location.pathname });
  const fallback = path === "/search" ? settings.search_default_range : settings.default_range;
  const from = search.from ?? fallback;
  const to = search.to ?? "now";
  const setRange = useCallback(
    (f: string, t: string) => {
      navigate({ to: ".", search: (prev: any) => ({ ...prev, from: f, to: t }) } as any);
    },
    [navigate],
  );
  return { from, to, setRange, defaultFrom: fallback };
}

/** The HEP source picked in the top bar, by address. Empty means every source. */
export function useSource() {
  const search = useSearch({ strict: false }) as { src?: string };
  const navigate = useNavigate();
  const setSource = useCallback((src: string) => {
    navigate({ to: ".", search: (prev: any) => ({ ...prev, src: src || undefined }) } as any);
  }, [navigate]);
  return { source: search.src ?? "", setSource };
}

/** Absolute range boundary for the URL. ISO keeps links readable and unambiguous. */
export function rangeParam(ms: number): string {
  return new Date(ms).toISOString();
}

export function resolveMs(expr: string, now: number): number {
  if (expr === "now") return now;
  const m = /^now-(\d+)([smhd])$/.exec(expr);
  if (m) return now - Number(m[1]) * { s: 1e3, m: 6e4, h: 36e5, d: 864e5 }[m[2] as "s" | "m" | "h" | "d"];
  const n = Number(expr);
  return Number.isFinite(n) ? n : Date.parse(expr);
}
