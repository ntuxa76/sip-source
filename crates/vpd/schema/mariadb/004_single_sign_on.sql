-- Version 4: single sign-on with OpenID Connect.

-- Sign-ins that were sent to the identity provider and haven't come back yet. A row lives for at
-- most ten minutes and is used once.
CREATE TABLE sso_logins (
    state CHAR(64) PRIMARY KEY,
    nonce CHAR(64) NOT NULL,
    verifier VARCHAR(128) NOT NULL,
    created_at DATETIME(6) NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- Accounts created by single sign-on have no usable password and are found again by the
-- provider's issuer and subject, never by their user name.
ALTER TABLE users
    ADD COLUMN auth_source VARCHAR(16) NOT NULL DEFAULT 'local' AFTER password_hash,
    ADD COLUMN external_id VARCHAR(255) NULL AFTER auth_source,
    ADD UNIQUE KEY uq_users_external (external_id);
