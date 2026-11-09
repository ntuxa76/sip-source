//! ClickHouse over HTTP, and the schema of the captured data.

use serde::Serialize;
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Clone)]
pub struct Ch {
    pub url: String,
    pub database: String,
    pub username: String,
    pub password: String,
    client: reqwest::Client,
}

#[derive(Serialize, Clone)]
pub struct Column {
    pub name: String,
    pub ty: String,
}

pub struct Table {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<Value>>,
    pub rows_read: u64,
    pub truncated: bool,
}

impl Ch {
    pub fn new(url: String, database: String, username: String, password: String) -> Result<Ch, String> {
        let client = reqwest::Client::builder().timeout(Duration::from_secs(600)).build().map_err(|e| e.to_string())?;
        Ok(Ch { url, database, username, password, client })
    }

    async fn post(&self, body: String, database: bool, timeout_s: u64, params: &[(&str, String)]) -> Result<String, String> {
        let mut query: Vec<(&str, String)> = vec![("session_timezone", "UTC".into()), ("output_format_json_quote_64bit_integers", "0".into())];
        if database {
            query.push(("database", self.database.clone()));
        }
        query.extend(params.iter().cloned());
        let res = self.client.post(&self.url).timeout(Duration::from_secs(timeout_s + 5)).query(&query)
            .header("X-ClickHouse-User", &self.username)
            .header("X-ClickHouse-Key", &self.password)
            .body(body).send().await
            .map_err(|e| format!("Could not reach ClickHouse: {e}"))?;
        let status = res.status();
        let text = res.text().await.map_err(|e| e.to_string())?;
        if status.is_success() {
            Ok(text)
        } else {
            Err(text.lines().next().unwrap_or("ClickHouse returned an error").chars().take(600).collect())
        }
    }

    pub async fn version(&self) -> Result<String, String> {
        self.post("SELECT version()".into(), false, 10, &[]).await.map(|v| v.trim().to_string())
    }

    pub async fn exec(&self, sql: &str, database: bool) -> Result<(), String> {
        self.post(sql.to_string(), database, 60, &[]).await.map(|_| ())
    }

    /// Internal queries, returning one JSON object per row.
    pub async fn select(&self, sql: &str) -> Result<Vec<Value>, String> {
        let text = self.post(format!("{sql} FORMAT JSONEachRow"), true, 60, &[]).await?;
        text.lines().map(|l| serde_json::from_str(l).map_err(|e| format!("Unexpected ClickHouse response: {e}"))).collect()
    }

    pub async fn insert(&self, table: &str, rows: String) -> Result<(), String> {
        let sql = format!("INSERT INTO {table} FORMAT JSONEachRow");
        self.post(rows, true, 30, &[("query", sql), ("date_time_input_format", "best_effort".into())]).await.map(|_| ())
    }

    /// User-written SQL. `readonly=2` makes the server itself refuse any change.
    pub async fn query_readonly(&self, sql: &str, timeout_s: u64, max_rows: usize) -> Result<Table, String> {
        let text = self.post(format!("{} FORMAT JSONCompact", sql.trim().trim_end_matches(';')), true, timeout_s, &[
            ("readonly", "2".into()),
            ("max_execution_time", timeout_s.to_string()),
            ("max_result_rows", (max_rows + 1).to_string()),
            ("result_overflow_mode", "break".into()),
        ]).await?;
        let body: Value = serde_json::from_str(&text).map_err(|e| format!("Unexpected ClickHouse response: {e}"))?;
        let columns: Vec<Column> = body["meta"].as_array().map(|m| m.iter().map(|c| Column { name: c["name"].as_str().unwrap_or("").to_string(), ty: c["type"].as_str().unwrap_or("").to_string() }).collect()).unwrap_or_default();
        let mut rows: Vec<Vec<Value>> = Vec::new();
        for row in body["data"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
            let cells = row.as_array().cloned().unwrap_or_default();
            rows.push(cells.into_iter().zip(columns.iter()).map(|(cell, col)| convert_time(cell, &col.ty)).collect());
        }
        let truncated = rows.len() > max_rows;
        rows.truncate(max_rows);
        Ok(Table { columns, rows, rows_read: body["statistics"]["rows_read"].as_u64().unwrap_or(0), truncated })
    }
}

