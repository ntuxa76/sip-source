# sip-source

SIP capture and troubleshooting panel, built to replace HOMER 11.

SIP servers send it HEP. It stores the traffic, joins the legs into calls, and gives you search, call flows, registrations, media quality and dashboards. Captured data goes to the built-in Local file storage, or to your own ClickHouse. Users, roles, dashboards and settings live in MariaDB.

## Run it

The image is `ntuxa76/sip-source:0.5` on Docker Hub. You need Docker (or Podman). MariaDB comes with the compose file, or you can use your own. A ClickHouse server is optional.

### With Docker Compose (runs the panel and MariaDB)

Copy [`docker-compose.yml`](docker-compose.yml) and [`.env.compose.example`](.env.compose.example) to the server, then:

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
docker run -d --name sip-source -p 8080:8080 -p 9060:9060/udp -p 9060:9060/tcp \
  -v sip-source-data:/data \
  -e VP_DB_HOST=your-mariadb \
  -e VP_DB_USER=voip_panel \
  -e VP_DB_PASSWORD=your-password \
  ntuxa76/sip-source:0.5
```

Open <http://localhost:8080>. On first start the login page asks you to create the administrator password, then takes you straight into the app. **Until that's done, anyone who reaches the port can claim the admin account**, so set `VP_SETUP_TOKEN` if the port is exposed, or `VP_ADMIN_PASSWORD` to skip the screen.

The container listens on plain HTTP. Put a TLS proxy in front of it for anything beyond a test.

### Storage

Captured data goes to the built-in **Local file** storage from the first start. It keeps everything under `/data` in the container, so keep that on a volume as above; the compose file does this already. Search, dashboards and SQL work the same on it as on ClickHouse, because it runs the same engine inside the container. It uses up to half the container's memory.

To use your own ClickHouse instead, go to **Admin → Storage**, press **Add storage**, choose **ClickHouse server**, enter its HTTP port (usually 8123), press Test, then **Use this storage**. The panel creates the database, tables and views, and stops the local engine. The ClickHouse user needs `CREATE`, `ALTER`, `INSERT` and `SELECT` on that database. Data already in the Local file storage stays there; it isn't copied.

### Point Asterisk at it

On each Asterisk, in `/etc/asterisk/hep.conf`:

```ini
[general]
enabled=yes
capture_address=PANEL_IP:9060
capture_id=2001
uuid_type=call-id
```

Use a different `capture_id` per server, and keep `uuid_type=call-id`, because RTCP reports are matched to calls by it. Then load the modules:

```bash
asterisk -rx "module load res_hep.so"
```

```bash
asterisk -rx "module load res_hep_pjsip.so"
```

```bash
asterisk -rx "module load res_hep_rtcp.so"
```

`res_hep_pjsip` only sees PJSIP. It doesn't capture `chan_sip`.

### Sources

Every server that sends HEP shows up under **Admin → Sources** by its address with the first packet. Give each one an alias there, such as `Asterisk-1` or `OpenSIPS-edge`. The source menu in the top bar then lets you look at one server at a time: dashboards, search and live capture all follow it. In search, `source:10.0.0.5` does the same.

If you set `VP_HEP_AUTH_KEY`, set the same value as `capture_password` in `hep.conf`.

**Joining legs.** A call through Asterisk has two SIP dialogs. To link them exactly, pass the A-leg's Call-ID to the outgoing INVITE in your dialplan:

```ini
[from-phones]
exten => _X.,1,Set(__ACALLID=${CHANNEL(pjsip,call-id)})
 same => n,Dial(PJSIP/${EXTEN}@carrier,20,b(add-cid^s^1))

[add-cid]
exten => s,1,Set(PJSIP_HEADER(add,X-CID)=${ACALLID})
 same => n,Return()
```

Without it, legs are matched by capture agent, timing and called number, and those calls are marked `inferred`.

**Direction and trunk.** Under **Admin → Settings → Topology**, list your carriers and subscriber networks as `Name=CIDR`. Until you do, direction shows as `unknown` and trunk charts are empty.

## Configuration

Every setting has an environment variable, `VP_<NAME>`. All are optional except the three database ones above. ClickHouse is the exception: it is always connected from **Admin → Storage**.

- [`docs/ENVIRONMENT.md`](docs/ENVIRONMENT.md) lists every variable with its default and what it does.
- [`.env.example`](.env.example) is the same list, ready to copy.
- `docker run --rm ntuxa76/sip-source:0.5 env` prints the reference from the image itself.

Application settings can also be changed under **Admin → Settings** and are saved in MariaDB. A value set in the environment takes priority and appears locked there. Invalid values stop the container at startup with a message saying which one is wrong.

Users, roles, sessions, dashboards, storages, settings, saved searches and the audit log all live in MariaDB.

## What you can do

**Dashboards.** Every dashboard is one JSON document, shipped from [`crates/vpd/data/dashboards`](crates/vpd/data/dashboards). Paste JSON to import a whole dashboard, or copy it out. In edit mode you can drag and resize panels, add, duplicate or delete them, change the visualization, and edit each panel's query with the builder or as SQL. Version history and restore are built in.

**Capture IDs.** Calls carry the HEP capture ID of every agent that saw them. Filter with `capture:201` or `capture:201,211`, group charts by capture ID or backend type, and use the Capture ID filter at the top of each dashboard.

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

**Storage.** Admin → Storage holds the ClickHouse connections and which one is in use. Retention per data type is under Settings and applies at startup.

## Limits of this version

- If ClickHouse is down, the panel retries each insert three times, then drops those rows and counts them. There is no disk buffer.
- HEP is plain UDP or TCP, without TLS. Keep port 9060 on a private network or set an auth key.
- One collector process. It doesn't scale out.
- One-way audio and no-media aren't detected. MOS is estimated from RTCP loss and jitter unless the sender reports it.
- No alert rules.
- Local accounts only: no LDAP, RADIUS, OIDC or TOTP.
- No PCAP import or share links. PCAP export works.
- `linux/amd64` images only.

## Development

The code is a Rust daemon (`crates/vpd`, axum) that embeds a React and TypeScript UI (`web`). Run MariaDB and ClickHouse locally, then in two terminals:

```bash
VP_LISTEN=127.0.0.1:8080 VP_DB_HOST=127.0.0.1 VP_DB_USER=voip_panel VP_DB_PASSWORD=your-password cargo run -p vpd
```

```bash
npm --prefix web run dev
```

Vite proxies `/api` to port 8080. After adding or changing a setting, regenerate the reference:

```bash
cargo run -q -p vpd -- env > docs/ENVIRONMENT.md
```

```bash
cargo run -q -p vpd -- env --dotenv > .env.example
```
