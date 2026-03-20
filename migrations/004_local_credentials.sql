CREATE TABLE local_credentials (
    user_id        UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    password_hash  TEXT NOT NULL,
    email_verified BOOLEAN NOT NULL DEFAULT false,
    verify_token   TEXT,
    verify_expires TIMESTAMPTZ,
    reset_token    TEXT,
    reset_expires  TIMESTAMPTZ,
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
