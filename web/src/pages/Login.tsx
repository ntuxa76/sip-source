import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Activity, Search, Workflow } from "lucide-react";
import { useEffect, useState } from "react";
import type { FormEvent } from "react";
import { Button } from "../components/ui";
import { post } from "../lib/api";
import { applyTheme, resolveTheme } from "../lib/context";

const field = "w-full h-10 rounded-lg bg-raised border border-line-2 px-3 focus:border-accent focus:outline-none focus:ring-2 focus:ring-accent/15";

const POINTS = [
  { icon: Workflow, title: "Every call, every leg", text: "SIP ladders with both legs joined, findings and RTCP quality." },
  { icon: Activity, title: "Live health", text: "ASR, PDD, concurrency and failures per source and trunk." },
  { icon: Search, title: "Find anything", text: "Search by number, Call-ID or IP across all captured traffic." },
];

function Frame({ settings, title, subtitle, children }: { settings: any; title: string; subtitle?: string; children: React.ReactNode }) {
  useEffect(() => {
    applyTheme(resolveTheme(settings.default_theme), settings.accent_color);
    document.title = settings.app_name;
  }, [settings]);
  return (
    <div className="min-h-full grid lg:grid-cols-[minmax(0,1fr)_minmax(0,1.1fr)] bg-page">
      <aside className="hidden lg:flex flex-col justify-between p-12 text-white bg-[linear-gradient(150deg,#002171_0%,#0b4fb8_55%,#0178f5_100%)]">
        <div className="flex items-center gap-3">
          <span className="w-11 h-11 rounded-xl bg-white flex items-center justify-center shadow-sm">
            <img src="/logo.png" alt="" width={34} height={34} />
          </span>
          <span className="text-xl font-semibold tracking-tight">{settings.app_name}</span>
        </div>
        <div className="max-w-[440px]">
          <h2 className="text-[30px] leading-tight font-semibold tracking-tight">SIP capture and troubleshooting for your voice network.</h2>
          <ul className="mt-8 space-y-5">
            {POINTS.map((p) => (
              <li key={p.title} className="flex gap-3.5">
                <span className="w-9 h-9 rounded-lg bg-white/12 border border-white/15 flex items-center justify-center shrink-0"><p.icon size={17} aria-hidden /></span>
                <span>
                  <span className="block font-medium">{p.title}</span>
                  <span className="block text-white/75 text-[13px] mt-0.5">{p.text}</span>
                </span>
              </li>
            ))}
          </ul>
        </div>
        <p className="text-white/55 text-xs">HEP v2 and v3 · Asterisk, OpenSIPS, Kamailio, FreeSWITCH</p>
      </aside>
      <main className="flex items-center justify-center p-6">
        <div className="w-full max-w-[380px]">
          <div className="flex items-center gap-2.5 mb-8 lg:hidden">
            <img src="/logo.png" alt="" width={36} height={36} />
            <span className="text-lg font-semibold tracking-tight">{settings.app_name}</span>
          </div>
          <h1 className="text-[24px] font-semibold tracking-tight">{title}</h1>
          {subtitle && <p className="text-ink-2 mt-1.5">{subtitle}</p>}
          <div className="mt-6">{children}</div>
          {settings.login_message && <p className="text-ink-3 text-xs mt-6 whitespace-pre-wrap">{settings.login_message}</p>}
        </div>
      </main>
    </div>
  );
}

function Label({ text, children }: { text: string; children: React.ReactNode }) {
  return (
    <label className="block">
      <span className="block text-[12.5px] font-medium text-ink-2 mb-1.5">{text}</span>
      {children}
    </label>
  );
}

export function LoginPage({ settings }: { settings: any }) {
  const client = useQueryClient();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const login = useMutation({
    mutationFn: () => post("/auth/login", { username, password }),
    onSuccess: () => client.invalidateQueries(),
  });
  const submit = (e: FormEvent) => {
    e.preventDefault();
    login.mutate();
  };
  return (
    <Frame settings={settings} title="Welcome back" subtitle="Sign in to continue.">
      <form onSubmit={submit} className="space-y-4">
        <Label text="Username">
          <input autoFocus autoComplete="username" value={username} onChange={(e) => setUsername(e.target.value)} className={field} />
        </Label>
        <Label text="Password">
          <input type="password" autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} className={field} />
        </Label>
        {login.isError && <p className="text-crit text-[12.5px]">{(login.error as Error).message}</p>}
        <Button variant="primary" type="submit" disabled={login.isPending} className="w-full !h-10 justify-center text-[13px]">Sign in</Button>
      </form>
    </Frame>
  );
}

export function SetupPage({ settings, tokenRequired }: { settings: any; tokenRequired: boolean }) {
  const client = useQueryClient();
  const [username, setUsername] = useState("admin");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [token, setToken] = useState("");
  const [problem, setProblem] = useState("");
  const setup = useMutation({
    mutationFn: () => post("/auth/setup", { username, password, token: token || undefined }),
    onSuccess: () => client.invalidateQueries(),
    onError: (e: Error) => setProblem(e.message),
  });
  const min = settings.password_min_length;
  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (password.length < min) return setProblem(`Use at least ${min} characters.`);
    if (password !== confirm) return setProblem("The two passwords don't match.");
    setProblem("");
    setup.mutate();
  };
  return (
    <Frame settings={settings} title={`Welcome to ${settings.app_name}`} subtitle="Create the administrator account to finish setting up. Captured calls are stored on this server right away, and you can add a ClickHouse server later.">
      <form onSubmit={submit} className="space-y-4">
        <Label text="Administrator username">
          <input autoComplete="username" value={username} onChange={(e) => setUsername(e.target.value)} className={field} />
        </Label>
        <Label text={`Password, at least ${min} characters`}>
          <input autoFocus type="password" autoComplete="new-password" value={password} onChange={(e) => setPassword(e.target.value)} className={field} />
        </Label>
        <Label text="Repeat password">
          <input type="password" autoComplete="new-password" value={confirm} onChange={(e) => setConfirm(e.target.value)} className={field} />
        </Label>
        {tokenRequired && (
          <Label text="Setup token (VP_SETUP_TOKEN)">
            <input value={token} onChange={(e) => setToken(e.target.value)} className={field} />
          </Label>
        )}
        {problem && <p className="text-crit text-[12.5px]">{problem}</p>}
        <Button variant="primary" type="submit" disabled={setup.isPending} className="w-full !h-10 justify-center text-[13px]">Create account and continue</Button>
      </form>
    </Frame>
  );
}
