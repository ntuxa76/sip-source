# Changelog

Images are published as `ntuxa76/sip-source:<version>`.

## 0.9

- The whole database schema, MariaDB and ClickHouse, is one file: [`schema.sql`](schema.sql).
  New databases get the current schema in one step; older ones get the upgrades they are missing.
- The service binary and crate are now named `sip-source`.
- Built-in dashboards moved to `crates/sip-source/dashboards`.
- Documentation reorganized under [`docs/`](docs): capture setup, single sign-on, security and the
  environment reference.

## 0.8

- Dashboard JSON moved from the header to **Settings → JSON** while editing: copy, download or
  replace it.
- Pointer cursor on every clickable control; disabled buttons keep their tooltips.
- "ClickHouse insert" metric and panels renamed to storage write time, since the built-in Local
  file storage is used by default.
- Collapsible sidebar, remembered per user, with its own colour tone.

## 0.7

- New default roles (Administrator, Engineer, Analyst, Support, Viewer, Auditor) and a rebuilt
  roles page with a comparison view. Nobody can grant more access than they hold.
- Every account opens on a page it may use; the time range and source pickers only appear where
  they apply. Theme is saved per user.
- Single sign-on with OpenID Connect.
- Environment variables renamed from `VP_*` to `SS_*`. The old names still work and print a
  warning. The session cookie was renamed, so everyone signs in once after upgrading.
- Security: user-written SQL can no longer reach other systems through ClickHouse table functions,
  CSV exports neutralize formulas, captured text in chart tooltips is escaped, failed sign-ins are
  limited per address.

## 0.6

- New interface: IBM Plex type, flat panels with hairline borders, tables, charts and dialogs
  redesigned for density and legibility, in light and dark themes.

## 0.5 and earlier

See the git history.
