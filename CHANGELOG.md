# Changelog

Images are published as `ntuxa76/sip-source:<version>`.

## 0.10

- **Anomalies.** sip-source learns what is normal for each hour from your own traffic and checks
  the last 30 minutes every five minutes, overall, per trunk and per source: call volume, answer
  rate, 5xx failures, post-dial delay, voice quality and failed registrations. Anything out of line
  opens an anomaly with its normal band, a 24-hour chart and a link to the calls behind it. Open
  anomalies show on Home and beside the menu item, and close by themselves when traffic recovers.
  Turn it off, or change how many calls are needed, under **Settings → Anomalies**.
- **Call path.** A new **Path** tab on every call shows each device the INVITE crossed, in order,
  with its software, how long it held the INVITE, what it answered, which device started a
  failure, where a device changed the answer it got (for example 503 passed on as 480), and
  proxies seen in Via headers that were not captured. A timing chart shows each hop's wait.
- **Trunks without configuration.** When the Topology settings name no carriers, a device that
  takes calls but never registers or takes registrations is treated as a trunk and named after its
  software and address, so the trunk dashboard, ASR by trunk and carrier rejects fill in on their
  own. Calls stored before the upgrade keep their old values.
- **Size limit.** Optional **Settings → Retention → Size limit (GB)**: when captured data grows
  past it, the oldest day is deleted, one day at a time. Off by default; retention by days still
  applies.
- **Live capture** keeps working under heavy traffic: keep-alives and registrations are hidden
  unless asked for, each dialog keeps its latest 300 messages, the stream reconnects by itself
  after a dropped connection, and reverse proxies no longer hold it back.
- The message ladder is coloured by message type, with a legend, the software of each device, a
  bar showing setup, ringing and talk, and arrows that draw in.
- Refresh buttons on dashboards, Explore and search. The logo goes to Home and reloads it.
- Gentle motion across the interface: pages and cards rise in, buttons respond, live states pulse.
  It is switched off when the system asks for reduced motion.

## 0.9.1

- Loading indicators are animated orbs whose motion shows the kind of work: loading, searching
  calls, running SQL, following live traffic, reaching another system, or waiting for storage to
  start. Each comes with a short label, and nothing appears for loads under 200 ms.

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
