# sports-log

A secure, async REST API backend for logging sporting activities — runs, swims, lifts, rides, and anything else. Written in Rust to explore the language and its performance characteristics.

---

## Table of Contents

- [Architecture Overview](#architecture-overview)
- [Technology Stack](#technology-stack)
- [Project Structure](#project-structure)
- [Database Schema](#database-schema)
- [API Reference](#api-reference)
- [Authentication Design](#authentication-design)
- [Metrics & Observability](#metrics--observability)
- [Rocky 9 Deployment (Podman Quadlets)](#rocky-9-deployment-podman-quadlets)
- [Future Work](#future-work)

---

## Architecture Overview

The system is a single Rust binary that exposes a JSON REST API on port 3000. In production it runs as a rootless Podman container alongside three supporting containers — PostgreSQL, Prometheus, and Grafana — all managed by systemd via Podman Quadlets.

```
                         ┌─────────────────────────────────────┐
                         │          sports-log-net (bridge)    │
                         │                                     │
  client ──:3000──► ┌────┴─────────┐     ┌──────────────────┐  │
                    │  sports-log  │────►│   postgres:5432  │  │
                    │  (Rust/Axum) │     └──────────────────┘  │
                    └────┬─────────┘                           │
                         │  GET /metrics                       │
                    ┌────▼─────────┐     ┌──────────────────┐  │
  :9090 ◄────────── │  prometheus  │     │     grafana      │◄─┘
                    └──────────────┘     └──────────────────┘
                                          :3001 ◄─── browser
```

All containers share a Podman bridge network named `sports-log-net`. The app container connects to Postgres by hostname. Prometheus scrapes `sports-log:3000/metrics` every 15 seconds. Grafana reads from Prometheus and serves dashboards on port 3001.

Systemd enforces startup ordering: `postgres` → `sports-log` → `prometheus` → `grafana`. Each service is set to `Restart=always` so crashes are handled automatically.

---

## Technology Stack

| Concern | Crate | Version | Notes |
|---|---|---|---|
| Web framework | `axum` | 0.8 | Tower-based, macro routing |
| Async runtime | `tokio` | 1 | Full feature set |
| HTTP middleware | `tower-http` | 0.6 | CORS, tracing, gzip |
| Database driver | `sqlx` | 0.8 | Async PostgreSQL, no ORM |
| Serialization | `serde` + `serde_json` | 1 | Derive macros |
| JWT | `jsonwebtoken` | 9 | HS256, token versioning |
| Password hashing | `argon2` | 0.5 | Argon2id, memory-hard |
| OAuth2 | `oauth2` | 4 | Authorization code flow |
| HTTP client | `reqwest` | 0.12 | rustls, for OAuth2 token exchange |
| Email | `lettre` | 0.11 | SMTP, async |
| Metrics | `metrics` + `axum-prometheus` | 0.24 / 0.7 | Prometheus exposition format |
| Process metrics | `metrics-process` | 2 | Memory, CPU, threads |
| Error handling | `thiserror` + `anyhow` | 2 / 1 | Typed errors + ad-hoc |
| Logging | `tracing` + `tracing-subscriber` | 0.1 / 0.3 | Structured, async-aware |
| UUIDs | `uuid` | 1 | v4, serde support |
| Date/time | `chrono` | 0.4 | serde support |
| Crypto utilities | `sha2`, `hex`, `rand` | — | OTP hashing, token generation |

Rust edition: **2024**. Minimum tested toolchain: **1.86**.

---

## Project Structure

```
sports-log/
├── Cargo.toml
├── Dockerfile
├── .dockerignore
├── .env.example              ← environment variable template
├── migrations/               ← plain SQL, run in order by sqlx::migrate!
│   ├── 001_users.sql
│   ├── 002_profiles.sql
│   ├── 003_oauth_connections.sql
│   ├── 004_local_credentials.sql
│   ├── 005_otp_challenges.sql
│   └── 006_activities.sql
├── src/
│   ├── lib.rs                ← module declarations
│   ├── main.rs               ← Tokio entry point, startup sequence
│   ├── config.rs             ← typed Config loaded from environment
│   ├── db.rs                 ← PgPool construction
│   ├── error.rs              ← AppError enum, IntoResponse impl
│   ├── metrics.rs            ← Prometheus init, process collector, /metrics handler
│   ├── auth/
│   │   ├── mod.rs
│   │   ├── jwt.rs            ← issue_token / verify_token, Claims struct
│   │   ├── middleware.rs     ← require_auth Axum middleware
│   │   ├── local.rs          ← email+password register, login, verify, reset
│   │   ├── otp.rs            ← 6-digit code request + verify
│   │   └── oauth.rs          ← Google / Facebook OAuth2 flows
│   ├── models/
│   │   ├── mod.rs
│   │   ├── user.rs
│   │   ├── profile.rs
│   │   └── activity.rs
│   └── routes/
│       ├── mod.rs            ← router assembly, middleware wiring
│       ├── profile.rs        ← /me endpoints
│       └── activities.rs     ← /activities CRUD
└── deploy/
    ├── setup.sh              ← host provisioning script
    ├── db.env.example
    ├── app.env.example
    ├── prometheus/
    │   └── prometheus.yml
    ├── grafana/
    │   ├── provisioning/
    │   │   ├── datasources/prometheus.yml
    │   │   └── dashboards/dashboards.yml
    │   └── dashboards/
    │       └── sports-log.json
    └── quadlets/
        ├── sports-log-net.network
        ├── postgres.container
        ├── sports-log.container
        ├── prometheus.container
        └── grafana.container
```

---

## Database Schema

All tables use UUIDs as primary keys generated by PostgreSQL (`gen_random_uuid()`). Timestamps are `TIMESTAMPTZ` (UTC). Raw device telemetry is stored as `JSONB` to accommodate the varying schemas of different fitness devices.

### users
The central identity table. Every authentication method anchors to a row here.

```sql
id            UUID PRIMARY KEY
email         TEXT NOT NULL UNIQUE
created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
token_version INTEGER NOT NULL DEFAULT 0      -- incremented on password reset to invalidate JWTs
```

### profiles
One-to-one with `users`. Holds display preferences and the phone number used for SMS OTP.

```sql
user_id         UUID PRIMARY KEY → users.id
display_name    TEXT
preferred_units TEXT NOT NULL DEFAULT 'metric'   -- 'metric' | 'imperial'
phone_number    TEXT
phone_verified  BOOLEAN NOT NULL DEFAULT false
preferences     JSONB NOT NULL DEFAULT '{}'       -- sparse user config bag
updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
```

### oauth_connections
Stores per-provider OAuth2 tokens. A user can have multiple rows (one per provider). Also used for future Garmin integration.

```sql
id            UUID PRIMARY KEY
user_id       UUID → users.id
provider      TEXT NOT NULL                      -- 'google' | 'facebook' | 'garmin'
provider_uid  TEXT NOT NULL
access_token  TEXT NOT NULL
refresh_token TEXT
expires_at    TIMESTAMPTZ
UNIQUE (provider, provider_uid)
```

### local_credentials
Created only for users who register with email and password. Absent for OAuth-only users.

```sql
user_id        UUID PRIMARY KEY → users.id
password_hash  TEXT NOT NULL                     -- argon2id hash
email_verified BOOLEAN NOT NULL DEFAULT false
verify_token   TEXT                              -- sha256 hash of the email token
verify_expires TIMESTAMPTZ
reset_token    TEXT                              -- sha256 hash of the reset token
reset_expires  TIMESTAMPTZ
updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
```

### otp_challenges
Transient records for 6-digit login codes. Short-lived (10 min TTL), single-use, attempt-limited.

```sql
id          UUID PRIMARY KEY
user_id     UUID → users.id
channel     TEXT NOT NULL                        -- 'email' | 'sms'
destination TEXT NOT NULL                        -- email address or phone number
code_hash   TEXT NOT NULL                        -- sha256 of the 6-digit code
expires_at  TIMESTAMPTZ NOT NULL
attempts    INTEGER NOT NULL DEFAULT 0
used        BOOLEAN NOT NULL DEFAULT false
created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
```

### activities
Core data table. Structured fields are normalized; raw device payloads go in `device_data`.

```sql
id              UUID PRIMARY KEY
user_id         UUID → users.id
activity_type   TEXT NOT NULL                    -- 'run' | 'swim' | 'lift' | 'cycle' | etc.
started_at      TIMESTAMPTZ NOT NULL
duration_secs   INTEGER NOT NULL
distance_meters REAL
calories        INTEGER
notes           TEXT
source          TEXT NOT NULL DEFAULT 'manual'   -- 'manual' | 'garmin' | 'apple'
device_data     JSONB                            -- GPS tracks, HR series, lap splits, etc.
created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()

INDEX (user_id, started_at DESC)
```

---

## API Reference

All protected routes require an `Authorization: Bearer <token>` header. All request and response bodies are `application/json`. Errors return `{"error": "<message>"}` with an appropriate HTTP status code.

### Authentication

| Method | Path | Auth | Description |
|---|---|---|---|
| `POST` | `/auth/register` | — | Register with email + password |
| `POST` | `/auth/login` | — | Login with email + password |
| `POST` | `/auth/verify-email` | — | Verify email address with token |
| `POST` | `/auth/forgot-password` | — | Request a password reset email |
| `POST` | `/auth/reset-password` | — | Complete password reset |
| `POST` | `/auth/otp/request` | — | Send 6-digit code via email or SMS |
| `POST` | `/auth/otp/verify` | — | Submit code, receive JWT |
| `GET` | `/auth/login/:provider` | — | Start OAuth2 flow (`google` or `facebook`) |
| `GET` | `/auth/callback/:provider` | — | OAuth2 redirect callback |

### Profile

| Method | Path | Auth | Description |
|---|---|---|---|
| `GET` | `/me` | ✓ | Get current user and profile |
| `PUT` | `/me/profile` | ✓ | Update display name, units, phone number |
| `PUT` | `/me/preferences` | ✓ | Merge-patch preferences JSON object |

### Activities

| Method | Path | Auth | Description |
|---|---|---|---|
| `GET` | `/activities` | ✓ | List activities (paginated, filterable) |
| `POST` | `/activities` | ✓ | Submit a new activity |
| `GET` | `/activities/:id` | ✓ | Get a single activity |
| `PUT` | `/activities/:id` | ✓ | Update an activity |
| `DELETE` | `/activities/:id` | ✓ | Delete an activity |

**Activity list query parameters:** `activity_type`, `from` (ISO 8601), `to` (ISO 8601), `limit` (max 100, default 20), `offset`.

### Observability

| Method | Path | Auth | Description |
|---|---|---|---|
| `GET` | `/metrics` | — | Prometheus scrape endpoint |

---

## Authentication Design

All three authentication paths produce an identical JWT response and are interchangeable. A single user account can use any combination of them.

### Email + Password

1. `POST /auth/register` — validates password strength (min 8 chars), hashes with argon2id, inserts `users` + `profiles` + `local_credentials`, generates a 32-byte random verify token (stored as its SHA-256 hash), and sends a verification email (TODO: wire `lettre`).
2. `POST /auth/verify-email` — submits the token from the email; atomically marks `email_verified = true` and issues a JWT.
3. `POST /auth/login` — looks up user and `local_credentials` in a single join, verifies the argon2 hash, rejects unverified accounts with 403, returns a JWT on success. Not-found and wrong-password both return 401 with identical bodies to prevent email enumeration.
4. `POST /auth/forgot-password` — always returns 200 regardless of whether the email exists. If it does, generates a 1-hour reset token and sends an email (TODO).
5. `POST /auth/reset-password` — validates the reset token, hashes the new password, and increments `token_version` on the `users` row, which invalidates all previously issued JWTs.

### 6-Digit OTP (Passwordless)

1. `POST /auth/otp/request { identifier, channel }` — `identifier` is an email address or phone number; `channel` is `"email"` or `"sms"`. Rate-limited to 3 requests per 15-minute window. Generates a cryptographically random 6-digit code, stores its SHA-256 hash with a 10-minute expiry, invalidates any previous unused codes, and sends the code (TODO: wire `lettre` / Twilio).  Always returns 200 to prevent enumeration.
2. `POST /auth/otp/verify { identifier, code }` — finds the most recent valid (unused, unexpired) challenge, increments the attempt counter first, then verifies. Invalidates the challenge after 5 failed attempts. Issues a JWT on success.

### OAuth2 (Google / Facebook)

1. `GET /auth/login/:provider` — builds the provider's authorization URL and redirects the browser. Scopes requested: `openid email profile` (Google) or `email public_profile` (Facebook).
2. `GET /auth/callback/:provider` — receives the authorization code, exchanges it for an access token via a direct `reqwest` POST to the provider's token endpoint, fetches user info (`/userinfo` for Google, `/me` for Facebook), and upserts the `users` + `profiles` + `oauth_connections` rows. Issues a JWT.

**Note:** The state/CSRF token from the OAuth2 authorization URL is not yet validated in the callback. This must be completed before production use — store the CSRF token in a short-lived signed cookie on the redirect and verify it in the callback.

### JWT Structure

```json
{
  "sub": "<user-uuid>",
  "email": "user@example.com",
  "ver": 0,
  "iat": 1710000000,
  "exp": 1710086400
}
```

`ver` mirrors `users.token_version`. On every authenticated request, the middleware fetches the user row and rejects tokens where `ver` does not match the current `token_version`. This allows immediate JWT invalidation after a password reset without a token blocklist.

---

## Metrics & Observability

The application exposes Prometheus metrics at `GET /metrics` in standard text exposition format.

### HTTP metrics (automatic, via `axum-prometheus`)

| Metric | Type | Labels |
|---|---|---|
| `axum_http_requests_total` | Counter | `method`, `endpoint`, `status` |
| `axum_http_requests_duration_seconds` | Histogram | `method`, `endpoint`, `status` |

These cover every route automatically with no per-handler instrumentation needed.

### Process metrics (background task, every 15s, via `metrics-process`)

| Metric | Type |
|---|---|
| `process_cpu_seconds_total` | Counter |
| `process_resident_memory_bytes` | Gauge |
| `process_virtual_memory_bytes` | Gauge |
| `process_threads` | Gauge |

### Business metrics (instrumented at call sites)

| Metric | Type | Labels |
|---|---|---|
| `sports_log_users_registered_total` | Counter | — |
| `sports_log_logins_total` | Counter | `method` (`local`\|`otp`\|`google`\|`facebook`), `status` (`success`\|`failure`) |
| `sports_log_activities_created_total` | Counter | — |

### Grafana dashboard

A pre-built dashboard is provisioned automatically at startup and includes panels for:
- Request rate and latency percentiles (p50/p95/p99)
- HTTP error rates (4xx / 5xx)
- New user registrations and total activities (stat panels)
- Login volume and failure rate by authentication method
- Process memory (RSS) and thread count
- CPU utilization
- PostgreSQL connection pool (active / idle)

---

## Rocky 9 Deployment (Podman Quadlets)

### Why Podman Quadlets?

Podman Quadlets are systemd unit files with a `[Container]` section. `systemd-generator` translates them into standard systemd service units at boot. This means:

- **No daemon** — Podman is daemonless; each container runs as a direct child process of systemd.
- **Rootless** — containers run as your deploy user, not root, dramatically reducing the blast radius of a container escape.
- **Standard Linux tooling** — `systemctl`, `journalctl`, `systemd-analyze` all work as normal.
- **Dependency ordering** — `After=` and `Requires=` enforce correct startup and restart sequencing.
- **No orchestrator overhead** — no kubelet, no etcd, no control plane; just systemd doing what it already does.

### Prerequisites

Install on a fresh Rocky Linux 9 server:

```bash
sudo dnf update -y
sudo dnf install -y podman curl git

# Verify Podman version — Quadlets require 4.4+
podman --version

# Enable lingering for the deploy user so containers survive logout
# (skip if running as root / system services)
sudo loginctl enable-linger $(whoami)
```

Podman 4.4+ ships with Rocky 9's default repositories. No COPR or extra repos needed.

### 1. Install Rust and build the binary

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

git clone <your-repo-url> ~/sports-log
cd ~/sports-log
cargo build --release
```

### 2. Build the container image

```bash
cd ~/sports-log
podman build -t localhost/sports-log:latest .
```

The Dockerfile uses a two-stage build:
- **Stage 1 (`builder`):** `rust:1.86-slim-bookworm` — compiles dependencies in a cached layer, then compiles the application source. Dependency compilation is cached separately so rebuilds after source-only changes are fast.
- **Stage 2 (`runtime`):** `debian:bookworm-slim` — copies only the compiled binary and migrations. The final image contains no Rust toolchain, no build tools, and runs as a non-root `appuser`.

### 3. Install config files and Quadlet units

```bash
cd ~/sports-log
sudo bash deploy/setup.sh
```

The setup script:
- Creates `/etc/sports-log/` with mode 750
- Copies `db.env.example` → `/etc/sports-log/db.env` and `app.env.example` → `/etc/sports-log/app.env` (only if they don't already exist)
- Copies `prometheus.yml` to `/etc/sports-log/`
- Copies Grafana provisioning and dashboard files to `/etc/sports-log/grafana/`
- Installs all four Quadlet files to `/etc/containers/systemd/`
- Runs `systemctl daemon-reload`

### 4. Edit the environment files

```bash
sudo nano /etc/sports-log/db.env
```

```ini
POSTGRES_USER=sports_log
POSTGRES_PASSWORD=<strong-random-password>
POSTGRES_DB=sports_log
```

```bash
sudo nano /etc/sports-log/app.env
```

```ini
DATABASE_URL=postgres://sports_log:<password>@postgres:5432/sports_log
JWT_SECRET=<at-least-32-random-chars>
JWT_EXPIRY_HOURS=24

APP_BASE_URL=https://yourdomain.com

GOOGLE_CLIENT_ID=<from Google Cloud Console>
GOOGLE_CLIENT_SECRET=<from Google Cloud Console>

FACEBOOK_CLIENT_ID=<from Meta Developer Portal>
FACEBOOK_CLIENT_SECRET=<from Meta Developer Portal>

SMTP_HOST=smtp.yourmailprovider.com
SMTP_PORT=587
SMTP_USER=noreply@yourdomain.com
SMTP_PASS=<smtp-password>
SMTP_FROM=noreply@yourdomain.com

TWILIO_ACCOUNT_SID=<from Twilio Console>
TWILIO_AUTH_TOKEN=<from Twilio Console>
TWILIO_FROM_NUMBER=+15551234567
```

The `DATABASE_URL` host is `postgres` — the container name, which is resolvable within the `sports-log-net` bridge network.

### 5. Set the Grafana admin password

Grafana reads its admin password from a secret file referenced in the Quadlet. Create it:

```bash
sudo mkdir -p /run/secrets
echo -n '<strong-grafana-password>' | sudo tee /run/secrets/grafana_admin_password
sudo chmod 600 /run/secrets/grafana_admin_password
```

### 6. Open firewall ports

```bash
# API
sudo firewall-cmd --permanent --add-port=3000/tcp

# Grafana (restrict to your IP in production)
sudo firewall-cmd --permanent --add-port=3001/tcp

# Prometheus (restrict to your IP or keep closed)
sudo firewall-cmd --permanent --add-port=9090/tcp

sudo firewall-cmd --reload
```

### 7. Pull upstream images

```bash
podman pull docker.io/postgres:16-alpine
podman pull docker.io/prom/prometheus:latest
podman pull docker.io/grafana/grafana:latest
```

### 8. Start and enable services

```bash
# Start in dependency order
sudo systemctl start postgres
sudo systemctl start sports-log
sudo systemctl start prometheus
sudo systemctl start grafana

# Verify all four are running
sudo systemctl status postgres sports-log prometheus grafana

# Enable on boot
sudo systemctl enable postgres sports-log prometheus grafana
```

On first start, the sports-log container runs `sqlx::migrate!` which applies all six migration files in order. The database schema is created automatically — no manual `psql` step required.

### 9. Verify

```bash
# Health check
curl http://localhost:3000/metrics | head -20

# Register a test user
curl -s -X POST http://localhost:3000/auth/register \
  -H 'Content-Type: application/json' \
  -d '{"email":"test@example.com","password":"hunter2abc"}' | jq

# Grafana dashboard
# Open http://<your-server-ip>:3001 in a browser
# Login: admin / <password from /run/secrets/grafana_admin_password>
# Dashboard is pre-loaded under the "sports-log" folder
```

### Persistent volumes

Podman creates named volumes automatically from the Quadlet `Volume=` directives:

| Volume | Contents |
|---|---|
| `postgres-data` | PostgreSQL data directory |
| `prometheus-data` | Prometheus TSDB (time-series data) |
| `grafana-data` | Grafana state: users, saved panels, alerts |

To inspect or back up:

```bash
podman volume ls
podman volume inspect postgres-data
```

### Updating the application

```bash
cd ~/sports-log
git pull

# Rebuild the image
podman build -t localhost/sports-log:latest .

# Restart the app container (Postgres and monitoring are unaffected)
sudo systemctl restart sports-log
```

Any new migrations are applied automatically on startup.

### Useful operational commands

```bash
# Follow app logs
journalctl -fu sports-log

# Follow all four services
journalctl -fu postgres -fu sports-log -fu prometheus -fu grafana

# Check container status
podman ps

# Open a shell in a running container
podman exec -it sports-log /bin/sh
podman exec -it postgres psql -U sports_log sports_log

# View Quadlet-generated unit files
systemctl cat sports-log
```

---

## Future Work

- **Wire email sending** — implement `lettre` SMTP calls for email verification, password reset, and OTP delivery (currently logged to stdout via `tracing::info!`).
- **Wire SMS** — implement Twilio REST API calls in `auth/otp.rs` for SMS OTP delivery.
- **CSRF protection** — store and validate the OAuth2 state parameter in a short-lived signed cookie in `auth/oauth.rs`.
- **Garmin Connect integration** — OAuth2 PKCE flow against `connect.garmin.com`; store tokens in `oauth_connections`; sync endpoint that fetches recent activities from the Garmin API and upserts them with `source = 'garmin'`.
- **Apple Health** — no backend API exists; a companion iOS app would read HealthKit data and POST to `/activities` with `source = 'apple'`.
- **Rate limiting** — add a Tower middleware layer for global request rate limiting (e.g., `tower_governor`).
- **TLS termination** — put a reverse proxy (Caddy or nginx) in front of the app container to handle HTTPS and automatic certificate renewal via ACME/Let's Encrypt.
- **Structured error codes** — add machine-readable error codes to the JSON error body alongside the human-readable message.
- **Pagination cursors** — replace offset-based pagination on `/activities` with keyset (cursor) pagination for consistent performance at scale.
