-- sip-source database schema
--
-- The service applies this file itself at startup; nothing has to be run by hand. It is split
-- into parts, each starting with a "-- @" line:
--
--   @mariadb            Application state in MariaDB, as of the current schema version. Runs once,
--                       on an empty database.
--   @mariadb-upgrade N  Brings a database created by an older version up to version N. Each runs
--                       once, in order, and is recorded in the schema_migrations table.
--   @clickhouse         Captured data. Runs at every start and whenever a storage is selected, so
--                       every statement is safe to repeat. {retention_*} is replaced with the
--                       retention settings, in days.
--
-- A statement ends with a semicolon at the end of a line. To change the MariaDB schema, change
-- the @mariadb part and add the same change as the next @mariadb-upgrade part.


-- @mariadb

-- Named sets of permissions. The role with locked = 1 (Administrator) always has every permission.
CREATE TABLE roles (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(64) NOT NULL UNIQUE,
    description VARCHAR(255) NOT NULL DEFAULT '',
    permissions LONGTEXT NOT NULL,
    locked TINYINT NOT NULL DEFAULT 0,
    created_at DATETIME(6) NOT NULL,
    updated_at DATETIME(6) NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- Accounts. Single sign-on accounts have auth_source = 'sso', no usable password, and are found
-- again by external_id (issuer|subject). grants and denies are per-user permission overrides.
CREATE TABLE users (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    username VARCHAR(96) NOT NULL UNIQUE,
    display_name VARCHAR(128) NOT NULL DEFAULT '',
    email VARCHAR(190) NOT NULL DEFAULT '',
    password_hash VARCHAR(255) NOT NULL,
    auth_source VARCHAR(16) NOT NULL DEFAULT 'local',
    external_id VARCHAR(255) NULL,
    role_id BIGINT NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'active',
    grants LONGTEXT NOT NULL,
    denies LONGTEXT NOT NULL,
    prefs LONGTEXT NOT NULL,
    failed_attempts INT NOT NULL DEFAULT 0,
    locked_until DATETIME(6) NULL,
    last_login_at DATETIME(6) NULL,
    created_at DATETIME(6) NOT NULL,
    updated_at DATETIME(6) NOT NULL,
    UNIQUE KEY uq_users_external (external_id),
    CONSTRAINT fk_users_role FOREIGN KEY (role_id) REFERENCES roles(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- Signed-in browsers. id is the SHA-256 of the cookie value, never the value itself.
CREATE TABLE sessions (
    id CHAR(64) PRIMARY KEY,
    user_id BIGINT NOT NULL,
    created_at DATETIME(6) NOT NULL,
    last_seen_at DATETIME(6) NOT NULL,
    expires_at DATETIME(6) NOT NULL,
    ip VARCHAR(64) NOT NULL DEFAULT '',
    user_agent VARCHAR(255) NOT NULL DEFAULT '',
    CONSTRAINT fk_sessions_user FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- Dashboards, each one JSON document in body.
CREATE TABLE dashboards (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    uid VARCHAR(64) NOT NULL UNIQUE,
    title VARCHAR(190) NOT NULL,
    description VARCHAR(512) NOT NULL DEFAULT '',
    body LONGTEXT NOT NULL,
    version INT NOT NULL DEFAULT 1,
    updated_by VARCHAR(96) NOT NULL DEFAULT '',
    created_at DATETIME(6) NOT NULL,
    updated_at DATETIME(6) NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- Earlier versions of each dashboard, for history and restore.
CREATE TABLE dashboard_versions (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    dashboard_id BIGINT NOT NULL,
    version INT NOT NULL,
    body LONGTEXT NOT NULL,
    message VARCHAR(255) NOT NULL DEFAULT '',
    created_by VARCHAR(96) NOT NULL DEFAULT '',
    created_at DATETIME(6) NOT NULL,
    UNIQUE KEY uq_dashboard_version (dashboard_id, version),
    CONSTRAINT fk_versions_dashboard FOREIGN KEY (dashboard_id) REFERENCES dashboards(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- Where captured data is written: the built-in Local file storage or ClickHouse servers.
-- secret is the encrypted password.
CREATE TABLE storages (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(96) NOT NULL UNIQUE,
    type VARCHAR(32) NOT NULL,
    config LONGTEXT NOT NULL,
    secret LONGTEXT NOT NULL,
    active TINYINT NOT NULL DEFAULT 0,
    built_in TINYINT NOT NULL DEFAULT 0,
    last_test_at DATETIME(6) NULL,
    last_test_ok TINYINT NULL,
    last_test_message VARCHAR(512) NOT NULL DEFAULT '',
    created_at DATETIME(6) NOT NULL,
    updated_at DATETIME(6) NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- Application settings changed from the UI. Keys starting with _ are internal markers.
CREATE TABLE settings (
    skey VARCHAR(96) PRIMARY KEY,
    value LONGTEXT NOT NULL,
    updated_by VARCHAR(96) NOT NULL DEFAULT '',
    updated_at DATETIME(6) NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- Searches saved by users, optionally shared with everyone.
CREATE TABLE saved_searches (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    owner_id BIGINT NOT NULL,
    name VARCHAR(128) NOT NULL,
    q TEXT NOT NULL,
    shared TINYINT NOT NULL DEFAULT 0,
    created_at DATETIME(6) NOT NULL,
    CONSTRAINT fk_saved_owner FOREIGN KEY (owner_id) REFERENCES users(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- Who did what, kept for the audit retention period.
CREATE TABLE audit_events (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    ts DATETIME(6) NOT NULL,
    actor VARCHAR(96) NOT NULL,
    src_ip VARCHAR(64) NOT NULL DEFAULT '',
    action VARCHAR(64) NOT NULL,
    object VARCHAR(190) NOT NULL DEFAULT '',
    outcome VARCHAR(16) NOT NULL,
    severity VARCHAR(16) NOT NULL DEFAULT 'info',
    details TEXT NOT NULL,
    KEY idx_audit_ts (ts)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- HEP senders, by address, with the alias shown in the UI.
CREATE TABLE sources (
    ip VARCHAR(64) PRIMARY KEY,
    alias VARCHAR(128) NOT NULL DEFAULT '',
    capture_id INT UNSIGNED NOT NULL DEFAULT 0,
    last_seen DATETIME(6) NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- Single sign-on attempts waiting for the provider to answer. Used once, kept ten minutes.
CREATE TABLE sso_logins (
    state CHAR(64) PRIMARY KEY,
    nonce CHAR(64) NOT NULL,
    verifier VARCHAR(128) NOT NULL,
    created_at DATETIME(6) NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;


-- @mariadb-upgrade 2
-- Removes the demo storage and alert acknowledgements, adds capture agents.

DELETE FROM storages WHERE type = 'demo';

DROP TABLE alert_acks;

CREATE TABLE agents (
    capture_id INT UNSIGNED PRIMARY KEY,
    name VARCHAR(128) NOT NULL,
    source_ip VARCHAR(64) NOT NULL DEFAULT '',
    last_seen DATETIME(6) NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;


-- @mariadb-upgrade 3
-- HEP sources are keyed by sender address instead of capture ID.

CREATE TABLE sources (
    ip VARCHAR(64) PRIMARY KEY,
    alias VARCHAR(128) NOT NULL DEFAULT '',
    capture_id INT UNSIGNED NOT NULL DEFAULT 0,
    last_seen DATETIME(6) NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

INSERT IGNORE INTO sources (ip, alias, capture_id, last_seen)
    SELECT source_ip, IF(name LIKE 'capture-%', '', name), capture_id, last_seen FROM agents WHERE source_ip <> '';

DROP TABLE agents;


-- @mariadb-upgrade 4
-- Single sign-on with OpenID Connect.

CREATE TABLE sso_logins (
    state CHAR(64) PRIMARY KEY,
    nonce CHAR(64) NOT NULL,
    verifier VARCHAR(128) NOT NULL,
    created_at DATETIME(6) NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

ALTER TABLE users
    ADD COLUMN auth_source VARCHAR(16) NOT NULL DEFAULT 'local' AFTER password_hash,
    ADD COLUMN external_id VARCHAR(255) NULL AFTER auth_source,
    ADD UNIQUE KEY uq_users_external (external_id);


-- @clickhouse

CREATE TABLE IF NOT EXISTS sip_messages (
    ts DateTime64(6, 'UTC') CODEC(Delta, ZSTD(1)),
    ingest_ts DateTime64(3, 'UTC') CODEC(Delta, ZSTD(1)),
    kind LowCardinality(String),
    capture_id UInt32,
    capture_point LowCardinality(String),
    agent_ip LowCardinality(String),
    src_ip String, src_port UInt16, dst_ip String, dst_port UInt16,
    transport LowCardinality(String),
    call_id String,
    is_request Bool,
    method LowCardinality(String),
    status UInt16,
    reason LowCardinality(String),
    cseq_num UInt32,
    cseq_method LowCardinality(String),
    from_user String, from_user_n String, from_domain LowCardinality(String), from_tag String,
    to_user String, to_user_n String, to_domain LowCardinality(String), to_tag String,
    ruri_user String, ruri_user_n String, pai_user_n String,
    user_agent LowCardinality(String),
    via_branch String,
    has_sdp Bool,
    codecs Array(LowCardinality(String)),
    sdp_ip String,
    sdp_dir LowCardinality(String),
    q850_cause UInt8,
    x_cid String,
    hep_cid String,
    parse_error Bool,
    payload String CODEC(ZSTD(3)),
    INDEX idx_from from_user_n TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_to to_user_n TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_ruri ruri_user_n TYPE bloom_filter(0.01) GRANULARITY 4
) ENGINE = MergeTree PARTITION BY toYYYYMMDD(ts) ORDER BY (call_id, ts)
TTL toDateTime(ts) + INTERVAL {retention_messages} DAY;

CREATE TABLE IF NOT EXISTS registration_events (
    ts DateTime64(6, 'UTC') CODEC(Delta, ZSTD(1)),
    aor String, number String, grp LowCardinality(String),
    contact String, src_ip String, src_port UInt16,
    transport LowCardinality(String),
    user_agent LowCardinality(String),
    expires_requested UInt32, expires_granted UInt32,
    result LowCardinality(String),
    code UInt16,
    rrd_ms Nullable(UInt32),
    node LowCardinality(String),
    capture_id UInt32,
    agent_ip LowCardinality(String),
    nat Bool,
    challenged Bool
) ENGINE = MergeTree PARTITION BY toYYYYMMDD(ts) ORDER BY (aor, ts)
TTL toDateTime(ts) + INTERVAL {retention_registrations} DAY;

CREATE TABLE IF NOT EXISTS calls (
    call_uid String,
    version UInt64,
    state LowCardinality(String),
    start_ts DateTime64(6, 'UTC'),
    ring_ts Nullable(DateTime64(6, 'UTC')),
    answer_ts Nullable(DateTime64(6, 'UTC')),
    end_ts Nullable(DateTime64(6, 'UTC')),
    pdd_ms Nullable(UInt32), setup_ms Nullable(UInt32), talk_ms UInt64, egress_pdd_ms Nullable(UInt32),
    caller String, caller_n String, callee String, callee_n String,
    direction LowCardinality(String),
    ingress_group LowCardinality(String), egress_group LowCardinality(String),
    trunk LowCardinality(String), node LowCardinality(String), pbx LowCardinality(String),
    backend LowCardinality(String),
    capture_ids Array(UInt32), capture_points Array(LowCardinality(String)), agent_ips Array(LowCardinality(String)),
    media_node LowCardinality(String),
    final_status UInt16, final_reason LowCardinality(String), egress_status UInt16,
    q850_cause UInt8, end_cause LowCardinality(String), hangup_side LowCardinality(String),
    root_call_id String, b_call_id String, legs Array(String), link LowCardinality(String),
    codecs Array(LowCardinality(String)),
    ua_caller LowCardinality(String), ua_callee LowCardinality(String),
    ua_vendor LowCardinality(String), ua_model LowCardinality(String),
    caller_ip String, callee_ip String, caller_country LowCardinality(String),
    min_mos Nullable(Float32), max_loss_pct Nullable(Float32), max_jitter_ms Nullable(Float32), max_rtt_ms Nullable(Float32),
    mos_estimated Bool, media_a_to_b Bool, media_b_to_a Bool,
    findings Array(LowCardinality(String)),
    msg_count UInt32,
    INDEX idx_uid call_uid TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_root root_call_id TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_caller caller_n TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_callee callee_n TYPE bloom_filter(0.01) GRANULARITY 4
) ENGINE = ReplacingMergeTree(version) PARTITION BY toYYYYMM(start_ts) ORDER BY (toDate(start_ts), call_uid)
TTL toDateTime(start_ts) + INTERVAL {retention_calls} DAY;

CREATE TABLE IF NOT EXISTS qos_reports (
    ts DateTime64(6, 'UTC') CODEC(Delta, ZSTD(1)),
    call_id String,
    capture_id UInt32,
    capture_point LowCardinality(String),
    agent_ip LowCardinality(String),
    source LowCardinality(String),
    ssrc String,
    src_ip String, dst_ip String,
    packets UInt64, lost Int64,
    loss_pct Float32, jitter_ms Float32, rtt_ms Nullable(Float32),
    mos Nullable(Float32),
    mos_estimated Bool
) ENGINE = MergeTree PARTITION BY toYYYYMMDD(ts) ORDER BY (call_id, ts)
TTL toDateTime(ts) + INTERVAL {retention_qos} DAY;

CREATE TABLE IF NOT EXISTS logs (
    ts DateTime64(6, 'UTC') CODEC(Delta, ZSTD(1)),
    call_id String,
    capture_id UInt32,
    agent_ip LowCardinality(String),
    node LowCardinality(String),
    severity LowCardinality(String),
    message String CODEC(ZSTD(3))
) ENGINE = MergeTree PARTITION BY toYYYYMMDD(ts) ORDER BY (call_id, ts)
TTL toDateTime(ts) + INTERVAL {retention_logs} DAY;

CREATE TABLE IF NOT EXISTS system_1m (
    bucket DateTime('UTC'),
    metric LowCardinality(String),
    dim LowCardinality(String),
    name LowCardinality(String),
    value Float64
) ENGINE = MergeTree PARTITION BY toYYYYMM(bucket) ORDER BY (metric, dim, name, bucket)
TTL toDateTime(bucket) + INTERVAL {retention_metrics} DAY;

CREATE TABLE IF NOT EXISTS reg_active_1m (
    bucket DateTime('UTC'),
    grp LowCardinality(String),
    active UInt32
) ENGINE = MergeTree PARTITION BY toYYYYMM(bucket) ORDER BY (grp, bucket)
TTL toDateTime(bucket) + INTERVAL {retention_metrics} DAY;

-- Retention, applied again whenever the setting changes.
ALTER TABLE sip_messages MODIFY TTL toDateTime(ts) + INTERVAL {retention_messages} DAY;
ALTER TABLE registration_events MODIFY TTL toDateTime(ts) + INTERVAL {retention_registrations} DAY;
ALTER TABLE qos_reports MODIFY TTL toDateTime(ts) + INTERVAL {retention_qos} DAY;
ALTER TABLE logs MODIFY TTL toDateTime(ts) + INTERVAL {retention_logs} DAY;
ALTER TABLE calls MODIFY TTL toDateTime(start_ts) + INTERVAL {retention_calls} DAY;
ALTER TABLE system_1m MODIFY TTL toDateTime(bucket) + INTERVAL {retention_metrics} DAY;
ALTER TABLE reg_active_1m MODIFY TTL toDateTime(bucket) + INTERVAL {retention_metrics} DAY;

-- Columns added after the first release, for tables created by older versions.
ALTER TABLE sip_messages ADD COLUMN IF NOT EXISTS agent_ip LowCardinality(String) AFTER capture_point;
ALTER TABLE registration_events ADD COLUMN IF NOT EXISTS agent_ip LowCardinality(String) AFTER capture_id;
ALTER TABLE qos_reports ADD COLUMN IF NOT EXISTS agent_ip LowCardinality(String) AFTER capture_point;
ALTER TABLE logs ADD COLUMN IF NOT EXISTS agent_ip LowCardinality(String) AFTER capture_id;
ALTER TABLE calls ADD COLUMN IF NOT EXISTS agent_ips Array(LowCardinality(String)) AFTER capture_points;

-- The public views. The SQL workbench and dashboard SQL panels read these, never the tables.
CREATE OR REPLACE VIEW v_calls AS SELECT * EXCEPT (version) FROM calls FINAL;

CREATE OR REPLACE VIEW v_findings AS SELECT arrayJoin(findings) AS code, * FROM v_calls;

CREATE OR REPLACE VIEW v_messages AS SELECT * EXCEPT (ingest_ts) FROM sip_messages;

CREATE OR REPLACE VIEW v_registrations AS SELECT * FROM registration_events;

CREATE OR REPLACE VIEW v_qos_reports AS SELECT * FROM qos_reports;

CREATE OR REPLACE VIEW v_logs AS SELECT * FROM logs;

CREATE OR REPLACE VIEW v_system_1m AS SELECT * FROM system_1m;

CREATE OR REPLACE VIEW v_kpi_reg_1m AS
  SELECT toStartOfMinute(ts) AS bucket, grp, count() AS attempts,
         countIf(code BETWEEN 200 AND 299) AS success,
         countIf(code = 401) AS fail_401, countIf(code = 403) AS fail_403,
         countIf(code = 408) AS fail_408, countIf(code >= 500) AS fail_503,
         quantile(0.95)(rrd_ms) AS rrd_ms
  FROM registration_events GROUP BY bucket, grp;

CREATE OR REPLACE VIEW v_reg_active_1m AS SELECT * FROM reg_active_1m;

-- One row per answered call per minute it was up.
CREATE OR REPLACE VIEW v_kpi_concurrency_1m AS
  SELECT toDateTime(arrayJoin(range(toUInt32(toStartOfMinute(assumeNotNull(answer_ts))), toUInt32(ifNull(end_ts, now())) + 1, 60)), 'UTC') AS bucket,
         direction, trunk, count() AS active
  FROM v_calls WHERE answer_ts IS NOT NULL
  GROUP BY bucket, direction, trunk;
