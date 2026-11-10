# voip-panel — Requirements Specification

| | |
|---|---|
| **Version** | 0.1 — draft for review |
| **Date** | 2026-11-08 |
| **Owner** | Noor (Systems & Infrastructure, Earthlink) — to confirm |
| **Status** | First full draft. Incorporates the decisions from the 2026-11-08 discussion (§12): ClickHouse for captured data, Rust for the core, fix HOMER 11's charts/time handling and query difficulty, built-in essential dashboards, A-to-Z call tracking, full HOMER feature coverage. Storage sub-decisions not yet confirmed are written as defaults and listed in §14. |

---

## 1. Purpose and scope

### 1.1 Purpose
voip-panel ("the Panel") is a from-scratch SIP/VoIP capture, troubleshooting and monitoring platform that replaces HOMER 11. It receives HEP traffic from OpenSIPS, Asterisk, rtpengine and any HEP-capable agent, stores it in ClickHouse, assembles every call **from the first INVITE to the last BYE across all legs and hops**, and gives operators fast search, a precise call-flow view, media-quality data, built-in dashboards and alerting — without anyone having to write SQL.

It keeps everything HOMER does well (HEP ingest, ladder diagram, PCAP export, QoS, aliases, share links, live capture) and removes what HOMER 11 does badly (§1.5).

### 1.2 In scope
- HEP v1/v2/v3 ingest over UDP, TCP, TLS and HTTP(S); agent authentication; compressed payloads
- SIP (incl. SIP over WS/WSS), SDP, RTCP, RTCP-XR (HEP and `vq-rtcpxr` SIP PUBLISH), rtpengine/agent QoS reports, HEP logs
- Call correlation across proxies and B2BUAs; first-class **calls**, **registrations** and **media-quality** tables
- Universal search (paste a number, Call-ID, IP or URI), structured search, saved searches
- Call view: summary, multi-hop ladder diagram, messages, SDP/media, QoS charts, logs, timeline, automatic diagnostics
- Subscriber view (registration state, devices, calls, quality for one number)
- Metric catalogue (ASR, NER, PDD, ACD, SER, SEER, MOS, …), correct time handling, high-quality charts
- Built-in dashboards for the essentials, plus user dashboards
- SQL workbench for engineers (schema browser, autocomplete, query library, safe execution)
- Live capture view (in-browser, sngrep-like)
- PCAP / text / JSON export, PCAP import, expiring share links
- Alerting with email/webhook notifications; Prometheus metrics for Zabbix/Grafana/Alertmanager
- Local, LDAP/AD, RADIUS and OIDC authentication; MFA; API tokens; role-based access
- Audit log stored locally and forwarded to IBM QRadar
- Retention and hot/cold storage managed from the UI
- Ready-made integration configs for OpenSIPS, Asterisk and rtpengine
- REST API with an OpenAPI contract; CLI; RPM, container image, Docker Compose, Podman Quadlet and Helm deployment

### 1.3 Out of scope
- Being a SIP proxy, SBC, B2BUA or media relay (the Panel only observes)
- Decrypting TLS/SRTP on the wire — capture comes from the SIP/media elements themselves via HEP, after decryption
- Billing/rating (call records are operational, not billing-grade CDRs)
- Lawful interception
- Storage backends other than ClickHouse (deliberate — AD-1)
- Capture agents for Solaris, BSD, macOS and Windows (Linux agent only, phase 2)
- Recording and playback of RTP media in phases 1–2 (phase 3 candidate, §14 Q5)

### 1.4 Phases at a glance

| Phase | Headline scope |
|---|---|
| **1 — MVP** | HEP ingest, SIP/SDP/RTCP/RTCP-XR/logs, correlation, calls/registrations/QoS tables, search, call view, subscriber view, diagnostics, metric catalogue, charts, built-in and custom dashboards, SQL workbench, live view, export/import/share, alerting, auth (local, LDAP, RADIUS, OIDC), audit + QRadar, retention UI, integration kit, single-node and replicated ClickHouse |
| **2** | Rust capture agent, data scopes (restricted views), Arabic UI, SIPREC, SIP-I/ISUP decode, Lua correlation scripts, syslog/OTLP logs, Kafka input/output, HEP forwarding, anomaly alerts, MCP server, HTML call report, sharded ClickHouse, CDR/CEL ingest |
| **3** | RTP media capture and playback, WebRTC JSON signalling, Diameter/DNS decode, natural-language query assistant, external tenants |

### 1.5 HOMER 11 problems and how voip-panel addresses them

| # | HOMER 11 problem | Evidence | voip-panel answer |
|---|---|---|---|
| H-1 | Charts look poor; inconsistent time buckets, gaps, time-zone confusion | Operator experience | One chart system and one time engine (FR-CH, AD-7) |
| H-2 | SQL is hard; data is hard to find | Operator experience; search depends on hand-built SQL and per-protocol forms | Universal search box over a lookup index (FR-SR, AD-5); metric catalogue so charts need no SQL (AD-6); SQL workbench with documented views, autocomplete and a query library for those who want SQL (FR-SQ) |
| H-3 | Out-of-memory on long searches and compaction; a failed flush invalidates the embedded DuckDB and the process exits | `docs/OOM.md`; commits for #939, #945 | ClickHouse server with per-query memory/time limits; ingest decoupled from queries (AD-10) |
| H-4 | Catalog corruption (duplicate snapshots, SQLite page damage) with repair/rebuild tooling | `docs/OOM.md`, `docs/CATALOG.md`; issues #900, #1048 | No catalog files; ClickHouse replicated MergeTree (AD-1) |
| H-5 | One writer per catalog (exclusive lock) | Commit "exclusive writer lock to prevent concurrent-writer corruption" | Any number of stateless collectors (AD-4) |
| H-6 | Three separate 30 s timeouts must all be raised for long searches | `docs/TROUBLESHOOTING.md` | One timeout per query class, shown with progress and cancel (FR-SR-14, NFR-02) |
| H-7 | Several overlapping retention settings; the Mappings "retention" field does nothing | `docs/RETENTION.md` | One retention value per data type, set in the UI, applied as ClickHouse TTL (AD-11, FR-AD-06) |
| H-8 | Call info (setup delay, duration, …) recomputed from raw messages on every view; no call table | `callinfo_compute.go` | Calls assembled once into a `calls` table (AD-3, FR-CR) |
| H-9 | No registration state ("is this phone registered now?") | — | `registration_state` + subscriber view (FR-RG, FR-SV) |
| H-10 | Raw SQL exposed through the API; at least ten security advisories (unauthenticated routes with empty JWT secret, shipped JWT placeholder, default admin password, SQL passthrough, file read through DuckDB scanners, self-assigned admin role) | `docs/SECURITY.md` | No raw SQL in the API except the workbench, which runs under ClickHouse read-only RBAC (AD-8); no default credentials; no JWT secrets — server-side sessions (AD-13) |
| H-11 | Games shipped inside the product (Doom, chess, Tetris variants, Packet Defender) | `src/ui/src/dashboard/widgets/` | Not included |
| H-12 | Users, dashboards and settings in an embedded DuckDB/SQLite file | `coordinator.settings_db_path` | PostgreSQL (AD-1) |
| H-13 | B2BUA correlation needs Lua scripts | `docs/LUA_CORRELATION.md` | Declarative correlation rules + link table (FR-CR); Lua optional in phase 2 |
| H-14 | Grafana access through an extra FlightSQL listener | `docs/GRAFANA_INTEGRATION.md` | Grafana's ClickHouse plugin on documented read-only views (FR-INT-02) |

---

## 2. Glossary

| Term | Meaning |
|---|---|
| **HEP** | Homer Encapsulation Protocol (v1/v2/v3). Wraps a captured packet or event with metadata (addresses, ports, timestamp, protocol type, agent id). |
| **Agent** | Anything that sends HEP: OpenSIPS (`proto_hep`/`siptrace`), Asterisk (`res_hep`), rtpengine, heplify, captagent, the voip-panel agent (P2). Identified by HEP agent id + name. |
| **Capture point** | Where a message was observed: (agent, interface/direction). The same SIP message is usually captured at more than one point. |
| **Collector** | voip-panel process role that receives, decodes, enriches and writes HEP data. |
| **Dialog** | One SIP dialog, identified by its Call-ID (plus tags). |
| **Leg** | One dialog that is part of a call. A call through a B2BUA has at least two legs (A-leg, B-leg). |
| **Call** | All legs that belong to one end-to-end call, correlated into one record with a stable `call_uid`. |
| **Link** | A recorded relation between two Call-IDs (e.g. B-leg carries the A-leg Call-ID in `X-CID`). |
| **Finalizer** | Worker job that assembles dialogs and links into call records and computes derived fields and diagnostics. |
| **AoR** | Address of record (`user@domain`) being registered. |
| **Topology group** | A named set of IPs/CIDRs/ports with a role: `subscriber`, `proxy`, `pbx`, `media`, `carrier`, `other`. Drives direction, trunk and dashboard dimensions. |
| **Alias** | Display name for an IP/CIDR/port (e.g. `opensips-1`, `Carrier-X SBC`). |
| **Direction** | Call classification from topology: `onnet` (subscriber→subscriber), `outbound` (subscriber→carrier), `inbound` (carrier→subscriber), `transit`, `unknown`. |
| **QoS report** | One RTCP, RTCP-XR or agent media report for one RTP stream. |
| **Finding** | An automatic diagnostic result attached to a call (e.g. `ACK_MISSING`, `ONE_WAY_AUDIO`). |
| **Metric** | A catalogue entry with formula, unit and allowed dimensions (e.g. ASR by trunk). |
| **Rollup** | Pre-aggregated per-minute or per-hour KPI table used for long time ranges. |
| **Settle window** | Delay after which a minute is considered complete (late messages accounted for); default 5 min. |
| **Lookup index** | ClickHouse table mapping every searchable identifier (numbers, Call-IDs, IPs, AoRs, correlation ids) to calls. |
| **Share link** | Signed, expiring, read-only URL to one call. |
| **PDD** | Post-dial delay: initial INVITE → first 180/183 (or final response if none). Same as RFC 6076 SRD. |
| **ASR / NER / ACD** | Answer-seizure ratio, network effectiveness ratio, average call duration (definitions in §5.12). |
| **MOS** | Mean opinion score; taken from reports when present, otherwise estimated (E-model) and labelled "estimated". |

---

## 3. Architecture decisions

| ID | Decision | Rationale / consequence |
|---|---|---|
| AD-1 | **ClickHouse is the only store for captured data; PostgreSQL holds application state** (users, roles, dashboards, saved searches, topology, rules, alerts, audit, jobs). No pluggable storage backends. | Decision 2026-11-08. One well-tuned store instead of HOMER's multi-database history and DuckLake fragility (H-3, H-4, H-12). |
| AD-2 | **Rust core**: one binary `vpd` with roles `collector`, `api`, `worker`, `migrate`, or `all` (all-in-one). CLI `vpctl`. | Decision 2026-11-08. Predictable latency and memory, no GC pauses on the ingest path, one artefact to deploy. |
| AD-3 | **Calls, registrations and media-quality summaries are first-class tables**, built once. Raw messages are kept for detail. | Search and dashboards read small tables; the call view never recomputes call info (H-8). |
| AD-4 | **Correlation is split in two**: collectors are stateless per message (they extract ids and write *link* rows); the **finalizer** worker assembles links and dialogs into calls. | Any number of collectors, any agent-to-collector distribution, no shared in-memory state (H-5). |
| AD-5 | **Universal lookup index**: every identifier a person might search by (normalized number, number suffix, Call-ID, X-CID, IP, AoR, user agent) is indexed to `call_uid` / AoR. | One search box that finds anything (H-2). |
| AD-6 | **Metric catalogue**: each KPI is defined once (name, formula, unit, dimensions, source tables per resolution). Dashboards, alerts, the API and the Prometheus exporter all use it. Built-in panels contain no hand-written SQL. | Consistent numbers everywhere; users build charts by picking metrics, not writing SQL (H-1, H-2). |
| AD-7 | **One time engine**: timestamps stored in UTC with microsecond precision; buckets computed server-side in the viewer's time zone with "nice" aligned intervals; zero-fill and partial-bucket rules are global. | Fixes chart timing (H-1). |
| AD-8 | **Query safety by ClickHouse RBAC**, not by parsing SQL. The API builds parameterized SQL only from typed filters. The SQL workbench runs as a read-only ClickHouse user per role with settings profiles, quotas and row policies. | Removes HOMER's SQL-passthrough vulnerability class (H-10). |
| AD-9 | **Frontend**: React + TypeScript single-page app built with Vite, Apache ECharts for charts, a custom ladder-diagram renderer; served as static files embedded in `vpd`. No Node.js in production. | Mature chart engine with linked charts, data zoom, heatmaps and canvas rendering; same origin, no CORS; same pattern as the DNS panel. |
| AD-10 | **Ingest never blocks on storage**: bounded queues, batched inserts, local disk spool when ClickHouse is unavailable; UDP overload produces counted drops, never a stall. | Capture survives ClickHouse maintenance; queries cannot starve ingest (H-3). |
| AD-11 | **Retention = one value per data type**, edited in the UI, applied by the Panel as ClickHouse TTL; optional hot→cold move to S3/MinIO via ClickHouse storage policy. | Fixes H-7. |
| AD-12 | **Integration-first**: phase 1 relies on OpenSIPS, Asterisk, rtpengine and existing HEP agents with configurations shipped and generated by the Panel. Own agent in phase 2. | Easy roll-out; no new software on voice nodes. |
| AD-13 | **Authentication**: local (Argon2id + TOTP), LDAP/AD, RADIUS, OIDC; server-side sessions; hashed API tokens. No JWTs, no shipped secrets, no default passwords. | Fixes H-10. |
| AD-14 | **Audit** has two sinks: append-only PostgreSQL table (reviewable in the UI) and QRadar via LEEF over TLS syslog. | Same model as the DNS panel. |
| AD-15 | **Jobs, schedules and leader election live in PostgreSQL** (advisory locks + job table). No extra broker in phase 1; Kafka is an optional input/output in phase 2. | Simplest HA story; add a broker only if measured scale demands it. |
| AD-16 | **ClickHouse topology modes**: single node, replicated (2 replicas + 3 Keeper nodes) and sharded (P2). One schema, generated per mode by `vpd migrate`. | Start small (10k users) without redesign for 2M users. |
| AD-17 | **Display names resolve at query time; classification resolves at ingest.** Aliases are a ClickHouse dictionary (renaming a host relabels history). Topology group, direction and trunk are stamped at ingest (KPIs stay stable). | Clear, documented semantics; re-classification affects new data only. |

### 3.1 Component overview

```
                 HEP UDP/TCP/TLS/HTTP            VQ-RTCPXR (SIP PUBLISH)
 OpenSIPS ─┐                                            │
 Asterisk ─┼──────────────►┌───────────────────────┐◄───┘
 rtpengine ┤               │  vpd --role collector │  (N instances, stateless)
 heplify  ─┘               │  decode → parse →     │
                           │  enrich → filter →    │──► local disk spool (if CH down)
                           │  batch insert         │
                           └──────────┬────────────┘
                                      │ live feed (internal WebSocket)
                                      ▼
┌────────────────────────────────────────────────────────────────────┐
│                         ClickHouse (captured data)                 │
│ sip_messages · reg_messages · call_links · dialogs · calls ·       │
│ lookup_index · registration_events · registration_state ·         │
│ qos_reports · stream_qos · logs · kpi_* rollups · counters         │
└───────────▲───────────────────────────────▲────────────────────────┘
            │ parameterized SQL             │ finalizer, rollups, TTL, alerts
┌───────────┴──────────────┐      ┌─────────┴─────────────┐
│ vpd --role api           │      │ vpd --role worker     │
│ REST /api/v1 + SPA + WS  │      │ (leader-elected jobs) │
└───────────┬──────────────┘      └─────────┬─────────────┘
            │                               │
            └────────────► PostgreSQL ◄─────┘   (app state, audit, jobs)
                                │
                                └──► QRadar (LEEF/TLS syslog), SMTP, webhooks
 Browser ◄── HTTPS ── HAProxy/ingress ── api        Prometheus/Zabbix ◄── /metrics
```

