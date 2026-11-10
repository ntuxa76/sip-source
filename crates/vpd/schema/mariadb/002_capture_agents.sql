-- Version 2: removes the demo storage and alert acknowledgements, adds capture agents.

DELETE FROM storages WHERE type = 'demo';

DROP TABLE alert_acks;

CREATE TABLE agents (
    capture_id INT UNSIGNED PRIMARY KEY,
    name VARCHAR(128) NOT NULL,
    source_ip VARCHAR(64) NOT NULL DEFAULT '',
    last_seen DATETIME(6) NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
