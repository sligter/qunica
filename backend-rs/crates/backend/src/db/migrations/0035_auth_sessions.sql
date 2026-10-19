-- Note: sessions persist until explicit revocation; credentials are identifiers,
-- never bearer tokens. Legacy JWTs cannot manufacture a device session.
CREATE TABLE auth_sessions (
    id TEXT PRIMARY KEY NOT NULL,
    owner_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    credential_id TEXT NOT NULL UNIQUE,
    device_label TEXT NOT NULL,
    created_at TEXT NOT NULL,
    revoked_at TEXT
);

CREATE INDEX ix_auth_sessions_owner ON auth_sessions(owner_id, created_at);
