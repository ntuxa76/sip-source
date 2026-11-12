import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { Check, CircleCheck, CircleX, Copy, KeyRound, Lock, Pencil, Plus, Trash2, X } from "lucide-react";
import { useMemo, useState } from "react";
import { Badge, Button, Card, Dialog, ErrorNote, Field, KV, Loading, PageHeader, Select, Tabs, Toggle, IconButton, Status, fieldClass, inputClass } from "../components/ui";
import { del, get, post, put } from "../lib/api";
import { Busy } from "../components/Orb";
import { ADMIN_SECTIONS } from "../lib/access";
import { usePrefs } from "../lib/context";
import { fmtDateTime } from "../lib/format";


const field = inputClass;

function useApi(section: string, enabled = true) {
  return useQuery({ queryKey: ["admin", section], queryFn: () => get(section === "audit" ? "/admin/audit" : `/admin/${section}`), enabled });
}

function SidePanel({ title, onClose, children }: { title: string; onClose: () => void; children: React.ReactNode }) {
  return (
    <div className="fixed inset-0 z-40 bg-scrim flex justify-end" onClick={onClose}>
      <aside role="dialog" aria-modal="true" aria-label={title} className="w-[560px] max-w-full bg-raised border-l border-line-2 shadow-pop h-full flex flex-col" onClick={(e) => e.stopPropagation()}>
        <header className="flex items-center justify-between gap-3 pl-4 pr-2 h-12 border-b border-line shrink-0">
          <h2 className="text-sm font-semibold truncate">{title}</h2>
          <IconButton label="Close" onClick={onClose}><X size={16} aria-hidden /></IconButton>
        </header>
        <div className="flex-1 overflow-y-auto p-4">{children}</div>
      </aside>
    </div>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="block">
      <span className="block mb-1 text-xs font-medium text-ink-2">{label}</span>
      {children}
    </label>
  );
}

