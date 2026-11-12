# sip-source

SIP capture, troubleshooting and monitoring for voice networks.

Your SIP servers send their traffic to sip-source over HEP. It stores every message, joins the
legs of each call, scores media quality from RTCP, and tracks registrations. Engineers and support
staff then search calls, read call flows, follow subscribers and watch dashboards in one web
interface.

## Features

- **Calls.** Every call with all its legs, as a message ladder, a timeline and raw messages, with
  findings such as missing ACKs, authentication loops, high post-dial delay or poor quality.
  Export a call as PCAP, text or JSON.
- **Search.** Find calls by number in any format, Call-ID, address, trunk, status, delay, MOS and
  more, from a filter panel or a one-line query. Save and share searches; export results as CSV.
- **Subscribers.** Registration state and history per number, devices, recent calls and quality.
- **Dashboards.** Built-in dashboards for calls, carriers, registrations, media, nodes, security
  and capture health. Each dashboard is one JSON document; panels are built from a metric catalogue
  or from SQL.
- **Live capture** of dialogs as they happen, off until someone starts it.
- **SQL workbench** with documented read-only views of all captured data.
- **Access control.** Roles, per-user permission overrides, an audit log, and single sign-on with
  any OpenID Connect provider.
- **Storage.** A built-in storage that needs nothing else, or your own ClickHouse server.

## Quick start

You need Docker or Podman. The compose file in this repository runs sip-source together with
MariaDB. Copy [docker-compose.yml](docker-compose.yml) and [.env.compose.example](.env.compose.example)
to the server, then:

```bash
cp .env.compose.example .env
```

Set the passwords in `.env`, then start it:

```bash
docker compose up -d
```

Open `http://SERVER:8080` and create the administrator account. Until that is done, anyone who
reaches the port can create it, so set `SS_SETUP_TOKEN` in `.env` if the port is exposed. Then
point your SIP servers at port 9060, as described in [docs/CAPTURE.md](docs/CAPTURE.md).

To use a MariaDB you already have, run the image on its own. Only three variables are required,
and the database must exist:

```bash
docker run -d --name sip-source \
  -p 8080:8080 -p 9060:9060/udp -p 9060:9060/tcp \
  -v sip-source-data:/data \
  -e SS_DB_HOST=mariadb.example.net \
  -e SS_DB_USER=sip_source \
  -e SS_DB_PASSWORD=change-me \
  ntuxa76/sip-source:0.9.1
```

The service speaks plain HTTP. Put a TLS reverse proxy in front of it for production; see
[docs/SECURITY.md](docs/SECURITY.md).

## Storage

Captured traffic goes to the built-in **Local file** storage from the first start. It keeps its
data under `/data` in the container, so keep that path on a volume, as above. It runs the
ClickHouse engine inside the container, so search, dashboards and SQL behave exactly as on a
ClickHouse server, and it uses up to half of the container's memory.

To use your own ClickHouse server instead, open **Admin → Storage → Add storage**, choose
**ClickHouse server**, enter its HTTP address (port 8123 by default), press **Test**, then **Use this
storage**. sip-source creates the database, tables and views itself. Its ClickHouse user needs
`CREATE`, `ALTER`, `INSERT` and `SELECT` on that database. Data already in the Local file storage is
not copied over.

Users, roles, sessions, dashboards, settings and the audit log are kept in MariaDB.

## Configuration

Every setting has an environment variable named `SS_<SETTING>`. Most can also be changed under
**Admin → Settings**; a value set in the environment takes priority and is shown as locked there.
An invalid value stops the container at startup with a message naming the variable.

- [docs/ENVIRONMENT.md](docs/ENVIRONMENT.md) lists every variable with its default.
- [.env.example](.env.example) has the same list, ready to copy.
- `docker run --rm ntuxa76/sip-source:0.9.1 env` prints the list from the image.

Passwords and keys can be read from files with the `_FILE` suffix, for example
`SS_DB_PASSWORD_FILE=/run/secrets/db_password`.

## Documentation

| Guide | Contents |
|---|---|
| [docs/CAPTURE.md](docs/CAPTURE.md) | Sending HEP from Asterisk and other servers, naming sources, joining call legs, network topology |
| [docs/SINGLE-SIGN-ON.md](docs/SINGLE-SIGN-ON.md) | OpenID Connect setup, account linking and role mapping |
| [docs/SECURITY.md](docs/SECURITY.md) | Deployment checklist and how the application protects itself |
| [docs/ENVIRONMENT.md](docs/ENVIRONMENT.md) | Every environment variable |
| [CHANGELOG.md](CHANGELOG.md) | Changes per release |

## Upgrading

Change the image tag and restart the container. The database schema is upgraded automatically at
startup. Read [CHANGELOG.md](CHANGELOG.md) for each version you skip; for example, 0.7 renamed the
environment variables from `VP_*` to `SS_*` (the old names still work, with a warning).

## Limitations

- HEP is received over plain UDP or TCP, without TLS.
- One collector instance; it does not scale out.
- If the storage is unavailable, inserts are retried three times and then dropped and counted;
  there is no disk buffer.
- One-way audio and missing media are not detected. MOS is estimated from RTCP loss and jitter
  unless the sender reports it.
- No alert rules and no PCAP import.
- Sign-in with local accounts or OpenID Connect; no LDAP, RADIUS, SAML or two-factor codes.
- Images are built for `linux/amd64`.

## Repository layout

```text
crates/sip-source/     Rust service: web server and API, HEP ingest, call assembly
  src/                 source code
  dashboards/          built-in dashboards, one JSON document each
web/                   React and TypeScript user interface, embedded in the service
schema.sql             database schema for MariaDB and ClickHouse
docs/                  guides and the environment reference
Dockerfile             container image build
docker-compose.yml     sip-source with MariaDB
.env.example           every environment variable
.env.compose.example   the variables the compose file needs
```

## Development

Requirements: Rust (stable), Node.js 22 and a MariaDB server. In one terminal, run the service:

```bash
SS_LISTEN=127.0.0.1:8080 SS_DB_HOST=127.0.0.1 SS_DB_USER=sip_source SS_DB_PASSWORD=change-me SS_DATA_DIR=./data cargo run -p sip-source
```

The Local file storage needs the ClickHouse binary: set `SS_LOCAL_ENGINE` to its path, or add a
ClickHouse server under **Admin → Storage**. In a second terminal, run the interface with live
reload; it forwards `/api` to port 8080:

```bash
npm --prefix web install
```

```bash
npm --prefix web run dev
```

Run the tests:

```bash
cargo test
```

Build the image:

```bash
docker build -t sip-source .
```

After adding or changing a setting, regenerate the reference files:

```bash
cargo run -q -p sip-source -- env > docs/ENVIRONMENT.md
```

```bash
cargo run -q -p sip-source -- env --dotenv > .env.example
```

To change the database schema, edit [schema.sql](schema.sql): change the `@mariadb` part and add the
same change as a new `@mariadb-upgrade` part, so existing installations are upgraded too.

## Acknowledgements

The loading indicators are [thinking-orbs](https://github.com/Jakubantalik/Libraries.dev) by Jakub
Antalik, used under the MIT licence.
