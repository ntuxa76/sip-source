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
