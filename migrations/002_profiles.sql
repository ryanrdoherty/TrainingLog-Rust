CREATE TABLE profiles (
    user_id         UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    display_name    TEXT,
    preferred_units TEXT NOT NULL DEFAULT 'metric',
    phone_number    TEXT,
    phone_verified  BOOLEAN NOT NULL DEFAULT false,
    preferences     JSONB NOT NULL DEFAULT '{}',
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
