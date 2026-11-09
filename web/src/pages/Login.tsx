import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import type { FormEvent } from "react";
import { Button } from "../components/ui";
import { post } from "../lib/api";
import { applyTheme, resolveTheme } from "../lib/context";

const field = "w-full h-9 rounded-md bg-sunken border border-line px-2.5 focus:border-accent focus:outline-none";

function Frame({ settings, title, subtitle, children }: { settings: any; title: string; subtitle?: string; children: React.ReactNode }) {
  useEffect(() => {
    applyTheme(resolveTheme(settings.default_theme), settings.accent_color);
    document.title = settings.app_name;
  }, [settings]);
  return (
    <div className="min-h-full flex items-center justify-center p-4">
      <div className="w-full max-w-[380px]">
        <div className="flex items-center gap-2 mb-5 justify-center">
          <svg width="26" height="26" viewBox="0 0 32 32" aria-hidden>
            <rect width="32" height="32" rx="7" fill="var(--accent)" />
            <path d="M8 9v14M16 9v14M24 9v14M8 13h16M24 19H8" stroke="white" strokeWidth="2" strokeLinecap="round" />
          </svg>
          <span className="text-lg font-semibold tracking-tight">{settings.app_name}</span>
        </div>
        <div className="bg-surface border border-line rounded-xl p-5">
          <h1 className="text-base font-semibold">{title}</h1>
          {subtitle && <p className="text-ink-2 mt-1 mb-1">{subtitle}</p>}
          {children}
        </div>
        {settings.login_message && <p className="text-ink-3 text-xs text-center mt-3 whitespace-pre-wrap">{settings.login_message}</p>}
      </div>
    </div>
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
    <Frame settings={settings} title="Sign in">
      <form onSubmit={submit} className="space-y-3 mt-3">
        <label className="block">
          <span className="text-xs text-ink-2">Username</span>
          <input autoFocus autoComplete="username" value={username} onChange={(e) => setUsername(e.target.value)} className={field} />
        </label>
        <label className="block">
          <span className="text-xs text-ink-2">Password</span>
          <input type="password" autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} className={field} />
        </label>
        {login.isError && <p className="text-crit text-xs">{(login.error as Error).message}</p>}
        <Button variant="primary" type="submit" disabled={login.isPending} className="w-full !h-9 justify-center">Sign in</Button>
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
    <Frame settings={settings} title="Create the administrator account" subtitle="This is the first start. Set the admin password to open the app.">
      <form onSubmit={submit} className="space-y-3 mt-3">
        <label className="block">
          <span className="text-xs text-ink-2">Username</span>
          <input autoComplete="username" value={username} onChange={(e) => setUsername(e.target.value)} className={field} />
        </label>
        <label className="block">
          <span className="text-xs text-ink-2">Password, at least {min} characters</span>
          <input autoFocus type="password" autoComplete="new-password" value={password} onChange={(e) => setPassword(e.target.value)} className={field} />
        </label>
        <label className="block">
          <span className="text-xs text-ink-2">Repeat password</span>
          <input type="password" autoComplete="new-password" value={confirm} onChange={(e) => setConfirm(e.target.value)} className={field} />
        </label>
        {tokenRequired && (
          <label className="block">
            <span className="text-xs text-ink-2">Setup token (VP_SETUP_TOKEN)</span>
            <input value={token} onChange={(e) => setToken(e.target.value)} className={field} />
          </label>
        )}
        {problem && <p className="text-crit text-xs">{problem}</p>}
        <Button variant="primary" type="submit" disabled={setup.isPending} className="w-full !h-9 justify-center">Create account and continue</Button>
      </form>
    </Frame>
  );
}