### 3.2 Proposed implementation stack (non-normative — confirm at tech selection)

**Core (Rust, current stable, edition 2024)**
- Runtime and HTTP: `tokio`, `axum` + `tower-http`; OpenAPI 3.1 generated from code with `utoipa`; Scalar or Swagger UI at `/api/docs`.
- Storage clients: official `clickhouse` crate (RowBinary over HTTP, LZ4) for inserts and queries; `sqlx` (PostgreSQL, compile-time checked queries).
- Networking: `socket2` (SO_REUSEPORT, large buffers), batched `recvmmsg` for UDP, `tokio-rustls`/`rustls` for TLS, `bytes`, `memchr`.
- Parsers (in-house, zero-copy, fuzzed): HEP v1/v2/v3, SIP, SDP, RTCP/RTCP-XR, `vq-rtcpxr` bodies; `flate2` for compressed HEP payloads.
- PCAP: `pcap-file` (write), `pcap-parser` + `etherparse` (import, TCP reassembly in-house).
- Enrichment: `maxminddb` (GeoLite2 or DB-IP Lite), in-house number normalizer.
- Auth: `argon2`, `totp-rs`, `openidconnect`, `ldap3`, a RADIUS client crate (to select; RFC 2865 PAP and CHAP).
- Notifications: `lettre` (SMTP), `reqwest` (webhooks).
- Observability: `tracing`, `tracing-opentelemetry`, `opentelemetry-otlp`, `metrics` + `metrics-exporter-prometheus`.
- Config and CLI: `figment` (TOML + `VP_*` environment variables), `clap`.
- Scripting (P2): `mlua` (Lua 5.4, sandboxed).
- UI embedding: `rust-embed`.
- Quality: `cargo-nextest`, `proptest`, `cargo-fuzz`, `criterion`, `cargo-deny`, `cargo-audit`, `clippy -D warnings`; `unsafe` forbidden outside audited parser/IO modules.

**Frontend**
- React + TypeScript + Vite; TanStack Router, Query, Table and Virtual.
- Tailwind CSS + shadcn/ui (Radix primitives), CSS logical properties (ready for Arabic RTL in P2).
- **Apache ECharts** for all charts (canvas renderer, `echarts.connect` for linked tooltips/crosshair, dataZoom, heatmap, custom series).
- Custom ladder-diagram component (SVG for ≤ 2,000 messages, canvas above, virtualized).
- MapLibre GL JS for geo maps (self-hosted tiles optional).
- CodeMirror 6 with ClickHouse SQL dialect for the workbench.
- `react-grid-layout` for dashboard layout; `react-hook-form` + `zod`; `date-fns` with `@date-fns/tz` for time zones; i18next.
- API client generated from the OpenAPI document (`openapi-typescript` + `openapi-fetch`).
- Tests: Vitest, Playwright (end-to-end and visual regression for charts and ladder).

**Platform**
- ClickHouse current LTS (25.8 LTS or newer), ClickHouse Keeper for replicated mode; storage policy with local NVMe (hot) and S3/MinIO (cold).
- PostgreSQL 16+.
- Packaging: static `vpd`/`vpctl` binaries (x86_64, aarch64), RPM (Rocky/RHEL 9 and 10), distroless container image, Docker Compose (all-in-one), Podman Quadlet units, Helm chart; cosign-signed images, SBOM, Trivy/Grype in CI.

---

## 4. Actors and roles

### 4.1 Roles

| Role | Can |
|---|---|
| **Admin** | Everything: users and auth sources, agents and agent keys, topology/aliases, correlation and normalization rules, ingest filters, retention and storage, notification channels, system settings, all data and exports. |
| **Engineer** | All search and call views with full payloads; PCAP/text export; PCAP import; share links; SQL workbench; create/edit dashboards, saved searches, alert rules; edit topology/aliases and correlation rules (audited). |
| **NOC Operator** | Dashboards (view, clone to personal), search, call view, subscriber view, live view, acknowledge/silence alerts, PCAP export of single calls. No SQL workbench, no configuration changes. |
| **Support** (helpdesk) | Subscriber view and call view **for a number they enter** (no free browsing of all traffic); SIP payload visible; other party's number masked to the last 4 digits (configurable); no export, no SQL, no live view. Designed for FTTH customer support. |
| **Auditor** | Read-only: audit log, settings, users, retention; no call content. |
| **Service account** | Non-interactive; API tokens only; role assigned like a user (e.g. read-only API for an external portal or Grafana). |

A user holds one platform role. Custom roles are composed from the permission list in FR-AZ-02 (Admin only).

