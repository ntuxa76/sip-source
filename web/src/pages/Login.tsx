import { useMutation, useQueryClient } from "@tanstack/react-query";
import { LogIn } from "lucide-react";
import { useEffect, useState } from "react";
import type { FormEvent } from "react";
import { Button, fieldClass } from "../components/ui";
import { post } from "../lib/api";
import { applyTheme, resolveTheme } from "../lib/context";

const field = `${fieldClass} w-full h-9 px-2.5`;

function Frame({ settings, title, subtitle, children }: { settings: any; title: string; subtitle?: string; children: React.ReactNode }) {
  useEffect(() => {
    applyTheme(resolveTheme(undefined), settings.accent_color);
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

/** A failed single sign-on comes back as ?sso_error=..., shown once and then removed from the address. */
function useSsoError() {
  const [message] = useState(() => new URLSearchParams(window.location.search).get("sso_error") ?? "");
  useEffect(() => {
    if (message) window.history.replaceState(null, "", window.location.pathname);
  }, [message]);
  return message;
}

export function LoginPage({ settings, sso }: { settings: any; sso?: { enabled: boolean; label: string; password: boolean } }) {
  const client = useQueryClient();
  const ssoError = useSsoError();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  // With password sign-in off, administrators can still open the form as a way in.
  const [showPassword, setShowPassword] = useState(!sso?.enabled || sso.password);
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
      {ssoError && <p role="alert" className="mb-4 px-3 py-2 rounded-md bg-crit-wash text-crit">{ssoError}</p>}
      {sso?.enabled && (
        <>
          <a href="/api/v1/auth/sso/login" className="flex items-center justify-center gap-2 w-full h-9 rounded-md border border-accent bg-accent text-on-accent font-medium hover:bg-accent-strong">
            <LogIn size={16} aria-hidden />
            Sign in with {sso.label}
          </a>
          {showPassword ? (
            <div className="flex items-center gap-3 my-4 text-xs text-ink-3">
              <span className="flex-1 border-t border-line" />
              or with a password
              <span className="flex-1 border-t border-line" />
            </div>
          ) : (
            <button type="button" onClick={() => setShowPassword(true)} className="block mx-auto mt-4 text-xs text-ink-3 hover:text-ink hover:underline">
              Administrator sign-in with a password
            </button>
          )}
        </>
      )}
      {showPassword && <form onSubmit={submit} className="space-y-4">
        <Label text="Username">
          <input autoFocus={!sso?.enabled} autoComplete="username" value={username} onChange={(e) => setUsername(e.target.value)} className={field} />
        </Label>
        <Label text="Password">
          <input type="password" autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} className={field} />
        </Label>
        {login.isError && <p role="alert" className="text-crit">{(login.error as Error).message}</p>}
        <Button variant={sso?.enabled ? "default" : "primary"} type="submit" disabled={login.isPending} className="w-full h-9">Sign in</Button>
      </form>}
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
          <Label text="Setup token (SS_SETUP_TOKEN)">
            <input value={token} onChange={(e) => setToken(e.target.value)} className={field} />
          </Label>
        )}
        {problem && <p role="alert" className="text-crit">{problem}</p>}
        <Button variant="primary" type="submit" disabled={setup.isPending} className="w-full h-9">Create account and continue</Button>
      </form>
    </Frame>
  );
}
