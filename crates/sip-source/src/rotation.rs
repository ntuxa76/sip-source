//! Size-based rotation, on top of the retention days. When the captured data grows past
//! `storage_max_gb`, the oldest day of messages, RTCP reports, registrations and logs is
//! dropped, one day per run, until it fits again. Call records and metrics are small and keep
//! their own retention. Off when the limit is 0, the default.

use crate::api::App;
use crate::db;
use crate::sql;

/// Tables partitioned by day (`toYYYYMMDD`), so a whole day can be dropped at once.
const DAILY: &[&str] = &["sip_messages", "qos_reports", "registration_events", "logs"];

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

#[derive(Default)]
pub struct Rotation {
    warned_today: bool,
}

fn num(v: &serde_json::Value) -> f64 {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or(0.0)
}

impl Rotation {
    pub async fn run(&mut self, app: &App) {
        let max_gb = app.settings.int("storage_max_gb");
        if max_gb <= 0 {
            return;
        }
        let Some(ch) = app.storage.read().unwrap().clone() else { return };
        let db_name = sql::quote(&ch.database);

        let used = match ch.select(&format!("SELECT sum(bytes_on_disk) AS b FROM system.parts WHERE active AND database = {db_name}")).await {
            Ok(rows) => rows.first().map(|r| num(&r["b"])).unwrap_or(0.0),
            Err(e) => return tracing::warn!("storage rotation: can't read the data size: {e}"),
        };
        if used <= max_gb as f64 * GIB {
            self.warned_today = false;
            return;
        }
        let reason = format!("captured data is {:.1} GB, over the {max_gb} GB limit", used / GIB);

        let tables = DAILY.iter().map(|t| sql::quote(t)).collect::<Vec<_>>().join(", ");
        let oldest = match ch.select(&format!("SELECT min(partition_id) AS p FROM system.parts WHERE active AND database = {db_name} AND table IN ({tables})")).await {
            Ok(rows) => rows.first().and_then(|r| r["p"].as_str().map(str::to_string)).unwrap_or_default(),
            Err(e) => return tracing::warn!("storage rotation: can't list the stored days: {e}"),
        };
        let today = db::utc_now().format("%Y%m%d").to_string();
        if oldest.len() != 8 || oldest >= today {
            if !self.warned_today {
                tracing::warn!("storage rotation: {reason}, but only today's data is left; raise the limit");
                self.warned_today = true;
            }
            return;
        }
        for t in DAILY {
            if let Err(e) = ch.exec(&format!("ALTER TABLE {t} DROP PARTITION ID {}", sql::quote(&oldest)), true).await {
                tracing::warn!("storage rotation: can't drop {oldest} from {t}: {e}");
            }
        }
        let day = format!("{}-{}-{}", &oldest[..4], &oldest[4..6], &oldest[6..]);
        tracing::warn!("storage rotation: deleted the captured data of {day} because {reason}");
        db::audit(&app.db, "system", "", "storage.rotate", &day, "success", "warning", &format!("Deleted the captured data of {day} because {reason}."));
    }
}
