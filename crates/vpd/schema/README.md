# Database schema

The daemon applies these files itself at startup; nothing has to be run by hand. They are kept here
as plain SQL so the schema can be read, reviewed and diffed.

## MariaDB (`mariadb/`)

Application state: users, roles, sessions, dashboards, storages, settings, saved searches, sources
and the audit log. Each file is one schema version. At startup every version not yet listed in the
`schema_migrations` table runs once, in order, under a lock so that several instances starting
together can't race. A file that has shipped is never edited; a change gets a new numbered file and
an entry in `MIGRATIONS` in `src/db.rs`.

Seed data (built-in roles, dashboards and the Local file storage) is inserted by `seed()` in
`src/db.rs`, not by these files, so that an administrator's later edits and deletions are kept.

## ClickHouse (`clickhouse/`)

Captured traffic. `tables.sql` creates the tables and `views.sql` the public `v_*` views that the
SQL workbench and dashboard SQL panels read. Both run every time a storage is selected and at
startup, so every statement must be safe to repeat (`IF NOT EXISTS`, `OR REPLACE`).
`{retention_*}` placeholders are replaced with the retention settings, in days. Columns added after
the first release and retention changes are applied by `init_schema()` in `src/ch.rs`.