/// ClickHouse returns times as wall-clock text in the column's zone. The app works in epoch ms.
fn convert_time(cell: Value, ty: &str) -> Value {
    if !(ty.starts_with("DateTime") || ty.starts_with("Nullable(DateTime") || ty.starts_with("Date") || ty.starts_with("Nullable(Date")) {
        return cell;
    }
    let Some(s) = cell.as_str() else { return cell };
    let parsed = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f")
        .or_else(|_| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").map(|d| d.and_hms_opt(0, 0, 0).unwrap_or_default()));
    let Ok(t) = parsed else { return cell };
    let tz: chrono_tz::Tz = ty.split('\'').nth(1).and_then(|z| z.parse().ok()).unwrap_or(chrono_tz::UTC);
    use chrono::TimeZone;
    match tz.from_local_datetime(&t).earliest() {
        Some(local) => json!(local.timestamp_millis()),
        None => json!(t.and_utc().timestamp_millis()),
    }
}

pub struct Retention {
    pub messages: i64,
    pub registrations: i64,
    pub qos: i64,
    pub logs: i64,
    pub calls: i64,
    pub metrics: i64,
}

/// Creates the database, tables and public views. Safe to run repeatedly.
pub async fn init_schema(ch: &Ch, r: &Retention) -> Result<(), String> {
    // A user without CREATE rights can still use a database that already exists.
    if let Err(e) = ch.exec(&format!("CREATE DATABASE IF NOT EXISTS `{}`", ch.database.replace('`', "")), false).await
        && ch.exec("SELECT 1", true).await.is_err()
    {
        return Err(e);
    }
    for ddl in tables(r) {
        ch.exec(&ddl, true).await?;
    }
    for (table, column, days) in [
        ("sip_messages", "ts", r.messages),
        ("registration_events", "ts", r.registrations),
        ("qos_reports", "ts", r.qos),
        ("logs", "ts", r.logs),
        ("calls", "start_ts", r.calls),
        ("system_1m", "bucket", r.metrics),
        ("reg_active_1m", "bucket", r.metrics),
    ] {
        ch.exec(&format!("ALTER TABLE {table} MODIFY TTL toDateTime({column}) + INTERVAL {days} DAY"), true).await?;
    }
    for view in views() {
        ch.exec(&view, true).await?;
    }
    Ok(())
}

fn tables(r: &Retention) -> Vec<String> {
    vec![
        format!("CREATE TABLE IF NOT EXISTS sip_messages (
            ts DateTime64(6, 'UTC') CODEC(Delta, ZSTD(1)),
            ingest_ts DateTime64(3, 'UTC') CODEC(Delta, ZSTD(1)),
            kind LowCardinality(String),
            capture_id UInt32,
            capture_point LowCardinality(String),
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
        TTL toDateTime(ts) + INTERVAL {} DAY", r.messages),
        format!("CREATE TABLE IF NOT EXISTS registration_events (
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
            nat Bool,
            challenged Bool
        ) ENGINE = MergeTree PARTITION BY toYYYYMMDD(ts) ORDER BY (aor, ts)
        TTL toDateTime(ts) + INTERVAL {} DAY", r.registrations),
        format!("CREATE TABLE IF NOT EXISTS calls (
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
            capture_ids Array(UInt32), capture_points Array(LowCardinality(String)),
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
        TTL toDateTime(start_ts) + INTERVAL {} DAY", r.calls),
        format!("CREATE TABLE IF NOT EXISTS qos_reports (
            ts DateTime64(6, 'UTC') CODEC(Delta, ZSTD(1)),
            call_id String,
            capture_id UInt32,
            capture_point LowCardinality(String),
            source LowCardinality(String),
            ssrc String,
            src_ip String, dst_ip String,
            packets UInt64, lost Int64,
            loss_pct Float32, jitter_ms Float32, rtt_ms Nullable(Float32),
            mos Nullable(Float32),
            mos_estimated Bool
        ) ENGINE = MergeTree PARTITION BY toYYYYMMDD(ts) ORDER BY (call_id, ts)
        TTL toDateTime(ts) + INTERVAL {} DAY", r.qos),
        format!("CREATE TABLE IF NOT EXISTS logs (
            ts DateTime64(6, 'UTC') CODEC(Delta, ZSTD(1)),
            call_id String,
            capture_id UInt32,
            node LowCardinality(String),
            severity LowCardinality(String),
            message String CODEC(ZSTD(3))
        ) ENGINE = MergeTree PARTITION BY toYYYYMMDD(ts) ORDER BY (call_id, ts)
        TTL toDateTime(ts) + INTERVAL {} DAY", r.logs),
        format!("CREATE TABLE IF NOT EXISTS system_1m (
            bucket DateTime('UTC'),
            metric LowCardinality(String),
            dim LowCardinality(String),
            name LowCardinality(String),
            value Float64
        ) ENGINE = MergeTree PARTITION BY toYYYYMM(bucket) ORDER BY (metric, dim, name, bucket)
        TTL toDateTime(bucket) + INTERVAL {} DAY", r.metrics),
        format!("CREATE TABLE IF NOT EXISTS reg_active_1m (
            bucket DateTime('UTC'),
            grp LowCardinality(String),
            active UInt32
        ) ENGINE = MergeTree PARTITION BY toYYYYMM(bucket) ORDER BY (grp, bucket)
        TTL toDateTime(bucket) + INTERVAL {} DAY", r.metrics),
    ]
}

fn views() -> Vec<String> {
    vec![
        "CREATE OR REPLACE VIEW v_calls AS SELECT * EXCEPT (version) FROM calls FINAL".to_string(),
        "CREATE OR REPLACE VIEW v_findings AS SELECT arrayJoin(findings) AS code, * FROM v_calls".to_string(),
        "CREATE OR REPLACE VIEW v_messages AS SELECT * EXCEPT (ingest_ts) FROM sip_messages".to_string(),
        "CREATE OR REPLACE VIEW v_registrations AS SELECT * FROM registration_events".to_string(),
        "CREATE OR REPLACE VIEW v_qos_reports AS SELECT * FROM qos_reports".to_string(),
        "CREATE OR REPLACE VIEW v_logs AS SELECT * FROM logs".to_string(),
        "CREATE OR REPLACE VIEW v_system_1m AS SELECT * FROM system_1m".to_string(),
        "CREATE OR REPLACE VIEW v_kpi_reg_1m AS
            SELECT toStartOfMinute(ts) AS bucket, grp, count() AS attempts,
                   countIf(code BETWEEN 200 AND 299) AS success,
                   countIf(code = 401) AS fail_401, countIf(code = 403) AS fail_403,
                   countIf(code = 408) AS fail_408, countIf(code >= 500) AS fail_503,
                   quantile(0.95)(rrd_ms) AS rrd_ms
            FROM registration_events GROUP BY bucket, grp".to_string(),
        "CREATE OR REPLACE VIEW v_reg_active_1m AS SELECT * FROM reg_active_1m".to_string(),
        // One row per answered call per minute it was up.
        "CREATE OR REPLACE VIEW v_kpi_concurrency_1m AS
            SELECT toDateTime(arrayJoin(range(toUInt32(toStartOfMinute(assumeNotNull(answer_ts))), toUInt32(ifNull(end_ts, now())) + 1, 60)), 'UTC') AS bucket,
                   direction, trunk, count() AS active
            FROM v_calls WHERE answer_ts IS NOT NULL
            GROUP BY bucket, direction, trunk".to_string(),
    ]
}