export function AdminPage({ section }: { section: string }) {
  const navigate = useNavigate();
  const { can } = usePrefs();
  const tabs = ADMIN_SECTIONS.filter((s) => can(s.perm));
  return (
    <div>
      <PageHeader title="Admin" />
      <Card>
        <div><Tabs tabs={tabs} active={section} onChange={(id) => navigate({ to: "/admin/$section", params: { section: id } } as any)} /></div>
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
                {on ? <CircleCheck size={14} className="text-good shrink-0" aria-label="Allowed" /> : <CircleX size={14} className="text-ink-3 shrink-0" aria-label="Not allowed" />}
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

const blankUser = { id: 0, username: "", display_name: "", email: "", role_id: 0, status: "active", grants: [] as string[], denies: [] as string[], password: "", sso: false };

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
        <Button variant="primary" onClick={() => { setProblem(""); setEditing({ ...blankUser, role_id: roles.data.find((r: any) => !r.locked)?.id ?? roles.data[0].id }); }}><Plus size={14} />Add user</Button>
      </div>
      {problem && !editing && !resetFor && <p className="text-crit px-3 pb-2">{problem}</p>}
      <div className="overflow-x-auto">
        <table className="data-table">
          <thead><tr><th>User</th><th>Sign-in</th><th>Role</th><th>Status</th><th>Overrides</th><th>Last login</th><th /></tr></thead>
          <tbody>
            {users.data.map((u: any) => (
              <tr key={u.id}>
                <td><div className="font-medium">{u.display_name || u.username}</div><div className="text-xs text-ink-3 font-mono">{u.username}</div></td>
                <td>{u.sso ? <span title={u.linked ? "Linked to the sign-in provider" : "Linked on the first single sign-on"}>Single sign-on</span> : <span className="text-ink-2">Password</span>}</td>
                <td>{u.role}</td>
                <td><Status tone={u.locked ? "warn" : u.status === "active" ? "good" : "neutral"}>{u.locked ? "locked" : u.status}</Status></td>
                <td className="text-ink-2 text-xs">{u.grants.length + u.denies.length === 0 ? "None" : `${u.grants.length} allowed, ${u.denies.length} denied`}</td>
                <td>{u.last_login ? fmtDateTime(u.last_login, tz).slice(0, 16) : "Never"}</td>
                <td className="text-right">
                  <Button variant="ghost" onClick={() => { setProblem(""); setEditing({ ...blankUser, ...u, password: "" }); }}><Pencil size={14} />Edit</Button>
                  {!u.sso && <Button variant="ghost" onClick={() => { setProblem(""); setResetFor(u); }}><KeyRound size={14} />Password</Button>}
                  {u.id !== me.id && <Button variant="ghost" onClick={() => confirm(`Delete ${u.username}?`) && remove.mutate(u.id)}><Trash2 size={14} /></Button>}
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
              {!editing.id && !editing.sso && <Row label={`Password, at least ${settings.password_min_length} characters`}><input type="password" autoComplete="new-password" value={editing.password} onChange={(e) => setEditing({ ...editing, password: e.target.value })} className={field} /></Row>}
              <Row label="Role"><Select className="w-full" value={String(editing.role_id)} onChange={(v) => setEditing({ ...editing, role_id: Number(v) })} options={roles.data.map((r: any) => ({ value: String(r.id), label: r.name }))} /></Row>
              <Row label="Status"><Select className="w-full" value={editing.status} onChange={(v) => setEditing({ ...editing, status: v })} options={[{ value: "active", label: "Active" }, { value: "disabled", label: "Disabled" }]} /></Row>
            </div>
            {!editing.id && (
              <Toggle checked={editing.sso} onChange={(v) => setEditing({ ...editing, sso: v, password: "" })}>
                Signs in with single sign-on only. The account is linked on the first sign-in with this username.
              </Toggle>
            )}
            <PermissionOverrides catalogue={perms.data} rolePerms={role?.permissions ?? []} grants={editing.grants} denies={editing.denies} onChange={(g, d) => setEditing({ ...editing, grants: g, denies: d })} />
            {problem && <p role="alert" className="text-crit">{problem}</p>}
            <Button variant="primary" type="submit" disabled={save.isPending}>Save</Button>
          </form>
        </SidePanel>
      )}
      {resetFor && (
        <SidePanel title={`New password for ${resetFor.username}`} onClose={() => setResetFor(null)}>
          <form className="space-y-3" onSubmit={(e) => { e.preventDefault(); reset.mutate(); }}>
            <Row label={`Password, at least ${settings.password_min_length} characters`}><input autoFocus type="password" autoComplete="new-password" value={password} onChange={(e) => setPassword(e.target.value)} className={field} /></Row>
            <p className="text-ink-3 text-xs">This signs the user out everywhere and clears any lockout.</p>
            {problem && <p role="alert" className="text-crit">{problem}</p>}
            <Button variant="primary" type="submit">Set password</Button>
          </form>
        </SidePanel>
      )}
    </div>
  );
}

function RoleDetailsDialog({ initial, roles, onClose, onSaved }: { initial: any; roles: any[]; onClose: () => void; onSaved: (id: number) => void }) {
  const [role, setRole] = useState(initial);
  const [problem, setProblem] = useState("");
  const save = useMutation({
    mutationFn: (r: any) => (r.id ? put(`/roles/${r.id}`, r) : post("/roles", r)),
    onSuccess: (res: any) => onSaved(role.id || res.id),
    onError: (e: Error) => setProblem(e.message),
  });
  return (
    <Dialog
      title={role.id ? `Edit ${initial.name}` : "New role"}
      onClose={onClose}
      width={480}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" type="submit" form="role-details" disabled={save.isPending}>{role.id ? "Save" : "Create role"}</Button>
        </>
      }
    >
      <form id="role-details" className="space-y-3" onSubmit={(e) => { e.preventDefault(); save.mutate(role); }}>
        <Field label="Name"><input value={role.name} maxLength={64} onChange={(e) => setRole({ ...role, name: e.target.value })} className={inputClass} /></Field>
        <Field label="Description" hint="Shown next to the role when assigning it to a user.">
          <textarea value={role.description} maxLength={255} rows={2} onChange={(e) => setRole({ ...role, description: e.target.value })} className={`${fieldClass} block w-full px-2.5 py-1.5`} />
        </Field>
        {!role.id && (
          <Field label="Start from">
            <Select className="w-full" value={role.from ?? ""} onChange={(v) => setRole({ ...role, from: v, permissions: roles.find((r: any) => String(r.id) === v)?.permissions ?? [] })} options={[{ value: "", label: "No permissions" }, ...roles.map((r: any) => ({ value: String(r.id), label: `Same permissions as ${r.name}` }))]} />
          </Field>
        )}
        {problem && <p role="alert" className="text-crit">{problem}</p>}
      </form>
    </Dialog>
  );
}

