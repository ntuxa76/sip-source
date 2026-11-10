import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { CircleCheck, CircleX, KeyRound, Lock, Pencil, Plus, Trash2, X } from "lucide-react";
import { useMemo, useState } from "react";
import { Badge, Button, Card, ErrorNote, KV, Loading, PageHeader, Select, Tabs, Toggle } from "../components/ui";
import { del, get, post, put } from "../lib/api";
import { usePrefs } from "../lib/context";
import { fmtDateTime } from "../lib/format";

const SECTIONS = [
  { id: "users", label: "Users", perm: "users_manage" },
  { id: "roles", label: "Roles & permissions", perm: "roles_manage" },
  { id: "settings", label: "Settings", perm: "settings_manage" },
  { id: "storage", label: "Storage", perm: "storage_manage" },
  { id: "sources", label: "Sources", perm: "system_view" },
  { id: "system", label: "System health", perm: "system_view" },
  { id: "audit", label: "Audit log", perm: "audit_read" },
];

const field = "w-full h-8 rounded-md bg-sunken border border-line px-2 focus:border-accent focus:outline-none";

function useApi(section: string, enabled = true) {
  return useQuery({ queryKey: ["admin", section], queryFn: () => get(section === "audit" ? "/admin/audit" : `/admin/${section}`), enabled });
}

function SidePanel({ title, onClose, children }: { title: string; onClose: () => void; children: React.ReactNode }) {
  return (
    <div className="fixed inset-0 z-40 bg-black/40 flex justify-end" onClick={onClose}>
      <aside className="w-[560px] max-w-full bg-raised border-l border-line-2 h-full flex flex-col" onClick={(e) => e.stopPropagation()}>
        <header className="flex items-center justify-between px-4 h-12 border-b border-line shrink-0">
          <h3 className="font-semibold">{title}</h3>
          <button type="button" onClick={onClose} aria-label="Close"><X size={16} /></button>
        </header>
        <div className="flex-1 overflow-y-auto p-4">{children}</div>
      </aside>
    </div>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="block">
      <span className="text-xs text-ink-2">{label}</span>
      {children}
    </label>
  );
}

export function AdminPage({ section }: { section: string }) {
  const navigate = useNavigate();
  const { can } = usePrefs();
  const tabs = SECTIONS.filter((s) => can(s.perm));
  return (
    <div>
      <PageHeader title="Admin" />
      <Card>
        <div className="px-2"><Tabs tabs={tabs} active={section} onChange={(id) => navigate({ to: "/admin/$section", params: { section: id } } as any)} /></div>
        {section === "users" && <Users />}
        {section === "roles" && <Roles />}
        {section === "settings" && <SettingsPanel />}
        {section === "storage" && <Storage />}
        {["sources", "system"].includes(section) && <Sample section={section} />}
        {section === "audit" && <AuditLog />}
      </Card>
    </div>
  );
}

function Sample({ section }: { section: string }) {
  const { data, error } = useApi(section);
  if (error) return <ErrorNote error={error} />;
  if (!data) return <Loading />;
  return (
    <div>
      {section === "sources" && <Sources rows={data} />}
      {section === "system" && <System data={data} />}
    </div>
  );
}

type Tri = "inherit" | "allow" | "deny";

function PermissionOverrides({ catalogue, rolePerms, grants, denies, onChange }: { catalogue: any[]; rolePerms: string[]; grants: string[]; denies: string[]; onChange: (g: string[], d: string[]) => void }) {
  const groups = [...new Set(catalogue.map((p) => p.group))];
  const stateOf = (id: string): Tri => (grants.includes(id) ? "allow" : denies.includes(id) ? "deny" : "inherit");
  const set = (id: string, v: Tri) => {
    const g = grants.filter((x) => x !== id);
    const d = denies.filter((x) => x !== id);
    if (v === "allow") g.push(id);
    if (v === "deny") d.push(id);
    onChange(g, d);
  };
  return (
    <div>
      <h4 className="font-semibold mb-1">Permissions for this user</h4>
      <p className="text-ink-3 text-xs mb-2">Each one follows the role unless you allow or deny it here.</p>
      {groups.map((group) => (
        <div key={group} className="mb-3">
          <div className="text-xs font-medium text-ink-2 mb-1">{group}</div>
          {catalogue.filter((p) => p.group === group).map((p) => {
            const inherited = rolePerms.includes(p.id);
            const st = stateOf(p.id);
            const on = st === "allow" || (st === "inherit" && inherited);
            return (
              <div key={p.id} className="flex items-center gap-2 py-0.5" title={p.help}>
                {on ? <CircleCheck size={13} className="text-good shrink-0" aria-label="Allowed" /> : <CircleX size={13} className="text-ink-3 shrink-0" aria-label="Not allowed" />}
                <span className="flex-1 truncate">{p.label}</span>
                <Select label={`Override for ${p.label}`} value={st} onChange={(v) => set(p.id, v as Tri)} options={[{ value: "inherit", label: inherited ? "From role: allowed" : "From role: not allowed" }, { value: "allow", label: "Allow" }, { value: "deny", label: "Deny" }]} />
              </div>
            );
          })}
        </div>
      ))}
    </div>
  );
}

