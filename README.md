# voip-panel

SIP/VoIP troubleshooting and monitoring panel, built to replace HOMER 11.

**Status:** the application around the data is real: login, users, roles, dashboards, settings and MariaDB storage. The traffic it shows is generated demo data, because HEP capture isn't built yet.

## Run it

You need Docker (or Podman) and a MariaDB database.

### With Docker Compose (includes MariaDB)

```bash
cp .env.compose.example .env
```

Put your own passwords in `.env`, then:

```bash
docker compose up -d
```

### Against your own MariaDB

Only three settings are required. The database must already exist.

```bash
docker run -d --name voip-panel -p 8080:8080 \
  -e VP_DB_HOST=your-mariadb \
  -e VP_DB_USER=voip_panel \
  -e VP_DB_PASSWORD=your-password \
  ghcr.io/noorotk/voip-panel:latest
```

Open <http://localhost:8080>. On first start the login page asks you to create the administrator password, then takes you straight into the app. **Until that's done, anyone who reaches the port can claim the admin account**, so set `VP_SETUP_TOKEN` if the port is exposed, or `VP_ADMIN_PASSWORD` to skip the screen.

The container listens on plain HTTP. Put a TLS proxy in front of it for anything beyond a test.

## Configuration

Every setting has an environment variable, `VP_<NAME>`. All are optional except the three above.

- [`docs/ENVIRONMENT.md`](docs/ENVIRONMENT.md) lists them all.
- [`.env.example`](.env.example) is the same list, ready to copy.
- `docker run --rm ghcr.io/noorotk/voip-panel env` prints the reference from the image itself.

Application settings can also be changed under **Admin → Settings** and are saved in MariaDB. A value set in the environment takes priority and appears locked there. Invalid values stop the container at startup with a message saying which one is wrong.

Users, roles, sessions, dashboards, storages, settings, saved searches and the audit log all live in MariaDB.

## What you can do

**Dashboards.** Every dashboard is one JSON document, shipped from [`crates/vpd/data/dashboards`](crates/vpd/data/dashboards). Paste JSON to import a whole dashboard, or copy it out. In edit mode you can drag and resize panels, add, duplicate or delete them, change the visualization, and edit each panel's query with the builder or as SQL. Version history and restore are built in.

**Capture IDs.** Calls carry the HEP capture ID of every agent that saw them. Filter with `capture:201` or `capture:201,211`, group charts by capture ID or backend type, and use the Capture ID filter at the top of each dashboard. The demo traffic has both Asterisk and FreeSWITCH nodes.

**Search.** The filter panel, the search box and the URL all use the same syntax:

| Example | Meaning |
|---|---|
| `status:503,486` or `status:503 status:486` | either code |
| `-trunk:Mobile-A` | not this trunk |
| `capture:201 pdd>3s mos<3.5` | all must hold |
| `*2345`, `96477*`, `07700012345` | suffix, prefix, any number format |

Columns, density, page size and the filter lists are saved per user, and searches can be saved and shared.

**Permissions.** Admin → Roles & permissions is a grid of permissions by role. Each user can also have individual allow or deny overrides. Everything is enforced by the server. The Admin role always has every permission and the last administrator can't be removed, so nobody can lock themselves out.

**Live capture** is off by default and costs nothing until someone turns it on and presses Start.

**Storage.** Admin → Storage holds the storage connections and which one is in use.

## Not built yet

- HEP ingest and everything behind it. **The app writes nothing to ClickHouse.** Selecting a ClickHouse storage points SQL panels and the SQL workbench at it, and all other pages keep showing demo data.
- On demo data, SQL runs on a built-in engine for a ClickHouse-compatible subset: one table per query, no joins or subqueries.
- LDAP, RADIUS, OIDC and TOTP sign-in. Only local accounts exist.
- Alert rules are sample content. Acknowledging an alert does work and is saved.
- Agents, topology, rules, retention and system health are sample content. PCAP import and share links aren't built.
- `linux/amd64` images only.

## Development

The code is a Rust daemon (`crates/vpd`, axum) that embeds a React and TypeScript UI (`web`). Run MariaDB locally, then in two terminals:

```bash
VP_LISTEN=127.0.0.1:8080 VP_DB_HOST=127.0.0.1 VP_DB_USER=voip_panel VP_DB_PASSWORD=your-password cargo run -p vpd
```

```bash
npm --prefix web run dev
```

Vite proxies `/api` to port 8080. After changing settings, regenerate the docs:

```bash
cargo run -q -p vpd -- env > docs/ENVIRONMENT.md
```

```bash
cargo run -q -p vpd -- env --dotenv > .env.example
```

The image is built and pushed to `ghcr.io/noorotk/voip-panel` by GitHub Actions on every push to `main`.