function Roles() {
  const client = useQueryClient();
  const { can } = usePrefs();
  const roles = useQuery({ queryKey: ["roles"], queryFn: () => get("/roles") });
  const perms = useQuery({ queryKey: ["permissions"], queryFn: () => get("/permissions") });
  const [view, setView] = useState<"edit" | "compare">("edit");
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [draft, setDraft] = useState<string[] | null>(null);
  const [dialog, setDialog] = useState<any>(null);
  const [problem, setProblem] = useState("");
  const refresh = () => client.invalidateQueries({ queryKey: ["roles"] });
  const save = useMutation({
    mutationFn: (r: any) => put(`/roles/${r.id}`, r),
    onSuccess: () => { setDraft(null); setProblem(""); refresh(); },
    onError: (e: Error) => setProblem(e.message),
  });
  const remove = useMutation({
    mutationFn: (id: number) => del(`/roles/${id}`),
    onSuccess: () => { setSelectedId(null); setProblem(""); refresh(); },
    onError: (e: Error) => setProblem(e.message),
  });

  if (roles.error) return <ErrorNote error={roles.error} />;
  if (!roles.data || !perms.data) return <Loading />;
  const list: any[] = roles.data;
  const catalogue: any[] = perms.data;
  const groups = [...new Set<string>(catalogue.map((p) => p.group))];
  const role = list.find((r) => r.id === selectedId) ?? list[0];
  const current: string[] = draft ?? role.permissions;
  const changed = draft !== null && (draft.length !== role.permissions.length || draft.some((p) => !role.permissions.includes(p)));
  const select = (id: number) => {
    if (changed && !confirm("Discard the unsaved changes to this role?")) return;
    setSelectedId(id);
    setDraft(null);
    setProblem("");
  };
  const toggle = (id: string) => setDraft(current.includes(id) ? current.filter((x) => x !== id) : [...current, id]);
  const toggleGroup = (g: string, on: boolean) => {
    const ids = catalogue.filter((p) => p.group === g && (!on || can(p.id))).map((p) => p.id);
    setDraft(on ? [...new Set([...current, ...ids])] : current.filter((x) => !ids.includes(x)));
  };

  return (
    <div>
      <div className="flex flex-wrap items-center justify-between gap-2 px-3 py-2.5 border-b border-line">
        <p className="text-ink-2 text-xs max-w-[640px]">A role is a named set of permissions. Every user has one role, and can have individual exceptions on the Users tab.</p>
        <div className="flex items-center gap-2">
          <div role="group" aria-label="View" className="inline-flex rounded-md border border-line-2 p-0.5">
            {(["edit", "compare"] as const).map((v) => (
              <button key={v} type="button" aria-pressed={view === v} onClick={() => setView(v)} className={`h-6 px-2.5 rounded-sm text-xs font-medium ${view === v ? "bg-accent-wash text-ink" : "text-ink-2 hover:text-ink"}`}>
                {v === "edit" ? "Edit" : "Compare"}
              </button>
            ))}
          </div>
          <Button variant="primary" size="sm" onClick={() => setDialog({ id: 0, name: "", description: "", permissions: [] })}><Plus size={14} />New role</Button>
        </div>
      </div>
      {problem && <p role="alert" className="text-crit px-3 pt-2">{problem}</p>}

      {view === "compare" ? (
        <div className="overflow-x-auto">
          <table className="data-table table-fixed min-w-[720px]">
            <colgroup>
              <col className="w-[300px]" />
              {list.map((r) => <col key={r.id} />)}
            </colgroup>
            <thead>
              <tr>
                <th>Permission</th>
                {list.map((r) => (
                  <th key={r.id} className="!text-center">
                    <button type="button" onClick={() => { select(r.id); setView("edit"); }} className="font-semibold text-ink hover:underline">{r.name}</button>
                    <div className="font-normal text-ink-3 tnum">{r.users} {r.users === 1 ? "user" : "users"}</div>
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {groups.map((g) => [
                <tr key={g} className="pointer-events-none"><td colSpan={list.length + 1} className="!bg-sunken !py-1 text-2xs font-semibold uppercase tracking-[0.06em] text-ink-3">{g}</td></tr>,
                ...catalogue.filter((p) => p.group === g).map((p) => (
                  <tr key={p.id}>
                    <td className="!whitespace-normal" title={p.help}>{p.label}</td>
                    {list.map((r) => (
                      <td key={r.id} className="text-center">
                        {r.permissions.includes(p.id) ? <Check size={16} className="inline text-accent" aria-label="Allowed" /> : <span className="text-ink-3" aria-label="Not allowed">–</span>}
                      </td>
                    ))}
                  </tr>
                )),
              ])}
            </tbody>
          </table>
        </div>
      ) : (
        <div className="grid md:grid-cols-[260px_minmax(0,1fr)]">
          <nav aria-label="Roles" className="md:border-r border-b md:border-b-0 border-line py-1.5">
            {list.map((r) => {
              const on = r.id === role.id;
              return (
                <button
                  key={r.id}
                  type="button"
                  aria-current={on ? "true" : undefined}
                  onClick={() => select(r.id)}
                  className={`w-full text-left pl-2.5 pr-3 py-2 border-l-2 focus-visible:-outline-offset-2 ${on ? "border-accent bg-accent-wash" : "border-transparent hover:bg-hover"}`}
                >
                  <span className="flex items-center gap-1.5">
                    <span className="font-medium truncate">{r.name}</span>
                    {r.locked && <Lock size={12} className="text-ink-3 shrink-0" aria-label="Fixed role" />}
                    <span className="ml-auto text-xs text-ink-3 tnum shrink-0">{r.users} {r.users === 1 ? "user" : "users"}</span>
                  </span>
                  <span className="text-xs text-ink-3 line-clamp-2">{r.description || "No description"}</span>
                </button>
              );
            })}
          </nav>
          <section className="min-w-0" aria-label={`Permissions of ${role.name}`}>
            <header className="flex flex-wrap items-start justify-between gap-3 px-4 py-3 border-b border-line">
              <div className="min-w-0">
                <h2 className="text-md font-semibold">{role.name}</h2>
                <p className="text-ink-2 max-w-[70ch]">{role.description || "No description"}</p>
              </div>
              <div className="flex flex-wrap gap-2">
                {!role.locked && <Button size="sm" onClick={() => setDialog(role)}><Pencil size={14} />Rename</Button>}
                <Button size="sm" onClick={() => setDialog({ id: 0, name: `${role.name} copy`, description: role.description, permissions: role.permissions, from: String(role.id) })}><Copy size={14} />Duplicate</Button>
                {!role.locked && (
                  <Button size="sm" variant="danger" disabled={role.users > 0} title={role.users > 0 ? "Move its users to another role first" : undefined} onClick={() => confirm(`Delete the role ${role.name}?`) && remove.mutate(role.id)}>
                    <Trash2 size={14} />Delete
                  </Button>
                )}
              </div>
            </header>
            {role.locked && (
              <p className="flex items-center gap-2 mx-4 mt-3 px-3 py-2 rounded-md bg-info-wash text-info text-xs"><Lock size={14} aria-hidden />This role always has every permission, so the system can't be left without an administrator.</p>
            )}
            <div className="px-4 pb-2">
              {groups.map((g) => {
                const items = catalogue.filter((p) => p.group === g);
                const all = items.every((p) => current.includes(p.id));
                return (
                  <fieldset key={g} className="mt-3">
                    <div className="flex items-center justify-between h-7 border-b border-line">
                      <legend className="text-2xs font-semibold uppercase tracking-[0.06em] text-ink-3">{g}</legend>
                      {!role.locked && (
                        <button type="button" onClick={() => toggleGroup(g, !all)} className="text-xs text-accent hover:underline">{all ? "Clear all" : "Select all"}</button>
                      )}
                    </div>
                    {items.map((p) => {
                      const mine = can(p.id);
                      const on = current.includes(p.id);
                      return (
                        <label key={p.id} className={`flex items-start gap-3 py-2 border-b border-line last:border-b-0 ${role.locked ? "" : "cursor-pointer hover:bg-hover"} -mx-2 px-2`}>
                          <span className="flex-1 min-w-0">
                            <span className="block font-medium">{p.label}</span>
                            <span className="block text-xs text-ink-3">{p.help}</span>
                          </span>
                          <input type="checkbox" className="mt-1" checked={on} disabled={role.locked || (!on && !mine)} title={!on && !mine ? "Your account doesn't have this permission, so you can't grant it" : undefined} onChange={() => toggle(p.id)} aria-label={`${p.label} for ${role.name}`} />
                        </label>
                      );
                    })}
                  </fieldset>
                );
              })}
            </div>
            {!role.locked && (
              <footer className="sticky bottom-0 flex items-center justify-end gap-2 px-4 py-2.5 border-t border-line bg-surface">
                <span className="mr-auto text-xs text-ink-3">{changed ? "Unsaved changes. They apply to every user with this role." : `${current.length} of ${catalogue.length} permissions`}</span>
                <Button size="sm" onClick={() => setDraft(null)} disabled={!changed}>Discard</Button>
                <Button size="sm" variant="primary" onClick={() => save.mutate({ ...role, permissions: current })} disabled={!changed || save.isPending}>Save changes</Button>
              </footer>
            )}
          </section>
        </div>
      )}
      {dialog && (
        <RoleDetailsDialog
          initial={dialog}
          roles={list}
          onClose={() => setDialog(null)}
          onSaved={(id) => { setDialog(null); setDraft(null); setSelectedId(id); setView("edit"); refresh(); }}
        />
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
  // A secret is never sent back, so the field starts empty and only a typed value replaces it.
  const commit = (v: string) => (s.secret ? v !== "" && save.mutate(v) : v !== s.value && save.mutate(v));
  const helpId = `setting-${s.key}-help`;
  return (
    <div className="grid md:grid-cols-[1fr_260px] gap-x-4 gap-y-1 px-3 py-2.5 border-b border-line last:border-b-0">
      <div>
        <div className="font-medium flex items-center gap-1.5 flex-wrap">
          {s.label}
          {s.restart && <Badge tone="warn">restart needed</Badge>}
        </div>
        <p id={helpId} className="text-xs text-ink-2">{s.help}</p>
        <code className="text-xs text-ink-3 font-mono">{s.env}</code>
      </div>
      <div>
        {s.kind === "bool" ? (
          <Toggle label={s.label} checked={value === "true"} onChange={(v) => { if (locked) return; setValue(String(v)); save.mutate(String(v)); }}>{value === "true" ? "On" : "Off"}</Toggle>
        ) : s.kind === "choice" ? (
          <Select label={s.label} className="w-full" value={value} onChange={(v) => { setValue(v); save.mutate(v); }} options={s.options.map((o: string) => ({ value: o, label: o }))} />
        ) : (
          <input aria-label={s.label} aria-describedby={helpId} disabled={locked} autoComplete="off" placeholder={s.secret ? (s.set ? "Saved. Type to replace it" : "Not set") : undefined} type={s.secret ? "password" : s.kind === "string" ? "text" : "number"} min={s.min ?? undefined} max={s.max ?? undefined} value={value} onChange={(e) => setValue(e.target.value)} onBlur={() => commit(value)} onKeyDown={(e) => e.key === "Enter" && commit(value)} className={`${field} disabled:opacity-60`} />
        )}
        <div className="flex items-center gap-2 mt-1 text-xs text-ink-3">
          {locked ? <span><Lock size={10} className="inline -mt-0.5 mr-0.5" aria-hidden />Set by environment variable</span> : s.source === "database" ? (
            <>
              <span>Saved in database</span>
              <button type="button" onClick={() => { setValue(s.secret ? "" : s.default); save.mutate(null); }} className="text-accent hover:underline">{s.secret ? "Remove" : "Reset to default"}</button>
            </>
          ) : <span>Default</span>}
          {save.isSuccess && <span className="text-good">Saved</span>}
        </div>
        {problem && <p role="alert" className="text-crit text-xs mt-1">{problem}</p>}
      </div>
    </div>
  );
}

/** The redirect URL to register with the provider, and a check that the provider is reachable. */
function SsoHelp() {
  const check = useMutation({ mutationFn: () => post("/admin/sso/check") });
  const redirect = `${window.location.origin}/api/v1/auth/sso/callback`;
  return (
    <div className="mx-3 mt-2.5 p-3 rounded-md border border-line bg-sunken">
      <div className="text-xs font-medium text-ink-2">Redirect URL to register with your provider</div>
      <div className="flex items-center gap-2 mt-1">
        <code className="flex-1 min-w-0 truncate font-mono text-xs">{redirect}</code>
        <Button size="sm" onClick={() => navigator.clipboard?.writeText(redirect)}><Copy size={14} />Copy</Button>
        <Button size="sm" onClick={() => check.mutate()} disabled={check.isPending}><Busy active={check.isPending} work="connect">{check.isPending ? "Checking…" : "Check provider"}</Busy></Button>
      </div>
      {check.isSuccess && <p className="mt-2 text-xs text-good">Reached {check.data.issuer}: {check.data.keys} signing {check.data.keys === 1 ? "key" : "keys"} published.</p>}
      {check.isError && <p role="alert" className="mt-2 text-xs text-crit">{(check.error as Error).message}</p>}
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
    <div className="flex flex-col md:flex-row">
      <nav className="md:w-[190px] shrink-0 border-b md:border-b-0 md:border-r border-line p-2 max-md:flex max-md:overflow-x-auto max-md:gap-1">
        {groups.map((g) => (
          <button type="button" key={g} onClick={() => setGroup(g)} className={`block md:w-full shrink-0 whitespace-nowrap text-left px-2.5 py-1.5 rounded-md ${group === g ? "bg-accent-wash font-medium" : "text-ink-2 hover:bg-hover"}`}>{g}</button>
        ))}
      </nav>
      <div className="flex-1 min-w-0">
        <p className="text-ink-3 text-xs px-3 pt-2.5">Every setting can also be set with its environment variable, which then takes priority over the value here.</p>
        {group === "Single sign-on" && <SsoHelp />}
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
        <Button variant="primary" onClick={() => { setProblem(""); setEditing({ ...blankStorage }); }}><Plus size={14} />Add storage</Button>
      </div>
      {!data.encryption_key_set && <p className="text-warn text-xs px-3 pb-2">SS_SECRET_KEY isn't set, so storage passwords are kept unencrypted in MariaDB.</p>}
      {problem && !editing && <p className="text-crit px-3 pb-2">{problem}</p>}
      <div className="px-3 pb-3 space-y-2">
        {data.storages.map((s: any) => {
          const r = results[s.id];
          return (
            <div key={s.id} className={`border rounded-md p-3 ${s.active ? "border-accent-soft bg-accent-wash" : "border-line"}`}>
              <div className="flex items-center gap-2 flex-wrap">
                <span className="font-semibold">{s.name}</span>
                <Badge>{s.type === "local" ? "Local file" : "ClickHouse"}</Badge>
                {s.built_in && <Badge>Built in</Badge>}
                {s.active && <Badge tone="info">In use</Badge>}
                <div className="flex-1" />
                <Button onClick={() => test.mutate(s.id)} disabled={test.isPending}><Busy active={test.isPending && test.variables === s.id} work="connect">{test.isPending && test.variables === s.id ? "Testing…" : "Test connection"}</Busy></Button>
                {!s.active && <Button variant="primary" onClick={() => activate.mutate(s.id)}>Use this storage</Button>}
                {!s.built_in && <Button onClick={() => { setProblem(""); setEditing({ ...s, password: "" }); }}><Pencil size={14} />Edit</Button>}
                {!s.built_in && !s.active && <Button onClick={() => confirm(`Remove ${s.name}?`) && remove.mutate(s.id)}><Trash2 size={14} /></Button>}
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
                <legend className="text-xs font-medium text-ink-2 mb-1">Backend</legend>
                <div className="grid grid-cols-2 gap-2">
                  {BACKENDS.map((b) => (
                    <label key={b.value} className={`rounded-md border p-2.5 cursor-pointer has-focus-visible:outline-2 has-focus-visible:outline-accent ${editing.type === b.value ? "border-accent bg-accent-wash" : "border-control hover:bg-hover"}`}>
                      <input type="radio" name="backend" value={b.value} checked={editing.type === b.value} onChange={() => setEditing({ ...editing, type: b.value })} className="sr-only" />
                      <span className="block font-medium text-[13px]">{b.label}</span>
                      <span className="block text-xs text-ink-3 mt-0.5 leading-snug">{b.text}</span>
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
            {problem && <p role="alert" className="text-crit">{problem}</p>}
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
  if (rows.length === 0) return <p className="text-ink-2 p-3">No HEP received yet. Point your SIP servers at port 9060 of this server; each sender appears here by its address.</p>;
  return (
    <div>
      <p className="text-ink-2 text-xs px-3 pt-3 pb-2 max-w-[720px]">Every server that sends HEP, by address. Give each one an alias, such as Asterisk-1 or OpenSIPS-edge; the source picker at the top shows it. The alias names new traffic, already stored calls keep the name they had.</p>
      <div className="overflow-x-auto">
        <table className="data-table">
          <thead><tr><th>Address</th><th>Alias</th><th>Capture ID</th><th>HEP</th><th className="num">Packets</th><th className="num">Clock offset</th><th>Last seen</th><th>Status</th></tr></thead>
          <tbody>{rows.map((r) => (
            <tr key={r.ip}>
              <td className="font-mono text-xs">{r.ip}</td>
              <td>{can("settings_manage") ? <input aria-label={`Alias for ${r.ip}`} defaultValue={r.alias} placeholder="Add an alias" onBlur={(e) => { const alias = e.target.value.trim(); if (alias !== r.alias) rename.mutate({ ip: r.ip, alias }); }} onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()} className={`${fieldClass} h-7 w-[200px] px-2`} /> : (r.alias || <span className="text-ink-3">–</span>)}</td>
              <td className="tnum">{r.capture_id}</td>
              <td>v{r.hep_version}</td>
              <td className="num">{r.packets.toLocaleString()}</td>
              <td className="num">{r.offset_ms} ms</td>
              <td>{r.last_seen ? fmtDateTime(r.last_seen, tz).slice(5) : "before restart"}</td>
              <td><Status tone={r.status === "active" ? "good" : r.status === "clock skew" ? "warn" : "neutral"}>{r.status}</Status></td>
            </tr>
          ))}</tbody>
        </table>
      </div>
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
    <div className="overflow-auto max-h-[calc(100vh-166px)]">
      <table className="data-table">
        <thead><tr><th>Time</th><th>Actor</th><th>Source</th><th>Action</th><th>Object</th><th>Outcome</th><th>Details</th></tr></thead>
        <tbody>{data.map((r: any) => <tr key={r.id}><td>{fmtDateTime(r.ts, tz).slice(5)}</td><td>{r.actor}</td><td className="font-mono text-xs">{r.src_ip}</td><td className="font-mono text-xs">{r.action}</td><td className="text-ink-2 font-mono text-xs">{r.object}</td><td><Status tone={tone(r.outcome)}>{r.outcome}</Status></td><td className="text-ink-2 text-xs max-w-[320px] truncate" title={r.details}>{r.details}</td></tr>)}</tbody>
      </table>
    </div>
  );
}

function System({ data }: { data: any }) {
  const ch = data.clickhouse;
  return (
    <div className="p-3 space-y-3">
      <div className="grid grid-cols-2 md:grid-cols-3 gap-x-6 gap-y-2.5">
        <KV label="Version">{data.version}</KV>
        <KV label="MariaDB">{data.mariadb}</KV>
        <KV label="HEP UDP">{data.listeners.hep_udp || "off"}</KV>
        <KV label="HEP TCP">{data.listeners.hep_tcp || "off"}</KV>
        <KV label="HEP auth key">{data.auth_key ? "required" : "not set"}</KV>
        <KV label="Storage engine">{ch ? `ClickHouse ${ch.version} · ${ch.database}` : "starting"}</KV>
      </div>
      {ch && ch.tables.length > 0 && (
        <div className="border border-line rounded-md overflow-x-auto">
          <table className="data-table">
            <thead><tr><th>Table</th><th className="num">Rows</th><th className="num">Size</th><th className="num">Parts</th></tr></thead>
            <tbody>{ch.tables.map((t: any) => <tr key={t.table}><td className="font-mono text-xs">{t.table}</td><td className="num">{Number(t.rows).toLocaleString()}</td><td className="num">{(Number(t.bytes) / 1048576).toFixed(1)} MB</td><td className="num">{t.parts}</td></tr>)}</tbody>
          </table>
        </div>
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
        <div className="p-3 space-y-3">
          <Toggle checked={crosshair} onChange={(v) => savePrefs({ crosshair: v })}>Link charts on hover: pointing at one chart shows the crosshair on every chart of the dashboard</Toggle>
          <div className="grid grid-cols-3 gap-2">
            <Row label="Theme"><Select className="w-full" value={prefs.theme ?? "system"} onChange={(v) => savePrefs({ theme: v as "light" | "dark" | "system" })} options={[{ value: "system", label: "Follow system" }, { value: "light", label: "Light" }, { value: "dark", label: "Dark" }]} /></Row>
            <Row label="Time zone"><Select className="w-full" value={tz} onChange={(v) => savePrefs({ tz: v })} options={zones.map((z) => ({ value: z, label: z }))} /></Row>
            <Row label="Table rows"><Select className="w-full" value={density} onChange={(v) => savePrefs({ density: v as "compact" | "comfortable" })} options={[{ value: "comfortable", label: "Comfortable" }, { value: "compact", label: "Compact" }]} /></Row>
          </div>
        </div>
      </Card>
      <Card title="Change password" className="mt-3">
        <form className="p-3 space-y-3" onSubmit={(e) => { e.preventDefault(); change.mutate(); }}>
          <Row label="Current password"><input type="password" autoComplete="current-password" value={current} onChange={(e) => setCurrent(e.target.value)} className={field} /></Row>
          <Row label={`New password, at least ${settings.password_min_length} characters`}><input type="password" autoComplete="new-password" value={next} onChange={(e) => setNext(e.target.value)} className={field} /></Row>
          {note && <p className="text-xs text-ink-2">{note}</p>}
          <Button type="submit" variant="primary">Change password</Button>
        </form>
      </Card>
    </div>
  );
}