const blankUser = { id: 0, username: "", display_name: "", email: "", role_id: 0, status: "active", grants: [] as string[], denies: [] as string[], password: "" };

function Users() {
  const client = useQueryClient();
  const { tz, me, settings } = usePrefs();
  const users = useQuery({ queryKey: ["users"], queryFn: () => get("/users") });
  const roles = useQuery({ queryKey: ["roles"], queryFn: () => get("/roles") });
  const perms = useQuery({ queryKey: ["permissions"], queryFn: () => get("/permissions") });
  const [editing, setEditing] = useState<typeof blankUser | null>(null);
  const [resetFor, setResetFor] = useState<any>(null);
  const [password, setPassword] = useState("");
  const [problem, setProblem] = useState("");
  const refresh = () => client.invalidateQueries({ queryKey: ["users"] });

  const save = useMutation({
    mutationFn: (u: typeof blankUser) => (u.id ? put(`/users/${u.id}`, u) : post("/users", u)),
    onSuccess: () => { setEditing(null); setProblem(""); refresh(); },
    onError: (e: Error) => setProblem(e.message),
  });
  const remove = useMutation({ mutationFn: (id: number) => del(`/users/${id}`), onSuccess: refresh, onError: (e: Error) => setProblem(e.message) });
  const reset = useMutation({
    mutationFn: () => post(`/users/${resetFor.id}/password`, { password }),
    onSuccess: () => { setResetFor(null); setPassword(""); setProblem(""); },
    onError: (e: Error) => setProblem(e.message),
  });

  if (users.error) return <ErrorNote error={users.error} />;
  if (!users.data || !roles.data || !perms.data) return <Loading />;
  const role = roles.data.find((r: any) => r.id === editing?.role_id);

  return (
    <div>
      <div className="flex items-center justify-between px-3 py-2">
        <span className="text-ink-3 text-xs">{users.data.length} users</span>
        <Button variant="primary" onClick={() => { setProblem(""); setEditing({ ...blankUser, role_id: roles.data.find((r: any) => !r.locked)?.id ?? roles.data[0].id }); }}><Plus size={13} />Add user</Button>
      </div>
      {problem && !editing && !resetFor && <p className="text-crit px-3 pb-2">{problem}</p>}
      <div className="overflow-x-auto">
        <table className="data-table">
          <thead><tr><th>User</th><th>Role</th><th>Status</th><th>Overrides</th><th>Last login</th><th /></tr></thead>
          <tbody>
            {users.data.map((u: any) => (
              <tr key={u.id}>
                <td><div className="font-medium">{u.display_name || u.username}</div><div className="text-xs text-ink-3 font-mono">{u.username}</div></td>
                <td>{u.role}</td>
                <td><Badge tone={u.locked ? "warn" : u.status === "active" ? "good" : "neutral"}>{u.locked ? "locked" : u.status}</Badge></td>
                <td className="text-ink-2 text-xs">{u.grants.length + u.denies.length === 0 ? "None" : `${u.grants.length} allowed, ${u.denies.length} denied`}</td>
                <td>{u.last_login ? fmtDateTime(u.last_login, tz).slice(0, 16) : "Never"}</td>
                <td className="text-right">
                  <Button variant="ghost" onClick={() => { setProblem(""); setEditing({ ...blankUser, ...u, password: "" }); }}><Pencil size={13} />Edit</Button>
                  <Button variant="ghost" onClick={() => { setProblem(""); setResetFor(u); }}><KeyRound size={13} />Password</Button>
                  {u.id !== me.id && <Button variant="ghost" onClick={() => confirm(`Delete ${u.username}?`) && remove.mutate(u.id)}><Trash2 size={13} /></Button>}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {editing && (
        <SidePanel title={editing.id ? `Edit ${editing.username}` : "Add user"} onClose={() => setEditing(null)}>
          <form className="space-y-3" onSubmit={(e) => { e.preventDefault(); save.mutate(editing); }}>
            <div className="grid grid-cols-2 gap-2">
              <Row label="Username"><input value={editing.username} onChange={(e) => setEditing({ ...editing, username: e.target.value })} className={field} /></Row>
              <Row label="Display name"><input value={editing.display_name} onChange={(e) => setEditing({ ...editing, display_name: e.target.value })} className={field} /></Row>
              <Row label="Email"><input value={editing.email} onChange={(e) => setEditing({ ...editing, email: e.target.value })} className={field} /></Row>
              {!editing.id && <Row label={`Password, at least ${settings.password_min_length} characters`}><input type="password" autoComplete="new-password" value={editing.password} onChange={(e) => setEditing({ ...editing, password: e.target.value })} className={field} /></Row>}
              <Row label="Role"><Select className="w-full !h-8" value={String(editing.role_id)} onChange={(v) => setEditing({ ...editing, role_id: Number(v) })} options={roles.data.map((r: any) => ({ value: String(r.id), label: r.name }))} /></Row>
              <Row label="Status"><Select className="w-full !h-8" value={editing.status} onChange={(v) => setEditing({ ...editing, status: v })} options={[{ value: "active", label: "Active" }, { value: "disabled", label: "Disabled" }]} /></Row>
            </div>
            <PermissionOverrides catalogue={perms.data} rolePerms={role?.permissions ?? []} grants={editing.grants} denies={editing.denies} onChange={(g, d) => setEditing({ ...editing, grants: g, denies: d })} />
            {problem && <p className="text-crit">{problem}</p>}
            <Button variant="primary" type="submit" disabled={save.isPending}>Save</Button>
          </form>
        </SidePanel>
      )}
      {resetFor && (
        <SidePanel title={`New password for ${resetFor.username}`} onClose={() => setResetFor(null)}>
          <form className="space-y-3" onSubmit={(e) => { e.preventDefault(); reset.mutate(); }}>
            <Row label={`Password, at least ${settings.password_min_length} characters`}><input autoFocus type="password" autoComplete="new-password" value={password} onChange={(e) => setPassword(e.target.value)} className={field} /></Row>
            <p className="text-ink-3 text-xs">This signs the user out everywhere and clears any lockout.</p>
            {problem && <p className="text-crit">{problem}</p>}
            <Button variant="primary" type="submit">Set password</Button>
          </form>
        </SidePanel>
      )}
    </div>
  );
}

function Roles() {
  const client = useQueryClient();
  const roles = useQuery({ queryKey: ["roles"], queryFn: () => get("/roles") });
  const perms = useQuery({ queryKey: ["permissions"], queryFn: () => get("/permissions") });
  const [editing, setEditing] = useState<any>(null);
  const [problem, setProblem] = useState("");
  const refresh = () => client.invalidateQueries({ queryKey: ["roles"] });
  const save = useMutation({
    mutationFn: (r: any) => (r.id ? put(`/roles/${r.id}`, r) : post("/roles", r)),
    onSuccess: () => { setEditing(null); setProblem(""); refresh(); },
    onError: (e: Error) => setProblem(e.message),
  });
  const remove = useMutation({ mutationFn: (id: number) => del(`/roles/${id}`), onSuccess: refresh, onError: (e: Error) => setProblem(e.message) });

  if (roles.error) return <ErrorNote error={roles.error} />;
  if (!roles.data || !perms.data) return <Loading />;
  const groups = [...new Set<string>(perms.data.map((p: any) => p.group))];
  const toggle = (r: any, id: string) => save.mutate({ ...r, permissions: r.permissions.includes(id) ? r.permissions.filter((x: string) => x !== id) : [...r.permissions, id] });

  return (
    <div>
      <div className="flex items-center justify-between px-3 py-2">
        <span className="text-ink-3 text-xs">Tick a box to change a role. It takes effect at once. Users can also have individual overrides.</span>
        <Button variant="primary" onClick={() => { setProblem(""); setEditing({ id: 0, name: "", description: "", permissions: [] }); }}><Plus size={13} />New role</Button>
      </div>
      {problem && !editing && <p className="text-crit px-3 pb-2">{problem}</p>}
      <div className="overflow-x-auto">
        <table className="data-table">
          <thead>
            <tr>
              <th>Permission</th>
              {roles.data.map((r: any) => (
                <th key={r.id} className="text-center !whitespace-normal min-w-[92px]">
                  <div className="font-semibold text-ink">{r.name}</div>
                  <div className="font-normal">{r.users} users</div>
                  {r.locked ? <Lock size={11} className="inline mt-0.5" aria-label="Locked" /> : (
                    <span className="inline-flex gap-1 mt-0.5">
                      <button type="button" aria-label={`Edit ${r.name}`} title="Rename or describe" onClick={() => { setProblem(""); setEditing(r); }} className="hover:text-ink"><Pencil size={11} /></button>
                      <button type="button" aria-label={`Delete ${r.name}`} title="Delete" onClick={() => confirm(`Delete role ${r.name}?`) && remove.mutate(r.id)} className="hover:text-crit"><Trash2 size={11} /></button>
                    </span>
                  )}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {groups.map((g) => [
              <tr key={g}><td colSpan={roles.data.length + 1} className="!bg-sunken text-xs font-medium text-ink-2">{g}</td></tr>,
              ...perms.data.filter((p: any) => p.group === g).map((p: any) => (
                <tr key={p.id}>
                  <td title={p.help}><div>{p.label}</div><div className="text-[11.5px] text-ink-3 whitespace-normal max-w-[340px]">{p.help}</div></td>
                  {roles.data.map((r: any) => (
                    <td key={r.id} className="text-center">
                      <input type="checkbox" aria-label={`${p.label} for ${r.name}`} checked={r.permissions.includes(p.id)} disabled={r.locked} onChange={() => toggle(r, p.id)} />
                    </td>
                  ))}
                </tr>
              )),
            ])}
          </tbody>
        </table>
      </div>
      {editing && (
        <SidePanel title={editing.id ? `Edit ${editing.name}` : "New role"} onClose={() => setEditing(null)}>
          <form className="space-y-3" onSubmit={(e) => { e.preventDefault(); save.mutate(editing); }}>
            <Row label="Name"><input autoFocus value={editing.name} onChange={(e) => setEditing({ ...editing, name: e.target.value })} className={field} /></Row>
            <Row label="Description"><input value={editing.description} onChange={(e) => setEditing({ ...editing, description: e.target.value })} className={field} /></Row>
            {!editing.id && (
              <Row label="Start from">
                <Select className="w-full !h-8" value="" onChange={(v) => setEditing({ ...editing, permissions: roles.data.find((r: any) => String(r.id) === v)?.permissions ?? [] })} options={[{ value: "", label: "No permissions" }, ...roles.data.map((r: any) => ({ value: String(r.id), label: `Copy of ${r.name}` }))]} />
              </Row>
            )}
            {problem && <p className="text-crit">{problem}</p>}
            <Button variant="primary" type="submit">Save</Button>
          </form>
        </SidePanel>
      )}
    </div>
  );
}

function SettingRow({ s, onSaved }: { s: any; onSaved: () => void }) {
  const [value, setValue] = useState<string>(s.value);
  const [problem, setProblem] = useState("");
  const locked = s.source === "env";
  const save = useMutation({
    mutationFn: (v: string | null) => put(`/settings/${s.key}`, { value: v }),
    onSuccess: () => { setProblem(""); onSaved(); },
    onError: (e: Error) => setProblem(e.message),
  });
  const commit = (v: string) => v !== s.value && save.mutate(v);
  return (
    <div className="grid md:grid-cols-[1fr_260px] gap-x-4 gap-y-1 px-3 py-2.5 border-b border-line last:border-b-0">
      <div>
        <div className="font-medium flex items-center gap-1.5 flex-wrap">
          {s.label}
          {s.restart && <Badge tone="warn">restart needed</Badge>}
        </div>
        <p className="text-xs text-ink-2">{s.help}</p>
        <code className="text-[11px] text-ink-3 font-mono">{s.env}</code>
      </div>
      <div>
        {s.kind === "bool" ? (
          <Toggle checked={value === "true"} onChange={(v) => { if (locked) return; setValue(String(v)); save.mutate(String(v)); }}>{value === "true" ? "On" : "Off"}</Toggle>
        ) : s.kind === "choice" ? (
          <Select className="w-full !h-8" value={value} onChange={(v) => { setValue(v); save.mutate(v); }} options={s.options.map((o: string) => ({ value: o, label: o }))} />
        ) : (
          <input disabled={locked} type={s.kind === "string" ? "text" : "number"} min={s.min ?? undefined} max={s.max ?? undefined} value={value} onChange={(e) => setValue(e.target.value)} onBlur={() => commit(value)} onKeyDown={(e) => e.key === "Enter" && commit(value)} className={`${field} disabled:opacity-60`} />
        )}
        <div className="flex items-center gap-2 mt-1 text-[11px] text-ink-3">
          {locked ? <span><Lock size={10} className="inline -mt-0.5 mr-0.5" aria-hidden />Set by environment variable</span> : s.source === "database" ? (
            <>
              <span>Saved in database</span>
              <button type="button" onClick={() => { setValue(s.default); save.mutate(null); }} className="text-accent hover:underline">Reset to default</button>
            </>
          ) : <span>Default</span>}
          {save.isSuccess && <span className="text-good">Saved</span>}
        </div>
        {problem && <p className="text-crit text-xs mt-1">{problem}</p>}
      </div>
    </div>
  );
}

function SettingsPanel() {
  const client = useQueryClient();
  const { data, error } = useQuery({ queryKey: ["settings"], queryFn: () => get("/settings") });
  const [group, setGroup] = useState("Branding");
  const groups = useMemo(() => [...new Set<string>((data ?? []).map((s: any) => s.group))], [data]);
  if (error) return <ErrorNote error={error} />;
  if (!data) return <Loading />;
  const refresh = () => {
    client.invalidateQueries({ queryKey: ["settings"] });
    client.invalidateQueries({ queryKey: ["me"] });
  };
  return (
    <div className="flex">
      <nav className="w-[190px] shrink-0 border-r border-line p-2">
        {groups.map((g) => (
          <button type="button" key={g} onClick={() => setGroup(g)} className={`block w-full text-left px-2.5 py-1.5 rounded-md ${group === g ? "bg-accent-wash font-medium" : "text-ink-2 hover:bg-sunken"}`}>{g}</button>
        ))}
      </nav>
      <div className="flex-1 min-w-0">
        <p className="text-ink-3 text-xs px-3 pt-2.5">Every setting can also be set with its environment variable, which then takes priority over the value here.</p>
        {data.filter((s: any) => s.group === group).map((s: any) => <SettingRow key={s.key} s={s} onSaved={refresh} />)}
      </div>
    </div>
  );
}

const blankStorage = { id: 0, name: "", type: "clickhouse", config: { host: "", port: 8123, database: "sip_source", username: "default", tls: false }, password: "" };

const BACKENDS = [
  { value: "local", label: "Local file", text: "Stored on this server in the data volume. Nothing else to run." },
  { value: "clickhouse", label: "ClickHouse server", text: "Your own ClickHouse, for more traffic, longer history or several panels." },
];

function Storage() {
  const client = useQueryClient();
  const { tz } = usePrefs();
  const { data, error } = useQuery({ queryKey: ["storages"], queryFn: () => get("/storages") });
  const [editing, setEditing] = useState<any>(null);
  const [problem, setProblem] = useState("");
  const [results, setResults] = useState<Record<number, any>>({});
  const refresh = () => {
    client.invalidateQueries({ queryKey: ["storages"] });
    client.invalidateQueries({ queryKey: ["active-storage"] });
  };
  const save = useMutation({
    mutationFn: (s: any) => (s.id ? put(`/storages/${s.id}`, s) : post("/storages", s)),
    onSuccess: () => { setEditing(null); setProblem(""); refresh(); },
    onError: (e: Error) => setProblem(e.message),
  });
  const test = useMutation({ mutationFn: (id: number) => post(`/storages/${id}/test`), onSuccess: (r, id) => { setResults((x) => ({ ...x, [id]: r })); refresh(); } });
  const activate = useMutation({ mutationFn: (id: number) => post(`/storages/${id}/activate`), onSuccess: refresh, onError: (e: Error) => setProblem(e.message) });
  const remove = useMutation({ mutationFn: (id: number) => del(`/storages/${id}`), onSuccess: refresh, onError: (e: Error) => setProblem(e.message) });
  if (error) return <ErrorNote error={error} />;
  if (!data) return <Loading />;
  const setC = (patch: any) => setEditing({ ...editing, config: { ...editing.config, ...patch } });
  return (
    <div>
      <div className="flex items-center justify-between gap-3 px-3 py-2">
        <p className="text-ink-2 text-xs max-w-[720px]">Where captured data lives. The built-in Local file storage works out of the box; add a ClickHouse server when you need more. Selecting a storage creates its tables if needed, and new traffic is written there.</p>
        <Button variant="primary" onClick={() => { setProblem(""); setEditing({ ...blankStorage }); }}><Plus size={13} />Add storage</Button>
      </div>
      {!data.encryption_key_set && <p className="text-warn text-xs px-3 pb-2">VP_SECRET_KEY isn't set, so storage passwords are kept unencrypted in MariaDB.</p>}
      {problem && !editing && <p className="text-crit px-3 pb-2">{problem}</p>}
      <div className="px-3 pb-3 space-y-2">
        {data.storages.map((s: any) => {
          const r = results[s.id];
          return (
            <div key={s.id} className={`border rounded-md p-3 ${s.active ? "border-accent" : "border-line"}`}>
              <div className="flex items-center gap-2 flex-wrap">
                <span className="font-semibold">{s.name}</span>
                <Badge>{s.type === "local" ? "Local file" : "ClickHouse"}</Badge>
                {s.built_in && <Badge>Built in</Badge>}
                {s.active && <Badge tone="info">In use</Badge>}
                <div className="flex-1" />
                <Button onClick={() => test.mutate(s.id)} disabled={test.isPending}>Test connection</Button>
                {!s.active && <Button variant="primary" onClick={() => activate.mutate(s.id)}>Use this storage</Button>}
                {!s.built_in && <Button onClick={() => { setProblem(""); setEditing({ ...s, password: "" }); }}><Pencil size={13} />Edit</Button>}
                {!s.built_in && !s.active && <Button onClick={() => confirm(`Remove ${s.name}?`) && remove.mutate(s.id)}><Trash2 size={13} /></Button>}
              </div>
              {s.type === "local" && <p className="text-xs text-ink-2 mt-1">On this server, database <span className="font-mono">{s.config.database}</span></p>}
              {s.type === "clickhouse" && <p className="text-xs text-ink-2 font-mono mt-1">{s.config.tls ? "https" : "http"}://{s.config.host}:{s.config.port}/{s.config.database} as {s.config.username}{s.encrypted ? " · password encrypted" : ""}</p>}
              {(r || s.last_test_at) && (
                <p className={`text-xs mt-1 ${(r?.ok ?? s.last_test_ok) ? "text-good" : "text-crit"}`}>
                  {r?.message ?? s.last_test_message}
                  {!r && s.last_test_at && <span className="text-ink-3"> · {fmtDateTime(s.last_test_at, tz).slice(5, 16)}</span>}
                </p>
              )}
            </div>
          );
        })}
      </div>
      {editing && (
        <SidePanel title={editing.id ? `Edit ${editing.name}` : "Add storage"} onClose={() => setEditing(null)}>
          <form className="space-y-3" onSubmit={(e) => { e.preventDefault(); save.mutate(editing); }}>
            {!editing.id && (
              <fieldset>
                <legend className="text-xs text-ink-2 mb-1.5">Backend</legend>
                <div className="grid grid-cols-2 gap-2">
                  {BACKENDS.map((b) => (
                    <label key={b.value} className={`rounded-lg border p-2.5 cursor-pointer ${editing.type === b.value ? "border-accent bg-accent-wash" : "border-line hover:border-line-2"}`}>
                      <input type="radio" name="backend" value={b.value} checked={editing.type === b.value} onChange={() => setEditing({ ...editing, type: b.value })} className="sr-only" />
                      <span className="block font-medium text-[12.5px]">{b.label}</span>
                      <span className="block text-[11.5px] text-ink-3 mt-0.5 leading-snug">{b.text}</span>
                    </label>
                  ))}
                </div>
              </fieldset>
            )}
            <Row label="Name"><input autoFocus value={editing.name} onChange={(e) => setEditing({ ...editing, name: e.target.value })} className={field} /></Row>
            {editing.type === "local" ? (
              <Row label="Database"><input value={editing.config.database} onChange={(e) => setC({ database: e.target.value })} className={field} /></Row>
            ) : (
            <>
            <div className="grid grid-cols-[1fr_110px] gap-2">
              <Row label="Host"><input value={editing.config.host} onChange={(e) => setC({ host: e.target.value })} placeholder="clickhouse.example.net" className={field} /></Row>
              <Row label="HTTP port"><input type="number" value={editing.config.port} onChange={(e) => setC({ port: Number(e.target.value) })} className={field} /></Row>
            </div>
            <Row label="Database"><input value={editing.config.database} onChange={(e) => setC({ database: e.target.value })} className={field} /></Row>
            <div className="grid grid-cols-2 gap-2">
              <Row label="Username"><input value={editing.config.username} onChange={(e) => setC({ username: e.target.value })} className={field} /></Row>
              <Row label={editing.id ? "Password (blank keeps the current one)" : "Password"}><input type="password" autoComplete="new-password" value={editing.password} onChange={(e) => setEditing({ ...editing, password: e.target.value })} className={field} /></Row>
            </div>
            <Toggle checked={editing.config.tls} onChange={(v) => setC({ tls: v })}>Use HTTPS</Toggle>
            </>
            )}
            {problem && <p className="text-crit">{problem}</p>}
            <div><Button variant="primary" type="submit">Save</Button></div>
          </form>
        </SidePanel>
      )}
    </div>
  );
}

function Sources({ rows }: { rows: any[] }) {
  const { tz, can } = usePrefs();
  const client = useQueryClient();
  const rename = useMutation({
    mutationFn: ({ ip, alias }: { ip: string; alias: string }) => put(`/admin/sources/${encodeURIComponent(ip)}`, { alias }),
    onSuccess: () => {
      client.invalidateQueries({ queryKey: ["admin", "sources"] });
      client.invalidateQueries({ queryKey: ["sources"] });
    },
  });
  if (rows.length === 0) return <p className="text-ink-2 p-4">No HEP received yet. Point your SIP servers at port 9060 of this server; each sender appears here by its address.</p>;
  return (
    <div>
      <p className="text-ink-2 text-xs px-4 pt-3 pb-2 max-w-[720px]">Every server that sends HEP, by address. Give each one an alias, such as Asterisk-1 or OpenSIPS-edge; the source picker at the top shows it. The alias names new traffic, already stored calls keep the name they had.</p>
      <table className="data-table">
        <thead><tr><th>Address</th><th>Alias</th><th>Capture ID</th><th>HEP</th><th className="num">Packets</th><th className="num">Clock offset</th><th>Last seen</th><th>Status</th></tr></thead>
        <tbody>{rows.map((r) => (
          <tr key={r.ip}>
            <td className="font-mono text-xs">{r.ip}</td>
            <td>{can("settings_manage") ? <input aria-label={`Alias for ${r.ip}`} defaultValue={r.alias} placeholder="Add an alias" onBlur={(e) => { const alias = e.target.value.trim(); if (alias !== r.alias) rename.mutate({ ip: r.ip, alias }); }} onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()} className="h-8 w-[200px] rounded-lg bg-raised border border-line-2 px-2" /> : (r.alias || <span className="text-ink-3">–</span>)}</td>
            <td className="tnum">{r.capture_id}</td>
            <td>v{r.hep_version}</td>
            <td className="num">{r.packets.toLocaleString()}</td>
            <td className="num">{r.offset_ms} ms</td>
            <td>{r.last_seen ? fmtDateTime(r.last_seen, tz).slice(5) : "before restart"}</td>
            <td><Badge tone={r.status === "active" ? "good" : r.status === "clock skew" ? "warn" : "neutral"}>{r.status}</Badge></td>
          </tr>
        ))}</tbody>
      </table>
    </div>
  );
}

function AuditLog() {
  const { tz } = usePrefs();
  const { data, error } = useApi("audit");
  if (error) return <ErrorNote error={error} />;
  if (!data) return <Loading />;
  const tone = (o: string) => (o === "success" ? "good" : o === "denied" || o === "failure" ? "crit" : "warn") as "good" | "crit" | "warn";
  return (
    <div className="overflow-auto max-h-[calc(100vh-190px)]">
      <table className="data-table">
        <thead><tr><th>Time</th><th>Actor</th><th>Source</th><th>Action</th><th>Object</th><th>Outcome</th><th>Details</th></tr></thead>
        <tbody>{data.map((r: any) => <tr key={r.id}><td>{fmtDateTime(r.ts, tz).slice(5)}</td><td>{r.actor}</td><td className="font-mono text-xs">{r.src_ip}</td><td className="font-mono text-xs">{r.action}</td><td className="text-ink-2 font-mono text-xs">{r.object}</td><td><Badge tone={tone(r.outcome)}>{r.outcome}</Badge></td><td className="text-ink-2 text-xs max-w-[320px] truncate" title={r.details}>{r.details}</td></tr>)}</tbody>
      </table>
    </div>
  );
}

function System({ data }: { data: any }) {
  const ch = data.clickhouse;
  return (
    <div className="p-3 space-y-3">
      <div className="grid md:grid-cols-2 gap-x-6">
        <KV label="Version">{data.version}</KV>
        <KV label="MariaDB">{data.mariadb}</KV>
        <KV label="HEP UDP">{data.listeners.hep_udp || "off"}</KV>
        <KV label="HEP TCP">{data.listeners.hep_tcp || "off"}</KV>
        <KV label="HEP auth key">{data.auth_key ? "required" : "not set"}</KV>
        <KV label="Storage engine">{ch ? `ClickHouse ${ch.version} · ${ch.database}` : "starting"}</KV>
      </div>
      {ch && ch.tables.length > 0 && (
        <table className="w-full">
          <thead><tr><th>Table</th><th>Rows</th><th>Size</th><th>Parts</th></tr></thead>
          <tbody>{ch.tables.map((t: any) => <tr key={t.table}><td className="font-mono text-xs">{t.table}</td><td className="tnum">{Number(t.rows).toLocaleString()}</td><td className="tnum">{(Number(t.bytes) / 1048576).toFixed(1)} MB</td><td className="tnum">{t.parts}</td></tr>)}</tbody>
        </table>
      )}
      <div>
        <h3 className="font-semibold mb-1">Dropped packets since start</h3>
        {data.drops.length === 0 ? <p className="text-ink-2">None</p> : <ul>{data.drops.map((d: any) => <li key={d.reason} className="font-mono text-xs">{d.reason}: {d.count.toLocaleString()}</li>)}</ul>}
      </div>
    </div>
  );
}

export function PreferencesPage() {
  const { prefs, savePrefs, settings, tz, crosshair, density, me } = usePrefs();
  const zones = [...new Set([tz, ...settings.time_zones.split(",").map((s) => s.trim()).filter(Boolean)])];
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [note, setNote] = useState("");
  const change = useMutation({
    mutationFn: () => post("/auth/me/password", { current, new: next }),
    onSuccess: () => { setCurrent(""); setNext(""); setNote("Password changed."); },
    onError: (e: Error) => setNote(e.message),
  });
  return (
    <div className="max-w-[620px]">
      <PageHeader title="Preferences" description={`Signed in as ${me.username}, role ${me.role}.`} />
      <Card title="Display">
        <div className="px-3.5 pb-3.5 space-y-3">
          <Toggle checked={crosshair} onChange={(v) => savePrefs({ crosshair: v })}>Link charts on hover: pointing at one chart shows the crosshair on every chart of the dashboard</Toggle>
          <div className="grid grid-cols-3 gap-2">
            <Row label="Theme"><Select className="w-full !h-8" value={prefs.theme ?? "system"} onChange={(v) => savePrefs({ theme: v as "light" | "dark" | "system" })} options={[{ value: "system", label: "Follow system" }, { value: "light", label: "Light" }, { value: "dark", label: "Dark" }]} /></Row>
            <Row label="Time zone"><Select className="w-full !h-8" value={tz} onChange={(v) => savePrefs({ tz: v })} options={zones.map((z) => ({ value: z, label: z }))} /></Row>
            <Row label="Table rows"><Select className="w-full !h-8" value={density} onChange={(v) => savePrefs({ density: v as "compact" | "comfortable" })} options={[{ value: "comfortable", label: "Comfortable" }, { value: "compact", label: "Compact" }]} /></Row>
          </div>
        </div>
      </Card>
      <Card title="Change password" className="mt-3">
        <form className="px-3.5 pb-3.5 space-y-2" onSubmit={(e) => { e.preventDefault(); change.mutate(); }}>
          <Row label="Current password"><input type="password" autoComplete="current-password" value={current} onChange={(e) => setCurrent(e.target.value)} className={field} /></Row>
          <Row label={`New password, at least ${settings.password_min_length} characters`}><input type="password" autoComplete="new-password" value={next} onChange={(e) => setNext(e.target.value)} className={field} /></Row>
          {note && <p className="text-xs text-ink-2">{note}</p>}
          <Button type="submit" variant="primary">Change password</Button>
        </form>
      </Card>
    </div>
  );
}
