CREATE TABLE activities (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    activity_type   TEXT NOT NULL,
    started_at      TIMESTAMPTZ NOT NULL,
    duration_secs   INTEGER NOT NULL,
    distance_meters REAL,
    calories        INTEGER,
    notes           TEXT,
    source          TEXT NOT NULL DEFAULT 'manual',
    device_data     JSONB,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_activities_user_started ON activities(user_id, started_at DESC);
