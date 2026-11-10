-- Version 3: HEP sources are keyed by sender address instead of capture ID.

CREATE TABLE sources (
    ip VARCHAR(64) PRIMARY KEY,
    alias VARCHAR(128) NOT NULL DEFAULT '',
    capture_id INT UNSIGNED NOT NULL DEFAULT 0,
    last_seen DATETIME(6) NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

INSERT IGNORE INTO sources (ip, alias, capture_id, last_seen)
    SELECT source_ip, IF(name LIKE 'capture-%', '', name), capture_id, last_seen FROM agents WHERE source_ip <> '';

DROP TABLE agents;
