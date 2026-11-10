/** Which pages an account may open, and where it lands after signing in. */

type Can = (permission: string) => boolean;

/** Admin tabs and the permission each needs, in the order they appear. */
export const ADMIN_SECTIONS: { id: string; label: string; perm: string }[] = [
  { id: "users", label: "Users", perm: "users_manage" },
  { id: "roles", label: "Roles & permissions", perm: "roles_manage" },
  { id: "settings", label: "Settings", perm: "settings_manage" },
  { id: "storage", label: "Storage", perm: "storage_manage" },
  { id: "sources", label: "Sources", perm: "system_view" },
  { id: "system", label: "System health", perm: "system_view" },
  { id: "audit", label: "Audit log", perm: "audit_read" },
];

const DATA_PERMISSIONS = ["dashboards_view", "explore", "live_view", "search_all", "subscribers_view", "sql_workbench"];

/** True for accounts that look at captured traffic, as opposed to administration only. */
export function seesTraffic(can: Can): boolean {
  return DATA_PERMISSIONS.some(can);
}

/** The first admin tab the account can open. Someone who can only read the audit log starts there. */
export function adminHome(can: Can): string | null {
  const manage = ADMIN_SECTIONS.slice(0, 4).find((s) => can(s.perm));
  if (manage) return `/admin/${manage.id}`;
  if (can("audit_read")) return "/admin/audit";
  const rest = ADMIN_SECTIONS.find((s) => can(s.perm));
  return rest ? `/admin/${rest.id}` : null;
}

export function homePath(can: Can, sqlEnabled: boolean): string {
  if (can("dashboards_view")) return "/";
  if (can("search_all")) return "/search";
  if (can("subscribers_view")) return "/subscribers";
  if (can("live_view")) return "/live";
  if (can("explore")) return "/explore";
  if (can("sql_workbench") && sqlEnabled) return "/sql";
  return adminHome(can) ?? "/preferences";
}

export function pathAllowed(path: string, can: Can, sqlEnabled: boolean): boolean {
  const [, first = "", second = ""] = path.split("/");
  switch (first) {
    case "":
    case "dashboards":
      return can("dashboards_view");
    case "explore":
      return can("explore");
    case "live":
      return can("live_view");
    case "search":
      return can("search_all");
    case "subscribers":
      return can("subscribers_view");
    case "calls":
      return can("search_all") || can("subscribers_view");
    case "sql":
      return can("sql_workbench") && sqlEnabled;
    case "admin":
      return ADMIN_SECTIONS.some((s) => s.id === second && can(s.perm));
    default:
      return true;
  }
}