### 4.2 Data scopes (P2)
A scope restricts which traffic a user or token can see: by capture agent, topology group, trunk/carrier, number prefix or domain. Scopes are enforced in the API and, for the SQL workbench and Grafana, as ClickHouse row policies generated by the Panel. Phase 1: all roles see all traffic (subject to the Support role's number-entry rule).

### 4.3 Account lifecycle

| Account type | Auto-disable after inactivity | Notes |
|---|---|---|
| Local user | Yes — 90 days without interactive login (configurable) | Reactivation by Admin; audited |
| LDAP / RADIUS / OIDC user | No — governed by the identity source; disabled when the source rejects login | Group → role mapping re-evaluated at every login |
| Break-glass local Admin | No | At least one must exist; MFA mandatory |
| Service account | No — token expiry governs | |

---

## 5. Functional requirements

Priority tags: untagged = phase 1; **P2** / **P3** as marked.

### 5.1 Capture and ingest
- **FR-IN-01** Listeners, each enabled/disabled and bound independently (IPv4 and IPv6): HEP over UDP (default `:9060`), TCP (`:9060`), TLS (`:9061`, optional mutual TLS), and HTTP(S) batch `POST /ingest/hep` (`:9080`) for agents that cannot use UDP/TCP.
- **FR-IN-02** HEP v1, v2 and v3. HEPv3 chunks per §8.1; unknown chunks are skipped without rejecting the packet; malformed packets are counted by reason (`bad_magic`, `bad_length`, `missing_chunk`, …) and a sample of the last 100 is kept for the Admin → Agents view.
- **FR-IN-03** Agent registry: agents are identified by HEP agent id (`0x000c`) and optional agent name (`0x0013`). Each agent has an optional authentication key (`0x000e`, stored hashed, rotation with an overlap period) and allowed source CIDRs. Policy for unknown agents (platform setting): `accept` (auto-register, flagged *new*), `quarantine` (stored, hidden from search until an Admin approves) or `reject`.
- **FR-IN-04** Compressed payloads (`0x0010`, gzip/zlib) are decompressed with a size cap (default 256 KB decompressed); over-cap packets are counted and dropped.
- **FR-IN-05** UDP performance: one SO_REUSEPORT socket per worker thread, batched `recvmmsg`, configurable receive buffer (default 64 MB); the receive loop never waits on downstream stages. TCP/TLS: framing by HEP length, keep-alive, max connections per source, idle timeout.
- **FR-IN-06** Pipeline: `receive → decode → parse → enrich → filter → route → batch` with a bounded queue between stages. On overload, UDP packets are dropped at the first queue with `dropped_total{reason="overload"}`; TCP applies back-pressure. Queue depth per stage is exported as a metric.
- **FR-IN-07** Batch writer: one batch per destination table, flushed at 50,000 rows or 1,000 ms (whichever first; both configurable); parallel inserts per table; retries with exponential backoff; each batch carries an `insert_deduplication_token` so a retried insert is never stored twice (single-node mode sets `non_replicated_deduplication_window` so this also works without replication).
- **FR-IN-08** Disk spool: when ClickHouse is unreachable or an insert exceeds its deadline, batches are written to a local spool directory (compressed, default cap 20 GB). Spooled batches are replayed in order on recovery. Spool size and age are metrics with alert thresholds; when the cap is reached, the oldest spooled data is kept and new data is dropped with a counter and an alert.
- **FR-IN-09** Ingest filters (Admin, evaluated before write, ordered, first match wins): match on agent, HEP protocol type, SIP method, status code/class, source/destination IP/CIDR/port, user agent, or header regex; actions `drop`, `keep`, `sample 1:N`, `metadata_only` (store parsed fields, discard payload). Each rule shows live hit counters; a **dry-run** mode counts matches without acting. Example: drop `OPTIONS` between OpenSIPS and the monitoring host.
- **FR-IN-10** Duplicate suppression: an identical packet (same agent, same 5-tuple, same payload hash) seen again within 2 s is dropped and counted (configurable window; per collector).
- **FR-IN-11** Timestamps: capture time from HEP (`0x0009` + `0x000a`, microseconds) is the primary timestamp; collector receive time is stored as `ingest_ts`. Per agent, the Panel estimates clock offset (median of `ingest_ts − ts` over a sliding window); offset above 1 s (configurable) raises an agent-health warning and a call-view banner for calls involving that agent. Per-agent option: use collector time instead of agent time (for agents with bad clocks).
- **FR-IN-12** `vq-rtcpxr` collector: a SIP listener (default `:5063` UDP/TCP) accepts `PUBLISH` (and optionally `NOTIFY`) with `application/vq-rtcpxr` bodies (RFC 6035), answers `200 OK`, and stores the reports as QoS reports (FR-QS).
- **FR-IN-13** Unsupported HEP protocol types are counted per type; optionally (default off) stored raw in `hep_other` with 3-day retention.
- **FR-IN-14** Live feed: every decoded SIP message (and, optionally, QoS report and log line) is published to a non-blocking in-memory ring buffer per collector for the live view (FR-LV). Slow subscribers miss events; they never slow ingest.
- **FR-IN-15** Per-agent statistics: packets/s and bytes/s by protocol type, decode errors, auth failures, last seen, clock offset, HEP version, source IPs — shown in Admin → Agents and exported as metrics. "Agent silent for N minutes" is a built-in alert.
- **FR-IN-16** Collectors are stateless and horizontally scalable; they can sit behind HAProxy or IPVS (UDP and TCP). Pairing of REGISTER transactions (FR-RG-02) requires that one agent's stream reaches one collector, which source-IP hashing provides.

### 5.2 Protocol decoding
- **FR-DE-01** SIP parser (in-house, zero-copy, RFC 3261 tolerant): compact header forms, folded headers, case-insensitive names, multiple header instances, body by `Content-Length`, multipart bodies. A message that fails to parse is **stored** with `parse_error` set and its raw payload — never silently dropped.
- **FR-DE-02** Extracted SIP fields (stored as columns): method or status code + reason, CSeq number and method, Call-ID, From/To user, domain and tag, R-URI user and domain, P-Asserted-Identity user, Contact, Via top branch and transport, Max-Forwards, User-Agent/Server, Content-Type, Expires, Reason (protocol, cause, text), `has_sdp`, body size.
- **FR-DE-03** Custom headers: Admins configure a list of extra headers (default list: `X-CID`, `Diversion`, `P-Charging-Vector`, `Session-ID`, `P-Preferred-Identity`, `Privacy`, `Replaces`, `Referred-By`). Their values are stored in a `hdr` map column and automatically become searchable fields in the UI and columns in the SQL views.
- **FR-DE-04** SDP decode: origin, session connection, per media line — type, connection IP, port, transport profile, payload types and codec names (`rtpmap`), `fmtp`, `ptime`, direction attribute, presence of SRTP (SDES or DTLS fingerprint), ICE candidate summary. **SDES key material (`a=crypto` inline keys) is masked in the stored payload** by default.
- **FR-DE-05** Reason header and Q.850 cause decoding with a cause-to-text table (e.g. 16 Normal clearing, 17 User busy, 19 No answer, 21 Call rejected, 34 No circuit available, 127 Interworking).
- **FR-DE-06** Number extraction from SIP and `tel:` URIs (user part without parameters, visual separators removed), then normalization per FR-TP-04. Raw and normalized values are both stored.
- **FR-DE-07** Transport: from the HEP IP protocol chunk and the top Via; `ws`/`wss` recognised from Via (SIP over WebSocket, RFC 7118).
- **FR-DE-08** Media reports normalized into one schema (FR-QS-02): RTCP JSON (HEP type 5, rtpengine and Asterisk), RTCP-XR (type 35), agent RTP statistics (type 34), and `vq-rtcpxr` bodies (RFC 6035: loss, discard, burst/gap, round-trip and end-system delay, jitter buffer, MOS-LQ, MOS-CQ, R-factor, codec).
- **FR-DE-09** Logs (HEP type 100): plain text or JSON payload; Call-ID from the HEP correlation chunk or from Admin-configured regex rules.
- **FR-DE-10 (P2)** SIP-I/ISUP: multipart ISUP bodies decoded (IAM, ACM, ANM, REL, RLC; called/calling number, cause).
- **FR-DE-11 (P3)** DNS (type 53) and Diameter (type 56) decoding.
- **FR-DE-12** Robustness: all parsers are fuzzed in CI; a panic inside a parser is caught, counted and the message stored as `parse_error`; the collector keeps running.

### 5.3 Topology, aliases and enrichment
- **FR-TP-01** Topology groups: name, role (`subscriber`, `proxy`, `pbx`, `media`, `carrier`, `monitoring`, `other`), members (IP, CIDR, IP:port, port range), optional carrier/trunk name. The most specific match wins. Example groups: `FTTH subscribers` (public CIDRs/CGNAT ranges), `OpenSIPS cluster`, `Asterisk pool`, `rtpengine`, `Carrier-X`.
- **FR-TP-02** Aliases: IP/CIDR/IP:port → display name; agent id → name; CSV import/export. Published to ClickHouse as a dictionary (refresh ≤ 60 s) so renaming relabels history in views (AD-17).
- **FR-TP-03** At ingest each message is stamped with source and destination group and role; calls derive direction, ingress and egress group and trunk from these (FR-CR-06).
- **FR-TP-04** Number normalization: ordered regex rules with a test tool, producing a canonical form (E.164 digits without `+`). Example rules for Iraq (to confirm, §14 Q11): `^\+?964(\d+)$ → 964$1`, `^00964(\d+)$ → 964$1`, `^0(7\d{9})$ → 964$1`. A reversed copy of each normalized number is indexed so "last N digits" searches are fast.
- **FR-TP-05** GeoIP/ASN for public IPs: country, city (optional), ASN and organisation; private and CGNAT ranges labelled as such. The database (GeoLite2 with a licence key or DB-IP Lite) is refreshed monthly by the worker or uploaded manually for offline sites.
- **FR-TP-06** User-agent parsing into vendor, model and firmware for common phones and servers (Yealink, Grandstream, Fanvil, Poly, Cisco, Linphone, Zoiper, MicroSIP, Asterisk, FreeSWITCH, OpenSIPS, Kamailio); unknown strings kept raw. Rules are data, updatable without a release.
- **FR-TP-07** Capture-point merge: Admins can declare that several agents are the same host (e.g. OpenSIPS and rtpengine on one server) so they appear as one column in the ladder diagram.
- **FR-TP-08** Topology and normalization changes apply to new data. An Admin job "re-derive calls for a time range" (FR-CR-13) applies them to existing call records.

### 5.4 Correlation and call records (A to Z)
- **FR-CR-01** Dialog aggregation: a ClickHouse materialized view maintains one `dialogs` row per Call-ID with first/last timestamp, initial INVITE details (numbers, R-URI, source/destination, user agent, SDP codecs and media endpoints), first 18x time, 2xx time, final status, BYE/CANCEL time and sender tag, re-INVITE/UPDATE count, message count, capture points, Reason/Q.850 cause. It is built from inserts alone, so it works with any number of collectors.
- **FR-CR-02** Correlation keys: each dialog has the implicit key `cid:<Call-ID>`. Collectors add keys from the ordered correlation rules (§8.2) and write them to `call_links`:
  - HEP correlation chunk (`0x0011`) and OpenSIPS `correlate()` extra correlation ids → `cid:<value>`
  - headers carrying another leg's Call-ID (default `X-CID`; configurable list) → `cid:<value>`
  - `P-Charging-Vector` `icid-value` → `icid:<value>`; RFC 7989 `Session-ID` → `sid:<uuid>`
  - `Replaces` / `Referred-By` (transfer, pickup) → related-call link, not merged
- **FR-CR-03** A **call** is the connected group of dialogs that share any key (bounded to 64 legs and depth 8; larger groups raise a finding and are split at the bound). The `call_uid` is a UUIDv5 of the root leg's Call-ID, so it is stable across re-assembly.
- **FR-CR-04** Heuristic matching (off by default, per PBX group): if a B2BUA leg has no key, link it to a leg entering the same PBX group with the same normalized caller and callee within ±2 s. Such links are labelled **inferred** in the UI.
- **FR-CR-05** Finalizer (worker job, default every 5 s): reads dialogs changed since its watermark, expands key groups, and upserts `calls` (versioned), `lookup_index` and findings (FR-DX). Call state: `setup → ringing → answered → completed | failed | cancelled | timeout`. A call closes when every leg has a final state and nothing arrived for 30 s (close grace); it times out after 6 h without activity (both configurable). Late messages reopen the call and produce a new version.
- **FR-CR-06** Call record content:
  - **Root leg** = earliest initial INVITE entering from a `subscriber` or `carrier` group; **egress leg** = the leg that left towards the destination group.
  - **Caller** = root-leg From user (or P-Asserted-Identity, configurable); **callee** = root-leg R-URI user. Raw and normalized forms.
  - **Direction**: `onnet`, `outbound`, `inbound`, `transit` or `unknown`, from ingress and egress group roles; **trunk** = carrier name of the carrier group involved.
  - **Final status** as the caller saw it (root leg) **and** egress final status (what the carrier or callee returned), each with reason and Q.850 cause.
  - **End cause**: `caller_hangup`, `callee_hangup`, `cancelled`, `rejected`, `no_answer`, `busy`, `unreachable`, `network_timeout`, `ack_timeout`, `session_timer`, `unknown`; **hangup side** from the BYE sender's tag relative to the root leg.
  - Legs (Call-IDs with role and group), capture points traversed, codecs negotiated, user agents of both ends, caller/callee IP and country, QoS summary (FR-QS-05), findings, message count.
- **FR-CR-07** Timings per call: `start_ts`, `ring_ts`, `answer_ts`, `end_ts`; **PDD** (RFC 6076 SRD), setup time, talk time, SDD (BYE → its 200 OK); egress-leg PDD (carrier response time); per-hop delay (time between captures of the same message at consecutive capture points).
- **FR-CR-08** Forking (parallel and serial) is shown as branches of a leg; the winning branch is marked.
- **FR-CR-09** Transfers and pickups (`REFER`, `Replaces`) link calls as **related**; they are listed under "Related" and are not merged into one call record.
- **FR-CR-10** In-progress calls are searchable within 10 s of the first INVITE (state `setup`/`ringing`/`answered`).
- **FR-CR-11** Concurrent calls: the finalizer writes per-minute occupancy (answered calls active in each minute, by direction, trunk and group) to `kpi_concurrency_1m`; the current value is also a Prometheus gauge.
- **FR-CR-12** Correlation test tool (Admin/Engineer): pick a Call-ID or paste messages → shows extracted keys, matched rules, links and the resulting call.
- **FR-CR-13** Rebuild: an Admin can re-derive calls for a time range (bounded, rate-limited, audited) after changing rules, topology or normalization; raw messages are untouched.
- **FR-CR-14 (P2)** Lua hooks (sandboxed, time-limited) for custom key extraction at the collector.
- **FR-CR-15 (P2)** CDR/CEL ingest from Asterisk (HTTP JSON or HEP log) joined to calls by Call-ID or `linkedid`, shown in the call summary.

### 5.5 Registrations
- **FR-RG-01** All REGISTER requests and responses are stored in `reg_messages`, ordered by AoR and time.
- **FR-RG-02** Collectors pair each REGISTER with its final response (Call-ID + CSeq + Via branch). A 401/407 challenge followed by an authenticated retry counts as one attempt with `challenged = true`. No final response within 32 s → `no_response`.
- **FR-RG-03** `registration_events` (one row per attempt): time, AoR, normalized number, contact(s), source IP/port, transport, user agent (vendor/model/firmware), expires requested and granted, result (`registered`, `refreshed`, `deregistered`, `auth_failed`, `rejected` with code, `no_response`), registration delay (RFC 6076 RRD), node, country/ASN, NAT flag (Contact address differs from source address).
- **FR-RG-04** `registration_state` (latest per AoR and contact): last success, expiry time, last failure (code, time), consecutive failures, source IP/port, user agent, country, NAT flag, first seen. **Registered now** = expiry in the future and not deregistered.
- **FR-RG-05** Storage mode (platform setting, §14 Q4): `all` (default) stores every REGISTER message; `changes` stores messages only for failures, first registration, contact/IP/user-agent change, de-registration and 1 in N refreshes. Events and state are always complete.
- **FR-RG-06** Registration KPIs: active registrations (by group, domain, UA vendor/model, country), attempts/s, success ratio, failures by code, RRD p50/p95, ineffective registration attempts (RFC 6076 IRA), top failing AoRs, registration storms, flapping AoRs (changing IP/port), share behind NAT.
- **FR-RG-07** Registration timeline per AoR (state-timeline chart: registered / failing / unregistered), with each attempt clickable to its messages.

### 5.6 Media quality
- **FR-QS-01** Sources: rtpengine and Asterisk RTCP via HEP (type 5), RTCP-XR (type 35), phone `vq-rtcpxr` reports (FR-IN-12), agent RTP statistics (type 34; voip-panel agent in P2).
- **FR-QS-02** `qos_reports` normalized columns: time, Call-ID, source type, reporter, SSRC, stream addresses, direction relative to the call (caller→callee or callee→caller, from SDP endpoints), codec, packets sent/received, lost, fraction lost, jitter (ms), RTT (ms, from LSR/DLSR), discards, burst/gap metrics, MOS and R-factor, `mos_estimated` flag.
- **FR-QS-03** When a report carries no Call-ID, it is matched to a call by SDP media endpoint and time overlap.
- **FR-QS-04** `stream_qos` summary per stream: duration, packets, loss %, max/avg jitter, avg/max RTT, min/avg MOS, worst 10-second window, verdict. Default thresholds (configurable): MOS ≥ 4.0 good, 3.6–4.0 fair, 3.1–3.6 poor, < 3.1 bad.
- **FR-QS-05** Call QoS summary on the call record: worst MOS, max loss %, max jitter, max RTT, per-direction media presence.
- **FR-QS-06** When no MOS is reported, it is estimated with the ITU-T G.107 E-model (simplified) from codec, loss, jitter and delay and always labelled *estimated*.
- **FR-QS-07** Quality is available by any dimension: node, trunk, topology group (e.g. subscriber CIDR/OLT range), UA model, country/ASN, codec.

### 5.7 Logs and events
- **FR-LG-01** HEP logs (type 100) from OpenSIPS and Asterisk are stored in `logs`: time, agent, node, severity (parsed when possible), Call-ID, message, structured fields (JSON payloads as a map).
- **FR-LG-02** Logs linked to a call are shown in the call view (Logs tab and, optionally, as rungs on the ladder and markers on the timeline).
- **FR-LG-03** Log search by token, node, severity, Call-ID and time range.
- **FR-LG-04 (P2)** syslog (RFC 5424 over UDP/TCP/TLS) and OTLP logs receivers into the same table.

### 5.8 Search
- **FR-SR-01** **Universal search box** (top bar, `/` shortcut) accepts anything pasted and detects the type: phone number in any format (normalized per FR-TP-04), number suffix (`*1234`), Call-ID, X-CID or other correlation id, IPv4/IPv6 address or CIDR, SIP URI/AoR, user-agent prefix (`ua:`). Results are grouped: calls, subscribers (registration state), messages, agents. The time range defaults to the global time picker (default last 24 h) with one-click "search 7 d / 30 d / all retained".
- **FR-SR-02** Filter syntax in the same box for power users: `field:value` terms with comparison operators, e.g. `caller:07701234567 status:4xx trunk:"Carrier-X" pdd>3s mos<3.5 last:6h`. Field names autocomplete with a one-line description; values autocomplete from recent distinct values for low-cardinality fields (trunk, node, method, status, UA vendor, country, finding). Syntax errors are underlined with an explanation. The syntax is compiled to parameterized SQL; it is never concatenated into SQL text.
- **FR-SR-03** Structured search forms (no syntax needed) for **Calls**, **Registrations**, **Messages**, **Media quality** and **Logs**. Calls form: time range, caller, callee, either party, number prefix/suffix, Call-ID, IP/CIDR, direction, trunk, ingress/egress group, node, state, final status or class (caller-side or egress), Q.850 cause, end cause, hangup side, duration range, PDD range, MOS below, loss above, codec, UA vendor/model, country, finding codes, any custom-header field.
- **FR-SR-04** Results: virtualized table with column chooser, sorting, row density, status colours; a time-distribution histogram above the table (drag to narrow the range); a facet panel with top values and counts (status, trunk, direction, finding, UA vendor, node) — click to add as a filter. Counts are exact up to 1 M rows, then approximate and labelled as such.
- **FR-SR-05** Keyset pagination (stable while data arrives). Result lists export to CSV (up to 100,000 rows; Engineer and above).
- **FR-SR-06** Saved searches (private or shared, pin to sidebar), search history (last 100 per user), and every search is a deep link URL.
- **FR-SR-07** "Find similar" from a call: same caller, same callee, same trunk + status, or same finding, within ±1 h (adjustable).
- **FR-SR-08** Message search on any extracted field or custom header; token search in payloads through the token index; substring search on payloads is limited to ranges ≤ 1 h and warns about cost.
- **FR-SR-09** Registration search by AoR, number, IP, user agent or result, plus a "registered now" list with counts and facets.
- **FR-SR-10** Number semantics: exact (normalized), prefix (`9647701*`), suffix (`*1234`); local and international formats are equivalent after normalization.
- **FR-SR-11** Search is cancellable; long searches stream partial results with progress (rows scanned, share of range done).
- **FR-SR-12** Role rules apply: the Support role must enter a number and sees other parties masked (FR-AZ-04).
- **FR-SR-13** Zero results come with reasons and suggestions: the normalized form searched, agents with no data in the range, retention start date, "try suffix search", "widen to 7 days".
- **FR-SR-14** One timeout per query class, configured in one place (interactive 30 s, workbench 120 s, export 10 min). When hit, the UI names the limit and suggests how to narrow the query.

### 5.9 Call view
- **FR-CV-01** Every call has a URL `/calls/{call_uid}`; `/calls/by-callid/{call_id}` redirects to it. Calls open as full pages (or browser tabs), not small floating windows.
- **FR-CV-02** Summary header: caller → callee (raw, normalized, alias), state, final status as the caller saw it and egress status, end cause, hangup side, start time with time zone, PDD, setup time, talk time, direction, trunk, number of legs, nodes traversed, codecs, worst MOS, finding badges by severity, copy buttons (Call-IDs, share link).
- **FR-CV-03** Tabs: **Flow**, **Timeline**, **Messages**, **Media**, **Logs**, **Diagnostics**, **Registrations**, **Related**, **Export**. Default tab is a user preference.
- **FR-CV-04** **Flow (ladder diagram)**:
  - Columns are hosts by alias; capture points merged per FR-TP-07; default order follows the path (caller side → proxy → PBX → proxy → callee/carrier side); users can reorder, hide and group columns by topology group; layout is remembered per user.
  - One arrow per message labelled with method or status + reason and CSeq, coloured per leg; requests, provisional responses, final responses and errors (4xx amber, 5xx/6xx red) are visually distinct.
  - Markers: SDP present (codec list on hover), retransmission (dashed, "retx #n"), the same message captured at several points (stacked indicator, expandable to each capture time), out-of-order capture.
  - Time column: absolute time (`HH:mm:ss.SSSSSS`, user time zone) and delta from previous message or from call start (toggle); idle gaps over 5 s collapse into a labelled separator ("34.2 s").
  - Filters: legs, methods, hide provisional, hide OPTIONS, hide retransmissions, one capture point per message.
  - Optional inline rungs: QoS reports, log lines, registration events of both parties.
  - Clicking a message opens a detail drawer: header table (custom headers highlighted), raw text (copy), decoded SDP table, **hop diff** (headers added, removed or rewritten compared with the previous capture of the same message, e.g. by OpenSIPS), Q.850 decode, parse warnings.
  - Keyboard navigation (↑/↓ message, ←/→ leg), find-in-flow, zoom.
  - Up to 10,000 messages (virtualized; canvas mode); first render under 1 s for 2,000 messages.
  - Export as PNG, SVG and text (sngrep-style).
  - Clock-skew banner when an involved agent has an offset warning; optional **causal order** (transaction order first, then time) to undo skew-induced inversions.
- **FR-CV-05** **Timeline** (Gantt): one row per leg with phases (setup, ringing, early media, talking, teardown), re-INVITE events (hold/resume), poor-quality windows, log lines, registration events and alerts on one time axis, with a crosshair shared with the Media charts.
- **FR-CV-06** **Messages**: table of all messages (time, delta, source → destination alias, method/status, CSeq, leg, capture point, size) with filters; each opens the detail drawer.
- **FR-CV-07** **Media**: SDP offer/answer per leg (codecs, addresses, direction, SRTP), media path (who sends to whom, rtpengine anchoring), per-stream charts across the call (MOS, loss %, jitter, RTT) aligned with the timeline, and a stream summary table with verdicts.
- **FR-CV-08** **Logs**: correlated log lines in time order with severity filter.
- **FR-CV-09** **Diagnostics**: findings with severity, a plain-language explanation, the evidence messages (clickable) and a suggested next step.
- **FR-CV-10** **Registrations**: registration state of the caller and callee AoRs at call time and their events within ±1 h — answers "was the callee registered, and from where?".
- **FR-CV-11** **Related**: transfers and pickups, other calls between the same parties within ±1 h, retries (same caller → callee within 2 min).
- **FR-CV-12** **Export**: PCAP (all legs or selected legs, optional RTCP), text, JSON, share link (FR-EX).
- **FR-CV-13** An open in-progress call updates live.
- **FR-CV-14 (P2)** Side-by-side comparison of two calls (e.g. a working and a failing call to the same carrier).
- **FR-CV-15 (P2)** Optional external dissector: "Decode with tshark" in the message drawer (tshark installed on the API host, sandboxed, time-limited) for protocols the Panel does not decode natively.

### 5.10 Subscriber view
- **FR-SV-01** `/subscribers/{number}` accepts any number format and shows one page for that subscriber.
- **FR-SV-02** **Now**: registered / failing / not registered; each contact with UA vendor, model and firmware, public IP:port, country/ASN, NAT flag, expiry countdown, last failure and reason.
- **FR-SV-03** **History**: registration state timeline (default 24 h) and attempt list; IP and user-agent changes highlighted.
- **FR-SV-04** **Calls**: incoming and outgoing calls with status, duration, PDD, MOS and findings; charts of calls per hour and failures by code.
- **FR-SV-05** **Quality**: MOS and loss trend per day.
- **FR-SV-06** **Findings summary**, e.g. "registration failing with 408 since 09:12", "one-way audio in 3 of the last 10 calls".
- **FR-SV-07** This page is the Support role's entry point (FR-AZ-04).
- **FR-SV-08 (P2)** Export as a self-contained HTML report for escalation.

### 5.11 Diagnostics
- **FR-DX-01** The finalizer evaluates diagnostic rules for every call; Admins enable/disable built-in rules and edit thresholds; Engineers can add threshold rules over call fields (e.g. `egress_status = 503 AND trunk = 'Carrier-X'` → custom code). Lua rules **P2**.
- **FR-DX-02** Findings are stored on the call (`findings` codes) and in `call_findings` with evidence (message references, measured values).
- **FR-DX-03** Findings are search filters, metric dimensions and alert sources.
- **FR-DX-04** Built-in rules (thresholds configurable):

| Code | Severity | Detects |
|---|---|---|
| `NO_FINAL_RESPONSE` | critical | INVITE without final response (Timer B, 32 s) |
| `ACK_MISSING` | critical | 200 OK to INVITE retransmitted without ACK; call torn down about 32 s after answer |
| `DROP_AFTER_32S` | warning | Answered call ended 30–33 s after answer (typical ACK/NAT problem) |
| `AUTH_LOOP` | warning | ≥ 3 consecutive 401/407 without success (call or registration) |
| `NO_COMMON_CODEC` | warning | 488/606, or answer SDP with no codec in common with the offer |
| `ONE_WAY_AUDIO` | critical | Media reported in one direction only |
| `NO_MEDIA` | critical | Answered call with no media in either direction where media reporting is expected |
| `PRIVATE_IP_IN_SDP` | warning | SDP connection address private/CGNAT while signalling comes from a public address and media is not anchored |
| `HIGH_PDD` | warning | PDD > 5 s |
| `RETRANSMISSIONS` | warning | A request retransmitted ≥ 3 times on one hop |
| `CALLEE_NOT_REGISTERED` | info | On-net/inbound call to an AoR not registered at call time |
| `POOR_QUALITY` | warning | Worst MOS < 3.1, or loss > 5 %, or jitter > 50 ms for more than 10 s |
| `SESSION_TIMER_FAILURE` | warning | Session refresh (re-INVITE/UPDATE) failed, followed by BYE |
| `CARRIER_REJECT` | info | Egress leg rejected by a carrier (code + Q.850 cause) |
| `CAPTURE_GAP` | info | Message captured leaving point A but never at the expected next point B |
| `CLOCK_SKEW` | info | Capture times inconsistent across agents |
| `CANCEL_RACE` | info | 200 OK crossed a CANCEL |
| `LONG_EARLY_MEDIA` | info | 183 with media for > 60 s without answer |
| `HOLD` | info | Hold/resume re-INVITEs (`sendonly`/`inactive`) |
| `SCANNER` | warning | Requests from known scanner user agents (e.g. `friendly-scanner`, `sipvicious`) or blocked countries |
| `HIGH_RISK_DESTINATION` | warning | Callee matches the configured high-risk prefix list (toll-fraud indicator) |

### 5.12 Metric catalogue
- **FR-KP-01** Each catalogue entry has: id, name, description, formula per source resolution, unit, aggregation type (count, rate, ratio, percentile, gauge), allowed dimensions, default visualization, threshold bands, "good direction" (higher or lower is better) and minimum sample size.
- **FR-KP-02** Built-in metrics:

| Metric | Definition |
|---|---|
| Call attempts / CPS | Calls (root legs) started per bucket; CPS = peak or average per second (stated) |
| Answered calls | Calls whose root leg received 2xx to the initial INVITE |
| **ASR %** | Answered ÷ attempts |
| **NER %** | (Answered + user busy 486/600 + no answer 480/408-after-ringing + user rejected 603) ÷ attempts — network effectiveness excluding user behaviour |
| **SER %** (RFC 6076) | INVITEs answered 200 ÷ (INVITEs − INVITEs answered 3xx) |
| **SEER %** (RFC 6076) | INVITEs answered 200, 480, 486, 600 or 603 ÷ (INVITEs − 3xx) |
| **ISA %** (RFC 6076) | INVITEs answered 408, 500, 503 or 504 ÷ INVITEs |
| **SCR %** (RFC 6076) | Calls answered and ended by a normal BYE ÷ INVITEs |
| **PDD / SRD** | Initial INVITE → first 18x (or final response); p50/p90/p95/p99/avg |
| Setup time | Initial INVITE → 2xx |
| **ACD** | Average talk time of answered calls |
| Traffic | Total talk minutes; Erlangs per bucket |
| Concurrent calls | Average and maximum per bucket (FR-CR-11) |
| SDD (RFC 6076) | BYE → 200 OK |
| Failures | Count by status code, class, Q.850 cause, end cause |
| Registrations | Active, attempts, success %, failures by code, RRD p50/p95, IRA % (RFC 6076) |
| Media quality | MOS (p5/p50/avg), % calls poor/bad, loss % (avg/p95), jitter p95, RTT p95, one-way-audio count |
| SIP message rates | Messages/s by method and status class; retransmission ratio |
| Findings | Count by finding code |
| Capture health | HEP packets/s per agent, drops by reason, ingest lag, spool size |

- **FR-KP-03** Dimensions: direction, trunk, ingress/egress group, node/capture point, status class/code, Q.850 cause, end cause, codec, UA vendor/model, country, ASN, domain, finding code, number prefix (configurable depth), and Admin-flagged low-cardinality custom-header fields.
- **FR-KP-04** Source selection is automatic and visible in the panel info: raw `calls` for ranges ≤ 6 h or buckets < 1 min, 1-minute rollups for ≤ 90 days, 1-hour rollups beyond. Data newer than the settle window always comes from raw tables, so charts are never empty at the right edge.
- **FR-KP-05** Ratios are computed from summed numerators and denominators per bucket (never an average of ratios); percentiles from mergeable t-digest states, so values agree across resolutions.
- **FR-KP-06** Custom metrics: Engineers define metrics as parameterized SQL templates over the public views (`{{from}}`, `{{to}}`, `{{interval}}`, `{{filters}}`); they are marked *custom* with an owner and are usable like built-in ones.
- **FR-KP-07** Metric query API (`POST /api/v1/metrics/query`: metric, range, interval, dimensions, filters → series) used by the UI and external tools.
- **FR-KP-08** Prometheus exporter: an Admin-selected list of catalogue metrics and dimensions is exposed by the worker at `/metrics/kpi` with settled 1-minute values, so Zabbix, Prometheus and Grafana can use them without SQL.

### 5.13 Charts and time handling
This section answers the main complaint about HOMER 11. Every chart in the product, built-in or user-made, goes through the same components and time engine.

**Time engine**
- **FR-CH-01** All timestamps are stored in UTC (`DateTime64(6)`); the API returns RFC 3339 UTC with microseconds; the UI formats in the user's time zone (platform default `Asia/Baghdad`, per-user override). The active time zone is always shown in the time picker and in chart footers.
- **FR-CH-02** Buckets are computed server-side with `toStartOfInterval(ts, INTERVAL n unit, '<user tz>')`, so day and hour buckets align with local midnight and local hours, and every panel on a dashboard uses the same bucket boundaries.
- **FR-CH-03** Automatic interval: `interval = nice_step(range ÷ max_points)`, with `max_points = min(panel width in px ÷ 3, 1,500)` and nice steps 1 s, 5 s, 10 s, 30 s, 1 m, 5 m, 15 m, 30 m, 1 h, 3 h, 6 h, 12 h, 1 d, 7 d. The interval never goes below the source's resolution (1 s raw, 1 m and 1 h rollups). Users can override (Auto / fixed value); the effective interval is shown on the panel ("1 m buckets").
- **FR-CH-04** Missing data: count and rate series are zero-filled (`WITH FILL`); ratio, latency and quality series show a gap (null) where there are no samples — lines are never drawn across missing data.
- **FR-CH-05** Partial data: the current, incomplete bucket and buckets inside the settle window are drawn distinctly (dashed line or hatched bar) and excluded from "last value" statistics unless the user opts in.
- **FR-CH-06** Rates are labelled with their unit (per second or per minute) and stay on the same scale when the bucket size changes; counts are labelled "per bucket".
- **FR-CH-07** Buckets with fewer samples than the metric's minimum (e.g. ASR from 3 calls) are greyed and flagged in the tooltip.
- **FR-CH-08** Time picker: quick ranges (5 m, 15 m, 1 h, 6 h, 24 h, 7 d, 30 d, today, yesterday, this week, last week), absolute range with calendar and time, relative expressions (`now-1h`), step back/forward by one range, auto-refresh (off, 5 s, 10 s, 30 s, 1 m, 5 m), live mode for ranges ≤ 1 h. The range lives in the URL.
- **FR-CH-09** Ingest lag (newest data timestamp vs now) is shown on dashboards; if lag exceeds 60 s the right edge of charts is marked "data delayed by N s".

**Rendering**
- **FR-CH-10** One chart library (Apache ECharts) wrapped in voip-panel components with one theme (light and dark), one typography scale, and one number/time formatter. Built-in dashboards contain no iframes or third-party panels.
- **FR-CH-11** Time-axis labels: ticks on round boundaries; format by span (`HH:mm:ss` < 15 min, `HH:mm` < 2 days, `MMM d HH:mm` < 14 days, `MMM d` beyond); date shown at each day change; labels never overlap (automatic thinning).
- **FR-CH-12** Linked panels: shared crosshair and synchronized tooltips across all panels of a dashboard; the tooltip shows the exact bucket start–end, value with unit, sample count and series, sorted by value.
- **FR-CH-13** Drag-to-zoom on any time chart sets the dashboard range; browser Back undoes it; double-click resets.
- **FR-CH-14** Click-through: any point, bar or table cell opens search pre-filtered with that bucket's time range and that series' dimension values (e.g. click the 503 bar at 14:05 → the calls that failed with 503 in that minute).
- **FR-CH-15** Colours: stable colour per series value across all panels (hash-based palette, colour-blind safe); fixed semantic colours for SIP classes (2xx green, 3xx blue, 4xx amber, 5xx red, 6xx purple) and quality verdicts. High-cardinality dimensions show top-N plus "other".
- **FR-CH-16** Legends: toggle and isolate series, sortable, with min/max/avg/last/total columns.
- **FR-CH-17** Units and numbers: SI prefixes (k, M), automatic ms/s/min, percentages with fixed decimals per metric, thousands separators by locale.
- **FR-CH-18** Panel types: time series (line, area, stacked, bars), stat (with sparkline and change vs previous period), gauge, bar (top-N), table (with sparklines and colour bands), heatmap (e.g. status code × time, PDD distribution × time), histogram, pie/donut (≤ 6 slices, else bar), geo map, state timeline (registration state), call list, log list, text/markdown.
- **FR-CH-19** Comparison: overlay the same series shifted by 1 day or 1 week.
- **FR-CH-20** Annotations on time charts: alerts fired/resolved, configuration changes, manual notes, deployment events posted through the API.
- **FR-CH-21** Performance: canvas renderer; client-side LTTB down-sampling above 10,000 points per series; queries for a panel are cancelled when the range changes; a 24 h panel renders within 1 s at p95 (NFR-02).
- **FR-CH-22** Clear empty and error states: "no data in range", "query error (detail)", "data source delayed".
- **FR-CH-23** Export per panel: PNG (current theme), CSV of exactly the plotted series, and "Show query" (the generated SQL, read-only, openable in the SQL workbench — a way to learn the data model).
- **FR-CH-24** Visual regression tests (Playwright screenshots) for every built-in panel type in light and dark themes, at three widths and three range sizes, run in CI.

### 5.14 Dashboards
- **FR-DB-01** Dashboards are grids of panels (drag, resize, duplicate) with variables (trunk, node, direction, group, number, country …) shown as dropdowns that filter all panels; variable values come from the field catalogue (distinct values) or fixed lists.
- **FR-DB-02** Panels are built **without SQL**: choose metric(s) from the catalogue → filters → group-by dimensions → visualization → thresholds. Engineers may instead bind a panel to a saved workbench query.
- **FR-DB-03** Built-in dashboards (read-only, versioned with the product; users clone them to customize):

| Dashboard | Content |
|---|---|
| **NOC overview** (home) | CPS, concurrent calls, ASR, NER, PDD p95, failures by class, top failing destinations and trunks, active registrations and registration failure ratio, MOS distribution, open alerts, ingest health |
| **Calls & traffic** | Attempts/answered by direction and trunk, Erlangs, ACD, status-code heatmap, Q.850 causes, end causes, top callers/callees by volume |
| **Registrations** | Active registrations (by group, domain, UA model, country), attempts/s, success %, failures by code, RRD p95, storms, flapping AoRs, NAT share, top failing AoRs |
| **Media quality** | MOS (p5/p50), poor/bad call share, loss, jitter, RTT by node/trunk/group/UA model/codec, one-way-audio and no-media counts, worst calls list |
| **Trunks & carriers** | Per carrier: CPS, concurrent calls vs capacity, ASR, NER, PDD, ACD, rejects by code/cause, quality |
| **SIP nodes** | Per OpenSIPS/Asterisk/rtpengine node: message rates by method/status, retransmission ratio, per-hop delay, error responses originated |
| **Errors & security** | 4xx/5xx spikes, auth failures by source IP/country, scanner traffic, high-risk destinations, unusual call volume per account |
| **Capture health** | HEP packets/s per agent, drops by reason, agent clock offset, ingest lag, spool size, ClickHouse insert latency, parts and merges, disk usage per table |
| **Subscriber** | Variable `number`: registration state timeline, contacts, calls, failures, quality (also embedded in FR-SV) |

- **FR-DB-04** User dashboards: private, shared (read) or shared (edit); folders; favourites; a per-user home dashboard; version history with restore; import/export as JSON.
- **FR-DB-05** TV/kiosk mode: full screen, auto-refresh, optional rotation between dashboards, readable from a distance (large font preset).
- **FR-DB-06** Every panel supports click-through to search (FR-CH-14) and "open in Explore" (FR-DB-07).
- **FR-DB-07** **Explore**: an ad-hoc page to pick a metric, filters, dimensions and visualization quickly, then save as a panel.
- **FR-DB-08** Dashboard links: a URL captures range, variables and refresh; snapshot links (P2) freeze data for sharing.
- **FR-DB-09 (P2)** Grafana panel embed (iframe) for teams that keep some Grafana dashboards.

### 5.15 SQL workbench
- **FR-SQ-01** Editor (CodeMirror 6, ClickHouse dialect) with autocomplete of views, columns and functions, column descriptions on hover, formatting, snippets and multiple tabs.
- **FR-SQ-02** Schema browser built for people: public views with stable names and documented columns (`v_calls`, `v_legs`, `v_messages`, `v_registrations`, `v_registration_state`, `v_qos_streams`, `v_qos_reports`, `v_logs`, `v_kpi_calls_1m`, `v_kpi_calls_1h`, `v_kpi_reg_1m`, `v_findings`); each column has a description (ClickHouse `COMMENT`) and example values. Internal tables are visible to Admins only.
- **FR-SQ-03** Query library: at least 30 built-in, categorised, parameterized examples (e.g. "Top 20 destinations by failures", "Calls with ACK missing per node", "Registrations from new countries today", "MOS by UA model", "Carrier PDD p95 per hour"), plus saved queries (private or shared) and per-user history.
- **FR-SQ-04** Macros bound to the time picker and variables: `{{from}}`, `{{to}}`, `{{interval}}`, `{{tz}}`, `{{number}}`, `{{trunk}}`.
- **FR-SQ-05** Results: virtualized table, quick chart (time series detected automatically), export CSV/JSON (row cap per role), "create panel from this query", query statistics (rows and bytes read, duration, memory).
- **FR-SQ-06** Safety (AD-8): queries run as the ClickHouse user mapped to the caller's role with `readonly = 2` and a settings profile whose **constraints** stop users from raising the limits (`max_execution_time` 120 s, `max_memory_usage` 8 GB, `max_result_rows` 100,000, `max_rows_to_read`/`max_bytes_to_read` caps), per-user quotas and, in P2, row policies for data scopes. These users hold `SELECT` on the public views only and no `SOURCES` privileges (file, URL, remote, S3 and other table-function sources), so only read queries on the lake data are possible. Every query (text hash, duration, rows read) is audited. Running queries can be cancelled.
- **FR-SQ-07** Available to Engineer and Admin roles only.
- **FR-SQ-08 (P3)** Natural-language-to-SQL assistant over the documented views (optional external or local LLM), always showing the generated SQL before running it.

### 5.16 Live view
- **FR-LV-01** In-browser live capture (sngrep-style): a continuously updating list of dialogs grouped by Call-ID (method, from, to, state, message count, last activity), filterable by number, IP/CIDR, method, status class, node and user agent before the stream starts.
- **FR-LV-02** Selecting a dialog shows its ladder updating live; one click opens the full call view once the call is finalized.
- **FR-LV-03** The API fans in from all collectors over internal WebSocket connections; filters are applied at the collectors so only matching events cross the network.
- **FR-LV-04** Limits: per-user stream rate cap (default 500 events/s; excess is sampled and the UI says so), maximum concurrent live sessions (default 20), automatic stop after 30 minutes idle.
- **FR-LV-05** Pause, resume, clear, and save the captured buffer as PCAP.

### 5.17 Export, import and sharing
- **FR-EX-01** PCAP export of one or more calls (multi-select up to 100 calls, Engineer; single call, NOC Operator): all correlated legs or selected legs, optional RTCP/HEP logs, reconstructed as UDP/TCP packets with original addresses and timestamps; pcap by default, pcapng with per-packet capture-point comments (P2).
- **FR-EX-02** Text export (sngrep-style flow plus full messages) and JSON export (call record, legs, messages, QoS, findings).
- **FR-EX-03** Export exclusion list: hosts/CIDRs whose packets are removed from exports (e.g. internal management addresses), as in HOMER.
- **FR-EX-04** Large exports run as background jobs with progress; results are downloadable for 24 h; every export is audited (who, what, how many calls).
- **FR-EX-05** Share links: signed, random token; read-only view of exactly one call (summary, flow, media, diagnostics — no search, no navigation); expiry default 7 days, maximum 30; optional password; revocable; every access is logged with IP. Payload visibility and number masking on shared links are configurable per link.
- **FR-EX-06** PCAP import: upload pcap or pcapng (≤ 500 MB, Engineer) through UI or API; SIP over UDP and TCP (with reassembly) and RTP/RTCP are parsed into the same tables under an `import_id`, shown only to the uploader and users it is shared with, retained 30 days, then deleted. Imported calls get the full call view and diagnostics.
- **FR-EX-07 (P2)** Self-contained HTML call report (summary, flow, media, findings) for carrier escalations.

### 5.18 Alerting and notifications
- **FR-AL-01** Rule types: threshold on a catalogue metric with dimensions and filters (e.g. ASR by trunk < 40 % for 5 min with ≥ 50 attempts); absence (no data from agent X for N min); change (active registrations drop > 20 % in 5 min); finding count (≥ 10 `ONE_WAY_AUDIO` in 15 min per node); capture health (drops, spool, ingest lag). Anomaly vs same time last week **P2**.
- **FR-AL-02** Evaluation every 30 s by the worker (leader-elected) on settled data plus raw data for the last minutes; states `ok → pending → firing → resolved` with "for" duration and hysteresis; per-dimension alert instances (one per trunk).
- **FR-AL-03** Notification channels: email (SMTP relay), generic webhook (JSON, HMAC-signed, retried), Microsoft Teams and Slack-compatible webhooks, Telegram bot, Prometheus Alertmanager forward. Routing by severity and labels; repeat interval; grouping.
- **FR-AL-04** Silences and maintenance windows (by rule, label, node or trunk; scheduled or ad hoc), with an audit trail.
- **FR-AL-05** Every alert notification contains a link to the relevant dashboard with the alert's time range and a link to the matching search (the HOMER "open in search" behaviour).
- **FR-AL-06** Alert history: list and timeline, acknowledgement with comment, annotations on charts (FR-CH-20).
- **FR-AL-07 (P2)** Inbound alert webhook (`POST /api/v1/alerts/inbound`) for Grafana or other sources, stored and shown with drill-down to search.

### 5.19 Identity and authentication
- **FR-ID-01** Local accounts: username or email + password (Argon2id), password policy (≥ 12 characters, optional breached-password check), TOTP MFA (mandatory for Admin, enforceable per role), progressive lockout and login rate limiting.
- **FR-ID-02** LDAP / Active Directory: LDAPS or StartTLS, search-then-bind, group → role mapping, nested groups (AD `LDAP_MATCHING_RULE_IN_CHAIN`), connection pool, multiple servers for failover.
- **FR-ID-03** RADIUS: RFC 2865 (PAP; CHAP where the server supports it), shared secret per server, primary/secondary servers, timeout/retries; role from a configurable attribute (`Class` or `Filter-Id`) mapping; optional RADIUS accounting of logins (RFC 2866).
- **FR-ID-04** OIDC SSO (e.g. the existing Keycloak): Authorization Code + PKCE, multiple providers, claim/group → role mapping, just-in-time provisioning, identities linked by issuer + subject.
- **FR-ID-05** Login order and fallback configurable (e.g. OIDC button + LDAP form; RADIUS for NOC); break-glass local Admin always available (FR-ID-08).
- **FR-ID-06** Sessions: server-side (PostgreSQL), secure HttpOnly SameSite cookies, CSRF tokens, idle timeout 30 min and absolute 12 h (configurable), session list and "sign out everywhere".
- **FR-ID-07** API tokens: created by a user or for a service account; secret shown once; stored as a hash with a lookup prefix; mandatory expiry (default 90 days, maximum 365); scope ≤ owner's permissions with optional narrowing (read-only, specific endpoints); last-used time and IP; revocable.
- **FR-ID-08** First start: no default credentials. `vpctl admin create` or a one-time bootstrap token printed in the log creates the first Admin, who must enrol TOTP.
- **FR-ID-09** Inactivity: local users disabled after 90 days without interactive login (configurable); warning email 7 days before.

### 5.20 Authorization
- **FR-AZ-01** One central policy function answers every authorization question for UI, API, live view and exports; denials are audited.
- **FR-AZ-02** Permissions (assigned to roles in §4.1; custom roles composed by Admin): `search`, `view_payload`, `export_single`, `export_bulk`, `import_pcap`, `share_link`, `live_view`, `sql_workbench`, `dashboards_edit`, `alerts_edit`, `alerts_ack`, `topology_edit`, `rules_edit`, `agents_manage`, `retention_manage`, `users_manage`, `auth_manage`, `audit_read`, `settings_manage`.
- **FR-AZ-03** Payload visibility can be removed per role (headers-only view: method, codes, numbers, timing, no bodies).
- **FR-AZ-04** Support role: must enter a number (subscriber view or call search by number); other-party numbers masked except the last 4 digits (configurable); no listing of unrelated traffic.
- **FR-AZ-05 (P2)** Data scopes per user/token (§4.2), enforced in the API and as ClickHouse row policies.

### 5.21 Audit
- **FR-AU-01** Audited events: authentication (success/failure, MFA, logout), authorization denials, token lifecycle and use, user/role/auth-source changes, call views (who opened which call), searches by number (number hash + count), exports, imports, share-link creation/access/revocation, SQL workbench queries, live-view sessions, topology/alias/rule/filter changes, retention changes, agent key changes, alert rule changes, silences, settings changes.
- **FR-AU-02** Event schema: `id, ts (UTC), actor {type: user|token|system, id, display}, auth_method, src_ip, user_agent, request_id, action, object {type, id, name}, outcome, severity, details`.
- **FR-AU-03** Local store: append-only PostgreSQL table partitioned by month; the application role has no UPDATE/DELETE privilege; optional hash chain; retention 365 days (configurable); pruning is itself audited.
- **FR-AU-04** Review UI for Admin and Auditor: filters (time, actor, action, object, outcome), detail view, CSV/JSON export.
- **FR-AU-05** QRadar forwarding: LEEF 2.0 over TLS syslog (RFC 5425, port 6514) with configurable vendor/product strings and a stable event-id catalogue; at-least-once delivery through a local outbox with retry, backlog metric and alert; "send test event" button. Plain syslog, CEF and JSON-over-syslog for other SIEMs.
- **FR-AU-06** Audit never blocks the user action: the local event is written in the same transaction as the change (or immediately for read events); forwarding is asynchronous.

### 5.22 Administration
- **FR-AD-01** **First-run setup** in the UI: admin account + TOTP, platform time zone, ClickHouse topology check, first agent key, and a config generator that produces ready-to-paste OpenSIPS, Asterisk and rtpengine snippets with the right addresses, ports and keys (§8.3).
- **FR-AD-02** **Agents**: list with status, rates, errors, clock offset, versions; create/rotate keys; approve quarantined agents; per-agent options (time source, merge into host).
- **FR-AD-03** **Topology & aliases** editor with CSV import/export and a "where does this IP belong" tester.
- **FR-AD-04** **Correlation rules**, **number normalization**, **custom headers**, **user-agent rules**, **ingest filters** and **diagnostic thresholds** editors, each with a test tool and change history.
- **FR-AD-05** **Notification channels** and SMTP settings with "send test".
- **FR-AD-06** **Retention & storage**: one row per data type (§7.4) with retention days, hot days (when a cold volume exists), current disk usage, compression ratio, rows, oldest data, projected disk at current ingest rate; changes are applied as `ALTER TABLE … MODIFY TTL` by the worker and audited.
- **FR-AD-07** **System health**: collectors (queues, drops, spool), workers (job lag, finalizer watermark lag), ClickHouse (version, replicas and replication lag, parts per partition, merges, disk per volume, slow queries), PostgreSQL (connections, size), certificates expiry, GeoIP database age.
- **FR-AD-08** **Backups**: status of ClickHouse backups (`BACKUP … TO S3` or clickhouse-backup) and PostgreSQL backups as reported by the operator's tooling; the Panel shows last success and alerts on age.
- **FR-AD-09** **Settings** export/import (all PostgreSQL configuration as one JSON/YAML bundle, secrets excluded) for staging → production promotion.

### 5.23 API and CLI
- **FR-API-01** REST/JSON under `/api/v1`; OpenAPI 3.1 generated from code, served at `/api/v1/openapi.json` with interactive docs at `/api/docs`; contract-tested in CI. The UI uses only this API.
- **FR-API-02** Authentication: `Authorization: Bearer <token>` for integrations; session cookie + CSRF for the UI.
- **FR-API-03** Conventions: cursor pagination, filtering and sorting, RFC 9457 Problem Details, request ids, `Idempotency-Key` on POST, rate limits per token and per user with `429` + `Retry-After`.
- **FR-API-04** Resource groups: `calls` (search, get, legs, messages, flow, media, qos, logs, findings, related, pcap, text, json), `registrations` (search, events, state), `subscribers/{number}`, `messages`, `qos`, `logs`, `search` (universal), `metrics` (catalogue, query), `dashboards`, `saved-searches`, `queries` (workbench, Engineer), `live` (WebSocket), `exports`, `imports`, `share-links`, `alerts` (rules, instances, silences, channels), `annotations`, `topology`, `aliases`, `agents`, `rules`, `retention`, `users`, `roles`, `tokens`, `auth-sources`, `audit`, `system`.
- **FR-API-05** Versioning: additive changes within v1; breaking changes only in v2 with a published deprecation window.
- **FR-API-06** CLI `vpctl`: `search` (table/JSON/CSV/flow/pcap output), `call get|flow|pcap`, `subscriber`, `sql` (workbench rules apply), `agents`, `admin create`, `config check|init`, `migrate`, `retention`, `export-settings`/`import-settings`, `bench` (HEP load generator for testing).
- **FR-API-07 (P2)** MCP server (read-only tools: search calls, get call, get flow, registration status, query metric) using the same authorization, for LLM assistants.

### 5.24 Web UI
- **FR-UI-01** Navigation: Home (NOC overview), Search, Live, Dashboards, Explore, Subscribers, SQL Workbench, Alerts, Imports, Admin. Role-aware (items without permission are hidden).
- **FR-UI-02** Light and dark themes, compact/comfortable density, keyboard shortcuts (with a `?` cheat sheet), deep links everywhere, browser back/forward works for every state change.
- **FR-UI-03** User preferences: time zone, date/time format (24 h default), number display (raw/normalized), default call-view tab, ladder options, home dashboard, theme.
- **FR-UI-04** Responsive down to 1280 px for full use; read-only dashboards and call views usable on tablets.
- **FR-UI-05** Accessibility WCAG 2.1 AA; ladder and tables keyboard-operable.
- **FR-UI-06** Internationalization: all strings externalized; English in phase 1; Arabic with full RTL **P2** (§14 Q9).
- **FR-UI-07** Supported browsers: current and previous major versions of Chrome, Edge and Firefox.

### 5.25 Capture agent (P2)
- **FR-AG-01** `vp-agent` (Rust, Linux x86_64/aarch64): capture with AF_PACKET (TPACKET_V3) and BPF filters, libpcap fallback; SIP over UDP and TCP (stream reassembly); RTP/RTCP observation.
- **FR-AG-02** Computes per-stream RTP statistics (packets, loss, jitter, sequence errors, estimated MOS) and sends periodic and end-of-stream reports (HEP type 34) correlated to the SIP Call-ID from observed SDP.
- **FR-AG-03** HEPv3 output over UDP, TCP or TLS to one or more collectors, authentication key, compressed payloads (`0x0010`), agent name chunk, local disk buffer for outages, rate limits.
- **FR-AG-04** Central configuration: the agent pulls its config (filters, destinations) from the Panel API with its token; status and statistics reported back (Admin → Agents).
- **FR-AG-05** Packages: RPM, DEB, container image; systemd unit with minimal capabilities (`CAP_NET_RAW`, `CAP_NET_ADMIN` only).
- **FR-AG-06 (P3)** RTP payload capture to object storage for playback (§14 Q5).

### 5.26 Integrations
- **FR-INT-01** Prometheus endpoints: `/metrics` on every role (process, ingest, queue, insert, finalizer metrics) and `/metrics/kpi` (FR-KP-08). Zabbix consumes them with its Prometheus item types.
- **FR-INT-02** Grafana: documented read-only ClickHouse user and public views (FR-SQ-02) for the official ClickHouse data source plugin; example Grafana dashboards shipped as JSON.
- **FR-INT-03** QRadar audit forwarding (FR-AU-05).
- **FR-INT-04 (P2)** Kafka: optional consumer (HEP or decoded JSON from a topic) and producer (decoded messages, call records, findings to topics) for other systems.
- **FR-INT-05 (P2)** HEP forwarding: collectors forward selected traffic (by filter) to another HEP receiver (e.g. a vendor tool or a second Panel).
- **FR-INT-06 (P2)** SIPREC SRS: SIP listener accepting recording sessions (RFC 7865/7866) and storing signalling and recording metadata (no media in phase 2).

---

## 6. Non-functional requirements

| ID | Requirement |
|---|---|
| NFR-01 | **Scale.** Phase 1: 10,000 subscribers, 500 concurrent calls, ~3 CPS, ~150 SIP messages/s and ~400 QoS reports/s at busy hour. **Design target:** 2,000,000 subscribers, 100,000 concurrent calls, ~560 CPS, ~30,000 SIP messages/s and ~80,000 QoS reports/s at busy hour (Appendix A), 50 agents, 200 concurrent UI users, 50 wall screens. The schema and code do not change between the two; only hardware and ClickHouse topology do. |
| NFR-02 | **Latency (p95).** Message searchable ≤ 5 s after capture; call record ≤ 10 s after first INVITE; call by Call-ID < 500 ms over full retention; number search over 24 h < 1 s and over 30 days < 5 s; dashboard panel over 24 h < 1 s and over 30 days < 3 s; call view first paint < 1.5 s; ladder with 2,000 messages < 1 s. |
| NFR-03 | **Throughput.** One collector (8 vCPU, 16 GB) sustains ≥ 100,000 HEP packets/s decode + parse + write; collectors scale linearly; the finalizer handles ≥ 2,000 dialog updates/s per instance. |
| NFR-04 | **Availability.** UI/API 99.9 % monthly; capture path 99.95 % (≥ 2 collectors behind HAProxy/IPVS in production). A ClickHouse outage shorter than spool capacity loses no data. A PostgreSQL outage does not stop ingest: collectors cache agent keys, topology and rules. |
| NFR-05 | **Durability.** Production ClickHouse runs replicated (2 replicas, 3 Keeper nodes); daily backups to S3/MinIO (weekly full, daily incremental). PostgreSQL with WAL archiving and point-in-time recovery: RPO ≤ 15 min, RTO ≤ 1 h. Restore drills documented and tested. |
| NFR-06 | **Bounded resources.** Every ClickHouse query from the Panel carries `max_execution_time` and `max_memory_usage` for its query class; collector queues are sized in bytes; no unbounded caches. No component can be pushed into an out-of-memory crash by a user query. |
| NFR-07 | **Security.** OWASP ASVS Level 2; TLS 1.2+ for UI/API, HEP-TLS, ClickHouse, PostgreSQL, LDAP, RADIUS-over-TLS where used, syslog; secrets from files, environment, Kubernetes Secrets or Vault; stored integration secrets (SMTP, LDAP bind, RADIUS, webhook HMAC) envelope-encrypted; CSP, HSTS, SameSite cookies, CSRF tokens; webhook targets SSRF-safe (private ranges denied unless allow-listed); parsers fuzzed; `cargo-deny`/`cargo-audit` and image scanning in CI; signed images (cosign) and SBOM; penetration test before production. |
| NFR-08 | **Privacy.** Captured data contains personal data (numbers, IP addresses, message content). Retention limits are enforced by TTL; access to call content is audited; Support-role masking; SDES keys masked; erasure by number (audited ClickHouse lightweight-delete job) for data-subject or legal requests. |
| NFR-09 | **Observability.** Prometheus metrics on every role (ingest per agent/proto, drops by reason, queue depth, insert latency and errors, spool size/age, finalizer lag, query latency per class, alert evaluation), structured JSON logs with request ids, OpenTelemetry traces for API requests and ClickHouse queries, `/healthz` and `/readyz` per role. |
| NFR-10 | **Deployment.** All-in-one (Docker Compose or Podman Quadlet: `vpd --role all` + ClickHouse + PostgreSQL) running in under 15 minutes; RPM with one systemd unit per role; Helm chart (API with HPA; collectors as Deployment with UDP/TCP Services or DaemonSet with host networking; worker ≥ 2 replicas with leader election; PodDisruptionBudgets; NetworkPolicies; migrations as a pre-upgrade Job). Production ClickHouse is external to the chart (VMs or an operator). |
| NFR-11 | **Upgrades.** Versioned migrations for PostgreSQL and ClickHouse; zero-downtime upgrades for API and collectors (expand/contract); ClickHouse migrations are online operations (add column, add index, modify TTL, new materialized view); rewriting large tables only via an explicit Admin-approved job. |
| NFR-12 | **Configuration.** Bootstrap settings in TOML + `VP_*` environment variables; runtime settings in PostgreSQL editable in the UI; `vpctl config check` validates before start. |
| NFR-13 | **Testing.** Unit and property tests for all parsers; continuous fuzzing; a golden corpus of anonymized real PCAPs with expected call records and findings; an end-to-end lab (OpenSIPS + Asterisk + rtpengine containers, SIPp scenarios: basic call, busy, no answer, cancel, auth challenge, forking, transfer, ACK loss, one-way audio, carrier 503) asserting call records and findings; load test at the NFR-01 design target before go-live; visual regression for charts and the ladder. |
| NFR-14 | **Documentation.** Operator guide, integration guide (OpenSIPS, Asterisk, rtpengine, heplify, phones), data dictionary of public views, metric definitions, API reference, runbooks (ingest drops, spool growth, ClickHouse disk full, replica lag, finalizer lag, agent clock skew, restore). |
| NFR-15 | **Code quality.** Rust workspace with `rustfmt`, `clippy -D warnings`, no `unsafe` outside audited modules, documented minimum Rust version, reproducible builds; frontend in strict TypeScript with ESLint; initial JS bundle < 500 KB gzipped. |
| NFR-16 | **Licensing.** No HOMER source code is copied: HOMER is AGPL-3.0, so voip-panel takes features and ideas only and implements them independently. Dependencies must be permissively licensed (MIT, Apache-2.0, BSD, ISC, MPL-2.0), enforced by `cargo-deny` and a frontend licence check. voip-panel's own licence is an open question (§14 Q15). |

---

## 7. Data model

### 7.1 ClickHouse tables (logical)
All tables live in database `vp`. In replicated mode engines are the `Replicated*` variants; in sharded mode (P2) each table has a `Distributed` front table sharded by `cityHash64(call_id)` (message tables) or `cityHash64(call_uid)` (call tables). Every captured-data table has `import_id LowCardinality(String)` (empty for live capture).

| Table | One row per | Engine | Partition | Sort key | Default TTL | Notes |
|---|---|---|---|---|---|---|
| `sip_messages` | captured SIP message except REGISTER | MergeTree | day | `(call_id, ts)` | 30 d; `kind='other'` (OPTIONS, NOTIFY, SUBSCRIBE, MESSAGE, PUBLISH, INFO outside dialogs) 7 d; cold after 7 d | bloom filters on numbers; token index on payload |
| `reg_messages` | REGISTER request/response | MergeTree | day | `(aor, ts)` | 7 d | storage mode FR-RG-05 |
| `call_links` | (Call-ID, correlation key) | MergeTree (duplicate edges are harmless) | month | `(key, call_id)` + projection `(call_id, key)` | 35 d | written by collectors |
| `dialogs` | Call-ID | AggregatingMergeTree (MV from `sip_messages`) | day | `(call_id)` | 35 d | per-dialog aggregate states; a dialog crossing midnight has rows in two partitions, merged at read time with `-Merge` + `GROUP BY call_id` |
| `calls` | correlated call | ReplacingMergeTree(`version`) | month | `(toDate(start_ts), call_uid)` | 365 d | written by finalizer |
| `call_findings` | (call, finding code) | ReplacingMergeTree | month | `(code, start_ts, call_uid)` | 365 d | evidence as JSON |
| `lookup_index` | (identifier, call) | ReplacingMergeTree | month | `(key_type, key, ts, call_uid)` | 365 d | numbers, reversed numbers, Call-IDs, correlation ids, IPs, AoRs |
| `registration_events` | registration attempt | MergeTree | day | `(aor, ts)` | 30 d | written by collectors |
| `registration_state` | (AoR, contact) | AggregatingMergeTree (MV from `registration_events`) | none | `(aor, contact)` | delete 90 d after `last_seen` | `argMaxIf` states keep last success and last failure independently; `last_seen` is `SimpleAggregateFunction(max)` |
| `qos_reports` | media report | MergeTree | day | `(call_id, ts)` | 7 d | normalized RTCP / RTCP-XR / VQ / agent |
| `stream_qos` | media stream | AggregatingMergeTree (MV) | month | `(call_id, ssrc)` | 365 d | min/avg MOS, loss, jitter, RTT states |
| `logs` | log line | MergeTree | day | `(call_id, ts)` | 14 d | token index on message |
| `sip_counters_1m` | minute × node × method × status × direction | SummingMergeTree (MV) | month | all dimensions | 90 d | message-rate metrics |
| `kpi_calls_1m` / `kpi_calls_1h` | bucket × dimensions | ReplacingMergeTree(`version`) with aggregate-state columns | month | `(bucket, dims…)` | 90 d / 730 d | written per settled bucket by worker |
| `kpi_reg_1m` / `kpi_reg_1h` | bucket × dimensions | as above | month | `(bucket, dims…)` | 90 d / 730 d | |
| `kpi_qos_1m` / `kpi_qos_1h` | bucket × dimensions | as above | month | `(bucket, dims…)` | 90 d / 730 d | |
| `kpi_concurrency_1m` | minute × dimensions | ReplacingMergeTree | month | `(bucket, dims…)` | 365 d | FR-CR-11 |
| `hep_other` | unsupported HEP packet | MergeTree | day | `(proto_type, ts)` | 3 d | optional (FR-IN-13) |

Dictionaries (refreshed from PostgreSQL): `aliases_dict` (IP/CIDR → name), `topology_dict` (CIDR → group, role, carrier), `agents_dict` (agent id → name, host).

Public views for the SQL workbench and Grafana (stable, documented): `v_calls`, `v_legs`, `v_messages`, `v_registrations`, `v_registration_state`, `v_qos_streams`, `v_qos_reports`, `v_logs`, `v_findings`, `v_kpi_calls_1m`, `v_kpi_calls_1h`, `v_kpi_reg_1m`, `v_kpi_qos_1m`. Views resolve aliases through the dictionaries and hide internal columns.

### 7.2 Reference DDL (indicative — final DDL is generated by `vpd migrate` per topology mode)

```sql
CREATE TABLE vp.sip_messages
(
    ts            DateTime64(6, 'UTC')            CODEC(Delta, ZSTD(1)),
    ingest_ts     DateTime64(3, 'UTC')            CODEC(Delta, ZSTD(1)),
    import_id     LowCardinality(String),
    kind          Enum8('dialog' = 1, 'other' = 2),
    agent_id      UInt32,
    capture_point LowCardinality(String),
    src_ip        IPv6,
    src_port      UInt16,
    dst_ip        IPv6,
    dst_port      UInt16,
    transport     Enum8('udp' = 1, 'tcp' = 2, 'tls' = 3, 'ws' = 4, 'wss' = 5, 'sctp' = 6),
    src_group     LowCardinality(String),
    dst_group     LowCardinality(String),
    call_id       String,
    is_request    Bool,
    method        LowCardinality(String),
    status        UInt16,
    reason        LowCardinality(String),
    cseq_num      UInt32,
    cseq_method   LowCardinality(String),
    from_user     String,
    from_user_n   String,
    from_domain   LowCardinality(String),
    from_tag      String,
    to_user       String,
    to_user_n     String,
    to_domain     LowCardinality(String),
    to_tag        String,
    ruri_user     String,
    ruri_user_n   String,
    pai_user_n    String,
    user_agent    LowCardinality(String),
    via_branch    String,
    has_sdp       Bool,
    sdp_media     Array(Tuple(media LowCardinality(String), ip IPv6, port UInt16,
                              codecs Array(LowCardinality(String)), dir LowCardinality(String))),
    q850_cause    UInt8,
    hdr           Map(LowCardinality(String), String),
    hep_cid       String,
    src_country   LowCardinality(String),
    dst_country   LowCardinality(String),
    parse_error   Bool,
    payload       String                           CODEC(ZSTD(3)),

    INDEX idx_from  from_user_n TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_to    to_user_n   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_ruri  ruri_user_n TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_pl    payload     TYPE tokenbf_v1(32768, 3, 0) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toYYYYMMDD(ts)
ORDER BY (call_id, ts)
TTL toDateTime(ts) + INTERVAL 7 DAY TO VOLUME 'cold',
    toDateTime(ts) + INTERVAL 7 DAY DELETE WHERE kind = 'other',
    toDateTime(ts) + INTERVAL 30 DAY DELETE
SETTINGS storage_policy = 'hot_cold';
-- Without a cold volume, migrate omits the TO VOLUME rule and the storage_policy setting.
-- The token index may be replaced by ClickHouse's native full-text index where it is GA
-- in the deployed version.

CREATE TABLE vp.calls
(
    call_uid        UUID,
    version         UInt64,
    import_id       LowCardinality(String),
    state           Enum8('setup' = 1, 'ringing' = 2, 'answered' = 3, 'completed' = 4,
                          'failed' = 5, 'cancelled' = 6, 'timeout' = 7),
    start_ts        DateTime64(6, 'UTC'),
    ring_ts         Nullable(DateTime64(6, 'UTC')),
    answer_ts       Nullable(DateTime64(6, 'UTC')),
    end_ts          Nullable(DateTime64(6, 'UTC')),
    pdd_ms          Nullable(UInt32),
    setup_ms        Nullable(UInt32),
    talk_ms         UInt64,
    egress_pdd_ms   Nullable(UInt32),
    caller          String,
    caller_n        String,
    callee          String,
    callee_n        String,
    direction       LowCardinality(String),
    ingress_group   LowCardinality(String),
    egress_group    LowCardinality(String),
    trunk           LowCardinality(String),
    final_status    UInt16,
    final_reason    LowCardinality(String),
    egress_status   UInt16,
    q850_cause      UInt8,
    end_cause       LowCardinality(String),
    hangup_side     Enum8('unknown' = 0, 'caller' = 1, 'callee' = 2, 'network' = 3),
    root_call_id    String,
    legs            Array(Tuple(call_id String, role LowCardinality(String),
                                grp LowCardinality(String), link LowCardinality(String))),
    capture_points  Array(LowCardinality(String)),
    codecs          Array(LowCardinality(String)),
    ua_caller       LowCardinality(String),
    ua_callee       LowCardinality(String),
    caller_ip       IPv6,
    callee_ip       IPv6,
    caller_country  LowCardinality(String),
    min_mos         Nullable(Float32),
    max_loss_pct    Nullable(Float32),
    max_jitter_ms   Nullable(Float32),
    max_rtt_ms      Nullable(Float32),
    media_a_to_b    Bool,
    media_b_to_a    Bool,
    findings        Array(LowCardinality(String)),
    msg_count       UInt32
)
ENGINE = ReplacingMergeTree(version)
PARTITION BY toYYYYMM(start_ts)
ORDER BY (toDate(start_ts), call_uid)
TTL toDateTime(start_ts) + INTERVAL 365 DAY DELETE;
-- Readers use FINAL (or argMax by version); call_uid is UUIDv5(root Call-ID) so it is stable.
```

### 7.3 PostgreSQL entities (application state)

| Entity | Key fields | Notes |
|---|---|---|
| user | id, username, email, display_name, kind (local/ldap/radius/oidc/service), role_id, status, mfa_enabled, locale, tz, last_login_at, disabled_at | |
| identity | id, user_id, source_id, external_id (issuer+subject / DN / RADIUS user), last_seen_at | |
| credential | user_id, password_hash, totp_secret_enc, recovery_codes_hash | local users |
| auth_source | id, type (ldap/radius/oidc), name, config_enc, group_role_map, order, enabled | |
| role / permission | id, name, permissions[] | built-in + custom |
| session | id, user_id, created_at, last_seen_at, expires_at, ip, user_agent | |
| api_token | id, user_id, prefix, secret_hash, name, scope, expires_at, last_used_at, last_used_ip, revoked_at | |
| data_scope (P2) | id, name, agents[], groups[], trunks[], prefixes[], domains[] | → ClickHouse row policies |
| agent | id (HEP id), name, host, key_hash, allowed_cidrs[], status (active/quarantined/disabled), time_source, merge_into, first_seen, last_seen | |
| topology_group | id, name, role, carrier, members[] (ip/cidr/port), priority | → `topology_dict` |
| alias | id, match (ip/cidr/ip:port), name | → `aliases_dict` |
| correlation_rule | id, order, source, header, param, transform, key_type, enabled | §8.2 |
| normalization_rule | id, order, pattern, replacement, scope (all/caller/callee), enabled | |
| custom_header | name, searchable, low_cardinality | |
| ua_rule | id, pattern, vendor, model, version_group | |
| ingest_filter | id, order, match, action, sample_n, dry_run, enabled | |
| diagnostic_rule | code, built_in, enabled, thresholds, severity, custom_condition | |
| metric | id, name, formulas, unit, type, dims[], thresholds, built_in, owner_id | catalogue |
| dashboard / dashboard_version | id, owner_id, folder, visibility, body (JSON), version, created_at | |
| saved_search / saved_query | id, owner_id, visibility, name, body | |
| alert_rule / alert_instance / silence / notification_channel | rule definition, labels, state, timestamps, channel config_enc | |
| annotation | id, ts, ts_end, text, tags, source | |
| share_link | id, call_uid, token_hash, created_by, expires_at, password_hash, options, revoked_at, access_count | |
| export_job / import_job | id, owner_id, kind, params, state, progress, file_ref, expires_at | |
| retention_policy | data_type, retention_days, hot_days, updated_by, applied_at | |
| setting | key, value, updated_by, updated_at | |
| audit_event / audit_outbox | as DNS panel FR-AU-02; outbox per sink | monthly partitions, append-only |
| job / lease | id, kind, run_at, payload, state, attempts, locked_by, locked_until | leader election + scheduler |

### 7.4 Retention defaults (to confirm, §14 Q3)

| Data type | Table(s) | Retention | Hot (if cold volume) |
|---|---|---|---|
| Call SIP messages | `sip_messages` (`kind='dialog'`) | 30 d | 7 d |
| Other SIP (OPTIONS, NOTIFY, …) | `sip_messages` (`kind='other'`) | 7 d | 7 d |
| REGISTER messages | `reg_messages` | 7 d | 7 d |
| Registration events | `registration_events` | 30 d | 30 d |
| Raw media reports | `qos_reports` | 7 d | 7 d |
| Logs | `logs` | 14 d | 14 d |
| Call records, findings, lookup index, stream summaries | `calls`, `call_findings`, `lookup_index`, `stream_qos` | 365 d | 30 d |
| 1-minute rollups | `kpi_*_1m`, `sip_counters_1m` | 90 d | 90 d |
| 1-hour rollups | `kpi_*_1h` | 730 d | 730 d |
| Imported PCAPs | all tables (`import_id ≠ ''`) | 30 d | — |
| Audit (PostgreSQL) | `audit_event` | 365 d | — |

---

## 8. Interfaces

### 8.1 HEP
Supported HEPv3 chunks (vendor `0x0000`):

| Chunk | Meaning | Use |
|---|---|---|
| `0x0001` / `0x0002` | IP family / IP protocol | transport |
| `0x0003`–`0x0006` | IPv4/IPv6 source and destination | addresses |
| `0x0007` / `0x0008` | source / destination port | ports |
| `0x0009` / `0x000a` | timestamp seconds / microseconds | capture time |
| `0x000b` | protocol type | routing (below) |
| `0x000c` | capture agent id | agent registry |
| `0x000e` | authentication key | agent auth |
| `0x000f` | payload | message |
| `0x0010` | compressed payload (gzip/zlib) | decompressed |
| `0x0011` | correlation id | correlation key / log & QoS linking |
| `0x0012` | VLAN id | stored |
| `0x0013` | capture agent name | agent registry |
| other | — | skipped, counted |

Protocol types: `1` SIP → `sip_messages`/`reg_messages`; `5` RTCP JSON → `qos_reports`; `34` agent RTP statistics → `qos_reports`; `35` RTCP-XR → `qos_reports`; `100` log → `logs`; `53` DNS and `56` Diameter → `hep_other` until P3; anything else → counted, optionally `hep_other`.

Default ports (all configurable): HEP UDP/TCP `9060`, HEP TLS `9061`, HEP over HTTP(S) `9080`, VQ-RTCPXR SIP `5063`, UI/API HTTPS `8443` (or plain `8080` behind HAProxy), `/metrics` on the API port and on a separate port per collector (`9100`).

### 8.2 Correlation rules (example)

```yaml
correlation:
  rules:                         # evaluated in order; every match adds a key
    - name: hep-correlation-chunk   # HEP 0x0011 and OpenSIPS correlate()
      source: hep_cid
      key: cid
    - name: asterisk-b-leg
      source: header
      header: X-CID
      methods: [INVITE]
      initial_only: true
      key: cid
    - name: ims-charging-vector
      source: header_param
      header: P-Charging-Vector
      param: icid-value
      key: icid
    - name: rfc7989-session-id
      source: header
      header: Session-ID
      transform: uuid_pair_sorted   # local + remote UUID, order-independent
      key: sid
    - name: transfer
      source: header_param
      header: Replaces
      param: call-id
      key: related                  # related call, not merged
  heuristic:
    enabled: false
    pbx_groups: ["Asterisk pool"]
    window_ms: 2000
  limits:
    max_legs: 64
    max_depth: 8
```

### 8.3 Integration kit (indicative — generated by first-run setup; validated against production versions, §14 Q12)

**OpenSIPS** (proxy, registrar, load balancer) — HEP over TLS to the collector, dialog-scoped tracing:
```
loadmodule "proto_hep.so"
modparam("proto_hep", "hep_id", "[vp] 10.10.0.20:9061; transport=tls; version=3")
modparam("proto_hep", "hep_capture_id", 101)      # unique per OpenSIPS node
# transport=tls requires tls_mgm with a client domain for 10.10.0.20:9061

loadmodule "siptrace.so"
modparam("siptrace", "trace_id", "[vptrace]uri=hep:vp")

route {
    if (!has_totag()) {
        if (is_method("INVITE"))
            sip_trace("vptrace", "d");   # whole dialog (dialog module loaded)
        else
            sip_trace("vptrace", "t");   # transaction (REGISTER, OPTIONS, ...)
    }
    # ...
    # Requests coming back from Asterisk carry X-CID (A-leg Call-ID); it is captured
    # on ingress, then removed before relaying to phones or carriers:
    #   remove_hf("X-CID");
}
```
If topology hiding with Call-ID masking is used, emit both Call-IDs with `correlate()` so the legs can be linked.

**Asterisk** (B2BUA) — HEP for SIP and RTCP, and the A-leg Call-ID carried to the B-leg:
```
; modules.conf
load = res_hep.so
load = res_hep_pjsip.so
load = res_hep_rtcp.so

; hep.conf
[general]
enabled = yes
capture_address = 10.10.0.20:9060
capture_password = <agent key>
capture_id = 201
uuid_type = call-id

; extensions.conf
[from-opensips]
exten => _X.,1,Set(XCID=${PJSIP_HEADER(read,X-CID)})
 same => n,Set(XCID=${IF($["${XCID}" = ""]?${CHANNEL(pjsip,call-id)}:${XCID})})
 same => n,Dial(PJSIP/${EXTEN}@opensips,,b(vp-hdr^s^1(${XCID})))

[vp-hdr]
exten => s,1,Set(PJSIP_HEADER(add,X-CID)=${ARG1})
 same => n,Return()
```
Asterisk sends HEP over UDP only; keep it on the management network.

**rtpengine** — RTCP statistics per call via HEP:
```
[rtpengine]
homer = 10.10.0.20:9060
homer-protocol = udp
homer-id = 301
```

**Phones** — point the VQ-RTCPXR collector (Yealink/Grandstream "voice quality report" settings) at `sip:vq@<collector>:5063`.

**Other hosts** (carrier SBC mirror port, legacy PBX) — heplify:
```
heplify -i eth1 -m SIPRTCP -hs 10.10.0.20:9060 -nt tcp -hi 401
```

### 8.4 Prometheus metrics (selection)

| Metric | Labels |
|---|---|
| `vp_hep_packets_received_total` | agent, proto |
| `vp_hep_packets_dropped_total` | reason (`overload`, `bad_packet`, `auth`, `filter`, `duplicate`, `spool_full`) |
| `vp_sip_messages_total` | node, method, status_class |
| `vp_ingest_queue_bytes` | stage |
| `vp_clickhouse_insert_seconds` / `vp_clickhouse_insert_errors_total` | table |
| `vp_spool_bytes` / `vp_spool_oldest_seconds` | — |
| `vp_agent_clock_offset_seconds` | agent |
| `vp_finalizer_lag_seconds` / `vp_finalizer_dialogs_total` | — |
| `vp_calls_active` | direction, trunk |
| `vp_registrations_active` | group |
| `vp_query_seconds` | class |
| `vp_kpi_*` (from `/metrics/kpi`) | catalogue dimensions |

---

## 9. Key workflows

**9.1 Ingest (packet → searchable)**
1. An agent sends HEP; the collector checks the agent key and source CIDR, decodes HEP, decompresses if needed, parses SIP/SDP (or the QoS/log payload).
2. Enrichment: number normalization, topology groups, GeoIP/ASN, user-agent parsing; correlation rules add keys.
3. Filters apply (drop/sample/metadata-only); duplicates are suppressed; the event is published to the live feed.
4. Rows are batched per table and inserted (≤ 1 s); `call_links` and `registration_events` are written alongside. ClickHouse materialized views update `dialogs`, `stream_qos`, `registration_state` and `sip_counters_1m`.
5. If ClickHouse is unavailable, batches go to the spool and are replayed in order later.

**9.2 Call assembly**
1. Every 5 s the finalizer reads dialogs changed since its watermark.
2. For each, it expands the key group through `call_links` (bounded), picks the root and egress legs, computes timings, states, end cause, direction, trunk, QoS summary and findings.
3. It upserts `calls` (new version), `call_findings` and `lookup_index`; closed calls feed the per-minute occupancy table.
4. Late messages make the dialog "changed" again, so the call is re-assembled with a higher version.

**9.3 Customer complaint (Support)**
1. Support enters the customer's number on the Subscribers page (any format).
2. The page shows "not registered — last attempt 401 loop since 09:12, phone Yealink T31P, IP 5.x.x.x".
3. Opening the latest attempt shows the REGISTER flow and the `AUTH_LOOP` finding with explanation (wrong password in provisioning); Support escalates with the share link.

**9.4 Failed call (Engineer)**
1. Paste the number or Call-ID into the universal search → the call appears (all legs).
2. The Flow tab shows A-leg via OpenSIPS to Asterisk and B-leg to Carrier-X; the carrier answered 503 with Q.850 cause 34; finding `CARRIER_REJECT`.
3. Hop diff shows OpenSIPS rewrote the R-URI as expected; the Engineer exports PCAP (both legs) or creates a share link for the carrier.

**9.5 Dashboard drill-down**
1. NOC overview shows ASR for Carrier-X falling at 14:05.
2. Clicking the 14:05 bucket opens the call search for Carrier-X in that minute; the facet panel shows 90 % `503 / cause 34`.
3. One click opens a sample call; an alert rule is created from the panel ("ASR < 40 % for 5 min, ≥ 50 attempts").

**9.6 Alert**
Rule pending → firing → notification (email/Teams/Telegram) with dashboard and search links → acknowledge with comment → optional silence → resolved notification; all steps annotated on charts and audited.

**9.7 Retention change**
Admin sets "Call SIP messages: 45 days" → the worker runs `ALTER TABLE … MODIFY TTL` (on every replica) → the Retention page shows the new projection → the change is audited and sent to QRadar.

**9.8 New agent**
Admin creates an agent (id, name, allowed CIDR) → the Panel shows the key once and a generated config snippet → the agent connects → it appears as *active* (or *quarantined* by policy until approved) → statistics start.

**9.9 PCAP import**
Engineer uploads a PCAP from a customer site → the import job parses it into the tables with an `import_id` → calls appear under Imports with full call view and diagnostics → automatically deleted after 30 days.

**9.10 ClickHouse outage**
Inserts fail → collectors spool to disk and raise "spool growing" → search shows "data delayed" → on recovery spooled batches replay in order (deduplication tokens prevent double inserts) → finalizer catches up → lag returns to normal.

---

## 10. Phasing

| Area | Phase 1 (MVP) | Phase 2 | Phase 3 |
|---|---|---|---|
| Ingest | HEP v1–v3 UDP/TCP/TLS/HTTP, agent keys, compression, filters, dedup, spool, VQ-RTCPXR listener | Kafka in/out, HEP forwarding, SIPREC SRS, syslog/OTLP logs | — |
| Decoding | SIP (incl. WS/WSS), SDP, RTCP, RTCP-XR, agent stats, HEP logs | SIP-I/ISUP, tshark decode | DNS, Diameter, WebRTC JSON signalling |
| Correlation | Header/HEP-chunk/ICID/Session-ID rules, links, finalizer, heuristic option, rebuild | Lua hooks, CDR/CEL ingest | — |
| Registrations | Events, state, KPIs, timeline, storage modes | — | — |
| Media | QoS reports, stream summaries, E-model estimate, media findings | voip-panel agent RTP statistics | RTP capture and playback |
| Search & views | Universal search, filter syntax, forms, call view, subscriber view, diagnostics | Call comparison, HTML report | — |
| Metrics & charts | Catalogue, time engine, all panel types, rollups | Anomaly baselines | — |
| Dashboards | Built-in set, user dashboards, Explore, TV mode | Snapshots, Grafana embed | — |
| SQL | Workbench with views, library, RBAC limits | Row policies for scopes | Natural-language assistant |
| Live view | Yes | — | — |
| Export/import | PCAP, text, JSON, share links, PCAP import, exclusion list | pcapng with comments | — |
| Alerting | Threshold, absence, change, finding, health rules; email, webhook, Teams/Slack, Telegram, Alertmanager | Anomaly rules, inbound alert webhook | — |
| Identity & access | Local + TOTP, LDAP/AD, RADIUS, OIDC, tokens, roles, Support masking | Data scopes, WebAuthn | External tenants |
| Audit | Local store, review UI, QRadar | — | — |
| API & CLI | REST v1 + OpenAPI, `vpctl` | MCP server | — |
| UI | English, light/dark | Arabic RTL | — |
| Agent | (use OpenSIPS/Asterisk/rtpengine/heplify) | `vp-agent` for Linux | Media capture |
| Platform | All-in-one, RPM, container, Compose, Quadlet, Helm; single-node and replicated ClickHouse | Sharded ClickHouse | — |

---

## 11. HOMER feature coverage

Every HOMER feature, from the HOMER feature list and the HOMER 11 code base, and where it lands.

| HOMER feature | voip-panel | Where / phase |
|---|---|---|
| Thousands of packets/s | ≥ 100,000 HEP packets/s per collector, horizontal scaling | NFR-03 · P1 |
| Multiple database options | **Changed:** ClickHouse only, deliberately | AD-1 |
| Powerful UI search/filter | Universal search, filter syntax, forms, facets | FR-SR · P1 |
| Native call-flow & PCAP | Multi-leg ladder, PCAP export/import | FR-CV-04, FR-EX · P1 |
| Statistic charts & analytics | Metric catalogue, time engine, dashboards | FR-KP, FR-CH, FR-DB · P1 |
| REST API & widgets | REST v1 + OpenAPI; dashboard panels | FR-API, FR-DB · P1 |
| Multi-user, LDAP, RADIUS | Local + TOTP, LDAP/AD, RADIUS, OIDC | FR-ID · P1 |
| Angular/JS user interface | **Changed:** React + TypeScript | AD-9 |
| HEP/EEP protocol | HEP v1–v3, all transports | FR-IN, §8.1 · P1 |
| SIP signalling | Full SIP/SDP decode | FR-DE · P1 |
| RTCP / RTCP-XR | RTCP, RTCP-XR, VQ-RTCPXR PUBLISH | FR-QS, FR-IN-12 · P1 |
| WebRTC signalling | SIP over WS/WSS · JSON signalling | FR-DE-07 · P1; P3 |
| Log collector | HEP logs · syslog/OTLP logs | FR-LG · P1; P2 |
| Geo mapping | GeoIP/ASN enrichment, geo map panel | FR-TP-05, FR-CH-18 · P1 |
| Docker support | Container, Compose, Quadlet, Helm, RPM | NFR-10 · P1 |
| Agent: HEP3, payload encryption (TLS), compression | Collector accepts all; own agent | FR-IN · P1; FR-AG · P2 |
| Agent: SIP, RTP/RTCP, logs, CDRs | Agent stats · CDR/CEL ingest | FR-AG · P2; FR-CR-15 · P2 |
| Agent: Linux/Solaris/BSD/OSX/Win32 | **Changed:** Linux only | §1.3 |
| HOMER 11 roles (writer, reader, coordinator, compactor) | Collector, API, worker; compaction is ClickHouse's own | AD-2 |
| DuckLake/Parquet on S3/Azure, tiering | ClickHouse hot/cold storage policy on S3/MinIO (Azure Blob disk also possible) | AD-11 · P1 |
| Smart routing across nodes | ClickHouse Distributed tables | AD-16 · P2 |
| Time-travel queries | **Dropped** (no operational need) | — |
| VQRTCP collector | VQ-RTCPXR listener | FR-IN-12 · P1 |
| SIPREC SRS | SIPREC SRS | FR-INT-06 · P2 |
| OTLP receiver | Logs only; traces/metrics belong in an observability stack | FR-LG-04 · P2 |
| InfluxDB line-protocol receiver | **Dropped** (use Prometheus/VictoriaMetrics) | — |
| FlightSQL for Grafana | Grafana ClickHouse plugin on public views | FR-INT-02 · P1 |
| MCP and natural-language search | MCP server · NL→SQL assistant | FR-API-07 · P2; FR-SQ-08 · P3 |
| Live HEP stream | Live view | FR-LV · P1 |
| Lua correlation scripts | Declarative rules · Lua hooks | FR-CR-02 · P1; FR-CR-14 · P2 |
| Search CLI (table, call flow, PCAP) and SQL shell | `vpctl search`, `vpctl call`, `vpctl sql` | FR-API-06 · P1 |
| Config wizard | First-run setup + `vpctl config init` | FR-AD-01 · P1 |
| Catalog backup/restore, rebuild | **Not needed** (no catalog); ClickHouse backups | NFR-05 |
| Retention (several knobs) | One value per data type in the UI | FR-AD-06 · P1 |
| Dashboards with widgets | Dashboards, Explore, TV mode | FR-DB · P1 |
| Alert store with drill-down to search | Built-in alerting with drill-down · inbound alerts | FR-AL · P1; FR-AL-07 · P2 |
| IP aliases, group by alias | Aliases, topology groups, column grouping | FR-TP, FR-CV-04 · P1 |
| Mappings, fields, virtual fields, custom headers, A-leg id headers | Custom headers as fields; correlation rules | FR-DE-03, §8.2 · P1 |
| HEP/agent subscriptions | Central agent configuration | FR-AG-04 · P2 |
| PCAP import | PCAP import | FR-EX-06 · P1 |
| PCAP/text export, export exclusion, correlated B2BUA legs | Same | FR-EX-01–03 · P1 |
| Deep links and share | Deep links, expiring share links | FR-SR-06, FR-EX-05 · P1 |
| Call info (SRD, setup delay, durations) | Stored call record timings | FR-CR-07 · P1 |
| QoS tab (RTCP, RTP, VQRTCP), RTCP inline on ladder | Media tab, inline rungs | FR-CV-04, FR-CV-07 · P1 |
| Log events on the ladder | Inline log rungs, Logs tab | FR-CV-04, FR-CV-08 · P1 |
| Capture-ID consolidation | Capture-point merge | FR-TP-07 · P1 |
| Per-user locale/time zone | User preferences | FR-UI-03 · P1 |
| Internal/LDAP/OAuth2 auth, API tokens | Same plus RADIUS and TOTP | FR-ID · P1 |
| tshark decode | Optional tshark decode | FR-CV-15 · P2 |
| SIP-I/ISUP decode | Same | FR-DE-10 · P2 |
| Prometheus SIP/RTCP metrics | Ingest metrics + KPI exporter | FR-INT-01, FR-KP-08 · P1 |
| Dedup, discard filters (heplify-server) | Dedup and ingest filters | FR-IN-09, FR-IN-10 · P1 |
| HEP forwarding (heplify-server) | HEP forwarding | FR-INT-05 · P2 |
| Loki log integration (HOMER 7) | Logs in ClickHouse | FR-LG · P1 |
| Game widgets (Doom, chess, NetChess, Netris, SIPetris, Packet Defender) | **Dropped** | — |
| Client/server reset panels | **Replaced** by settings export/import | FR-AD-09 |

---

## 12. Decision log (2026-11-08 discussion)

| # | Decision | Notes / interpretation to confirm |
|---|---|---|
| 1 | ClickHouse is the storage for captured data. | PostgreSQL added for application state (AD-1). |
| 2 | Rust for the core. | One binary with roles (AD-2). |
| 3 | Frontend choice delegated. | React + TypeScript + Vite + Apache ECharts (AD-9). Confirm. |
| 4 | Charts and everything about chart timing must be fixed. | FR-CH, AD-7. |
| 5 | SQL is too hard and data too hard to find — must be fixed. | Universal search + lookup index, metric catalogue, SQL workbench with documented views (AD-5, AD-6, FR-SQ). |
| 6 | Built-in dashboards for the essentials. | FR-DB-03 (nine dashboards). |
| 7 | Track every call from beginning to end, A to Z, as HOMER does. | Correlated multi-leg calls, call view, subscriber view, diagnostics. |
| 8 | All HOMER features. | §11; deliberate drops (games, time travel, line protocol, non-Linux agents, multi-DB) to confirm. |
| 9 | Easy implementation. | All-in-one deployment, generated OpenSIPS/Asterisk/rtpengine configs, no new agent in phase 1. |

Defaults applied where the storage discussion is not yet closed: no Kafka in phase 1; retention per §7.4; registration storage mode `all`; no RTP media capture; replicated ClickHouse in production (single node acceptable for the 10k-user phase); Earthlink staff only (no tenants). See §14.

---

## 13. Default parameters

| Parameter | Default | Scope |
|---|---|---|
| Batch flush | 50,000 rows or 1,000 ms | collector |
| Spool cap | 20 GB | collector |
| Duplicate window | 2 s | collector |
| Agent clock-offset warning | 1 s | per agent |
| Decompressed payload cap | 256 KB | collector |
| Finalizer interval | 5 s | worker |
| Call close grace / max call age | 30 s / 6 h | worker |
| Correlation limits | 64 legs, depth 8 | worker |
| Settle window | 5 min | platform |
| Source switch (raw → 1 m → 1 h) | ≤ 6 h raw; ≤ 90 d 1-minute | platform |
| Chart max points | min(width ÷ 3, 1,500) | UI |
| Query timeouts | interactive 30 s; workbench 120 s; export 10 min | per class |
| Workbench limits | 8 GB memory, 100,000 result rows | per role |
| Live view | 500 events/s per user; 20 sessions; 30 min idle stop | platform |
| Share link expiry | 7 days (max 30) | platform |
| Bulk PCAP export | 100 calls | Engineer |
| PCAP import | 500 MB; kept 30 days | Engineer |
| Search result export | 100,000 rows | Engineer |
| Session idle / absolute | 30 min / 12 h | platform |
| API token expiry | 90 days default, 365 max | platform |
| Local-user inactivity disable | 90 days (warning at 83) | platform |
| PDD warning | 5 s | diagnostics |
| MOS bands | ≥ 4.0 good; 3.6–4.0 fair; 3.1–3.6 poor; < 3.1 bad | diagnostics |
| Alert evaluation | every 30 s | worker |
| Retention | §7.4 | per data type |
| Audit local retention | 365 days | platform |
| QRadar transport | LEEF 2.0, TLS syslog, port 6514 | platform |
| Platform time zone | Asia/Baghdad | platform / per user |

---

## 14. Open questions

1. **Kafka:** none in phase 1 (default), or put the existing Kafka cluster in front of ClickHouse from day one?
2. **ClickHouse placement:** dedicated cluster for voip-panel (recommended — different retention and load profile) or the existing ClickHouse that holds the HAProxy logs?
3. **Retention:** confirm §7.4; is there a regulatory retention requirement for call records or signalling?
4. **Registrations:** store every REGISTER (`all`, default) or `changes` only (saves ~40 % of SIP volume at 2M users)?
5. **RTP media:** is recording/playback of audio needed, and in which phase?
6. **HA and platform:** single ClickHouse node or replicated in phase 1; run on Kubernetes (ArgoCD) or VMs/Podman?
7. **Tenancy:** Earthlink staff only, or should carriers/enterprise customers get restricted access (data scopes P2, tenants P3)?
8. **Identity sources:** which are needed at launch — AD/LDAP, RADIUS (which server), Keycloak OIDC — and the group → role mapping.
9. **Arabic UI:** needed? Phase 2 is assumed.
10. **QRadar:** same endpoint, LEEF identifiers and event-id conventions as the DNS panel?
11. **Numbering plan:** number formats seen from phones and carriers; normalization rules for Iraqi mobile, fixed and international numbers.
12. **Versions and topology:** OpenSIPS, Asterisk and rtpengine versions in production; is OpenSIPS topology hiding used; how many OpenSIPS/Asterisk nodes; which carriers and their SBC addresses.
13. **Phones:** models and firmware; can the provisioning template enable VQ-RTCPXR reports to the Panel?
14. **Support role:** masking rule (last 4 digits?) and whether Support may see SIP payloads.
15. **Licence** for voip-panel itself (internal/proprietary or open source).
16. **Sizing assumptions** (Appendix A): average call duration, 5 % busy-hour concurrency, REGISTER refresh interval, messages per call.
17. **Alert channels** to support at launch (email, Teams, Telegram, Slack, Alertmanager).
18. **Subscriber addressing:** do FTTH phones reach OpenSIPS from public IPs, CGNAT or a private service VLAN? Drives topology groups and NAT findings.

---

## Appendix A — Sizing estimate

**Assumptions** (to confirm, §14 Q16): all signalling over TLS, so capture comes from OpenSIPS, Asterisk and rtpengine via HEP; each message captured at every OpenSIPS hop on both legs (~40 messages per call including the auth challenge); 5 % busy-hour concurrency; 180 s average call; REGISTER refresh every 600 s (2 messages per refresh); ~1.2 KB per stored SIP message; ~10× compression for SIP and ~15× for media reports; average daily load 40 % of busy hour; media reports for 4 streams per call every 5 s.

| | 10k subscribers (phase 1) | 2M subscribers (design target) |
|---|---|---|
| Concurrent calls / CPS | 500 / 2.8 | 100,000 / 556 |
| SIP messages/s (busy hour) | ~150 | ~29,000 (≈ 6,700 REGISTER) |
| SIP per day | 7 M messages, ~1 GB stored | 1.3 B messages, 1.6 TB raw, ~160 GB stored |
| Media reports/s (busy hour) | 400 | 80,000 |
| Calls per day | ~0.1 M | ~19 M |

**Disk per ClickHouse replica at 2M subscribers with §7.4 retention (steady state, ±50 %):**

| Data | Estimate |
|---|---|
| `sip_messages` (30 d) | 2.75 TB |
| `reg_messages` (7 d) | 0.48 TB |
| `qos_reports` (7 d) | 0.52 TB |
| `calls` (365 d) | 1.40 TB |
| `lookup_index` (365 d) | 1.68 TB |
| `stream_qos` (365 d) | 1.40 TB |
| `registration_events` (30 d) | 0.52 TB |
| `logs` (14 d, assuming 20 GB/day) | 0.28 TB |
| **Total** | **~9 TB** (≈ 2.8 TB hot on NVMe, ≈ 6.2 TB cold on S3/MinIO) |

**Indicative hardware**
- **Phase 1 (10k):** one all-in-one VM — 8 vCPU, 32 GB RAM, 500 GB SSD — running `vpd --role all`, ClickHouse and PostgreSQL; a second VM for a replica when HA is required.
- **2M target:** 3 collectors (8 vCPU, 16 GB); 2 API and 2 worker instances (4 vCPU, 8 GB); ClickHouse 2 replicas (start with 1 shard, 32 vCPU, 128 GB RAM, 4 TB NVMe each; add a shard when CPU or hot disk passes 60 %) + 3 Keeper nodes (2 vCPU, 4 GB); S3/MinIO for cold data and backups; PostgreSQL primary + replica (4 vCPU, 16 GB).

---

## 15. References
- HOMER 11 source and docs (`sipcapture/homer`, branch `homer11`): README, `docs/STORAGE_ARCHITECTURE.md`, `docs/STORAGE_LAYOUT.md`, `docs/RETENTION.md`, `docs/OOM.md`, `docs/TROUBLESHOOTING.md`, `docs/SECURITY.md`, `docs/LUA_CORRELATION.md`, `docs/ALERTS.md`, `docs/HEP_STREAM.md` — https://github.com/sipcapture/homer
- HEP3 network protocol specification — https://github.com/sipcapture/HEP
- OpenSIPS `proto_hep` (hep_id, transport TLS, `correlate()`) — https://docs.opensips.org/manual/3-5/modules/proto_hep/
- RFC 3261 (SIP), RFC 4566/8866 (SDP), RFC 3550 (RTP/RTCP), RFC 3611 (RTCP-XR), RFC 6035 (SIP event package for voice quality reporting), RFC 6076 (basic telephony SIP end-to-end performance metrics), RFC 7118 (SIP over WebSocket), RFC 7865/7866 (SIPREC), RFC 7989 (Session-ID), RFC 2865/2866 (RADIUS), RFC 5425 (syslog over TLS), RFC 9457 (Problem Details)
- ITU-T G.107 (E-model), ITU-T Q.850 (cause values), ITU-T E.411 / E.425 (ASR/NER concepts)
- ClickHouse documentation — MergeTree TTL and storage policies, materialized views, skip indexes, settings profiles, quotas and row policies — https://clickhouse.com/docs
- Apache ECharts — https://echarts.apache.org
