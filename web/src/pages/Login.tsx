import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import type { FormEvent } from "react";
import { Button, fieldClass } from "../components/ui";
import { post } from "../lib/api";
import { applyTheme, resolveTheme } from "../lib/context";

const field = `${fieldClass} w-full h-9 px-2.5`;

function Frame({ settings, title, subtitle, children }: { settings: any; title: string; subtitle?: string; children: React.ReactNode }) {
  useEffect(() => {
    applyTheme(resolveTheme(settings.default_theme), settings.accent_color);
    document.title = settings.app_name;
  }, [settings]);
  return (
    <main className="min-h-full flex flex-col items-center justify-center px-4 py-10 bg-page">
      <div className="w-full max-w-[380px]">
        <div className="flex items-center justify-center gap-2.5 mb-6">
          <img src="/logo.png" alt="" width={32} height={32} />
          <span className="text-md font-semibold">{settings.app_name}</span>
        </div>
        <div className="bg-surface border border-line rounded-lg p-6">
          <h1 className="text-xl font-semibold">{title}</h1>
          {subtitle && <p className="text-ink-2 mt-1">{subtitle}</p>}
          <div className="mt-5">{children}</div>
        </div>
        {settings.login_message && <p className="text-ink-3 text-xs mt-4 text-center whitespace-pre-wrap">{settings.login_message}</p>}
      </div>
    </main>
  );
}

function Label({ text, children }: { text: string; children: React.ReactNode }) {
  return (
    <label className="block">
      <span className="block text-xs font-medium text-ink-2 mb-1">{text}</span>
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
    <Frame settings={settings} title="Sign in">
      <form onSubmit={submit} className="space-y-4">
        <Label text="Username">
          <input autoFocus autoComplete="username" value={username} onChange={(e) => setUsername(e.target.value)} className={field} />
        </Label>
        <Label text="Password">
          <input type="password" autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} className={field} />
        </Label>
        {login.isError && <p role="alert" className="text-crit">{(login.error as Error).message}</p>}
        <Button variant="primary" type="submit" disabled={login.isPending} className="w-full h-9">Sign in</Button>
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
    <Frame settings={settings} title="Set up the administrator" subtitle="Create the administrator account to finish setting up. Captured calls are stored on this server right away, and you can add a ClickHouse server later.">
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
        {problem && <p role="alert" className="text-crit">{problem}</p>}
        <Button variant="primary" type="submit" disabled={setup.isPending} className="w-full h-9">Create account and continue</Button>
      </form>
    </Frame>
  );
}
