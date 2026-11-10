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
    /// A separate account for user-written SQL, when the storage has one. It can only read.
    reader: Option<(String, String)>,
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
        Ok(Ch { url, database, username, password, reader: None, client })
    }

    pub fn with_reader(mut self, username: String, password: String) -> Ch {
        self.reader = Some((username, password));
        self
    }

    async fn post(&self, body: String, database: bool, timeout_s: u64, params: &[(&str, String)]) -> Result<String, String> {
        self.post_as(&self.username, &self.password, body, database, timeout_s, params).await
    }

    async fn post_as(&self, username: &str, password: &str, body: String, database: bool, timeout_s: u64, params: &[(&str, String)]) -> Result<String, String> {
        let mut query: Vec<(&str, String)> = vec![("session_timezone", "UTC".into()), ("output_format_json_quote_64bit_integers", "0".into())];
        if database {
            query.push(("database", self.database.clone()));
        }
        query.extend(params.iter().cloned());
        let res = self.client.post(&self.url).timeout(Duration::from_secs(timeout_s + 5)).query(&query)
            .header("X-ClickHouse-User", username)
            .header("X-ClickHouse-Key", password)
            .body(body).send().await
            .map_err(|e| format!("Could not reach ClickHouse: {e}"))?;
        let status = res.status();
        let text = res.text().await.map_err(|e| e.to_string())?;
        if status.is_success() {
            Ok(text)
        } else {
            // With a JSON output format the error arrives inside the body, so look for the message.
            let line = text.lines().find(|l| l.contains("Exception")).or_else(|| text.lines().next()).unwrap_or("ClickHouse returned an error");
            let line = line.trim().trim_start_matches("\"exception\":").trim().trim_matches(|c| c == '"' || c == ',');
            let line = line.split(" (version ").next().unwrap_or(line);
            Err(line.chars().take(600).collect())
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

    /// User-written SQL. `readonly=1` makes the server refuse any change, any table function that
    /// reaches outside (url, remote, mysql, file ...) and any setting changed from the query, so
    /// the limits below hold.
    pub async fn query_readonly(&self, sql: &str, timeout_s: u64, max_rows: usize) -> Result<Table, String> {
        let (user, key) = self.reader.as_ref().map(|(u, k)| (u.as_str(), k.as_str())).unwrap_or((&self.username, &self.password));
        let text = self.post_as(user, key, format!("{} FORMAT JSONCompact", sql.trim().trim_end_matches(';')), true, timeout_s, &[
            ("readonly", "1".into()),
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
    // Columns added after the first release.
    for (table, column) in [
        ("sip_messages", "agent_ip LowCardinality(String) AFTER capture_point"),
        ("registration_events", "agent_ip LowCardinality(String) AFTER capture_id"),
        ("qos_reports", "agent_ip LowCardinality(String) AFTER capture_point"),
        ("logs", "agent_ip LowCardinality(String) AFTER capture_id"),
        ("calls", "agent_ips Array(LowCardinality(String)) AFTER capture_points"),
    ] {
        ch.exec(&format!("ALTER TABLE {table} ADD COLUMN IF NOT EXISTS {column}"), true).await?;
    }
    for view in views() {
        ch.exec(&view, true).await?;
    }
    Ok(())
}

/// `schema/clickhouse/tables.sql`, with the retention settings filled in.
fn tables(r: &Retention) -> Vec<String> {
    let file = include_str!("../schema/clickhouse/tables.sql")
        .replace("{retention_messages}", &r.messages.to_string())
        .replace("{retention_registrations}", &r.registrations.to_string())
        .replace("{retention_calls}", &r.calls.to_string())
        .replace("{retention_qos}", &r.qos.to_string())
        .replace("{retention_logs}", &r.logs.to_string())
        .replace("{retention_metrics}", &r.metrics.to_string());
    crate::db::statements(&file)
}

fn views() -> Vec<String> {
    crate::db::statements(include_str!("../schema/clickhouse/views.sql"))
}
