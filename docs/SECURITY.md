# Security

## Deployment checklist

- Put a TLS reverse proxy in front of the web port. The service itself speaks plain HTTP.
- Set `SS_SETUP_TOKEN` if the port is reachable by others before the first administrator is
  created, or create the administrator at startup with `SS_ADMIN_PASSWORD`.
- Set `SS_SECRET_KEY` so that saved ClickHouse passwords and the single sign-on client secret are
  stored encrypted. Keep it stable: changing it makes saved secrets unreadable.
- Keep the HEP port (9060) on a private network, or set `SS_HEP_AUTH_KEY`. HEP has no encryption.
- For your own ClickHouse server, give sip-source a user that can only use its own database.
- Set `SS_TRUST_PROXY=true` only behind a proxy you control, so client addresses in the audit log
  can't be forged.

## How the application protects itself

**Accounts.** Passwords are hashed with Argon2. Failed sign-ins lock the account for a while, and
too many failures from one address are refused. Unknown user names take as long to check as known
ones.

**Sessions.** Sessions are random tokens kept server side; only their hash is stored. The cookie is
HttpOnly and SameSite, and Secure behind a TLS proxy (`SS_COOKIE_SECURE=true` forces it). Sessions
end after the idle and absolute timeouts, and when a password is changed or an account disabled.

**Requests.** Every change needs a custom request header, which stops cross-site request forgery.
Pages are sent with a strict Content Security Policy, framing is refused, and the usual hardening
headers are set.

**Permissions.** Every permission is enforced by the server. Nobody can grant a permission they
don't hold, or change an account that has more access than their own. The Administrator role
always has every permission, and the last account that can manage users can't be removed or
demoted.

**Queries.** Search filters become SQL only through quoted values and fixed column names.
User-written SQL, in the SQL workbench and dashboard SQL panels, runs in ClickHouse read-only mode,
which also blocks table functions such as `url`, `remote` and `mysql`, so it can't reach other
systems or lift the time and row limits. On the built-in Local file storage it runs as a separate
account that can only read the capture database.

**Captured data.** SIP traffic is untrusted input. The parsers are tested against malformed packets,
captured text is escaped wherever it is displayed, and CSV exports neutralize cells that a
spreadsheet would run as formulas.

**Audit.** Sign-ins, changes to users, roles, settings and storage, exports and SQL queries are
written to the audit log. Secrets are never written to it.

## Reporting a problem

Report security problems privately to the maintainers rather than in a public issue.
