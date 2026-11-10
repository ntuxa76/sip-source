-- MariaDB schema, version 1: application state (users, roles, sessions, dashboards, storages,
-- settings, saved searches, audit log).

CREATE TABLE roles (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(64) NOT NULL UNIQUE,
    description VARCHAR(255) NOT NULL DEFAULT '',
    permissions LONGTEXT NOT NULL,
    locked TINYINT NOT NULL DEFAULT 0,
    created_at DATETIME(6) NOT NULL,
    updated_at DATETIME(6) NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE users (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    username VARCHAR(96) NOT NULL UNIQUE,
    display_name VARCHAR(128) NOT NULL DEFAULT '',
    email VARCHAR(190) NOT NULL DEFAULT '',
    password_hash VARCHAR(255) NOT NULL,
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
    CONSTRAINT fk_users_role FOREIGN KEY (role_id) REFERENCES roles(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

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

CREATE TABLE settings (
    skey VARCHAR(96) PRIMARY KEY,
    value LONGTEXT NOT NULL,
    updated_by VARCHAR(96) NOT NULL DEFAULT '',
    updated_at DATETIME(6) NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE saved_searches (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    owner_id BIGINT NOT NULL,
    name VARCHAR(128) NOT NULL,
    q TEXT NOT NULL,
    shared TINYINT NOT NULL DEFAULT 0,
    created_at DATETIME(6) NOT NULL,
    CONSTRAINT fk_saved_owner FOREIGN KEY (owner_id) REFERENCES users(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

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

CREATE TABLE alert_acks (
    alert_id VARCHAR(64) PRIMARY KEY,
    acked_by VARCHAR(96) NOT NULL,
    comment TEXT NOT NULL,
    acked_at DATETIME(6) NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
