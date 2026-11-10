-- ClickHouse tables for captured data. Applied when a storage is selected and at startup; every
-- statement is safe to run again. {retention_*} is replaced with the retention setting in days.

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
