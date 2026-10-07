# StudySync backend (Rust)

This is the **new** production API. It is a separate Git repository from the legacy Flask app in `StudySync/studysync-backend`.

Do not copy this project into the old Flask tree. Do not push it to `DillonOpperman/studysync-backend`.

This repository currently implements Phases 1–5: configuration, tracing, SQLx migrations, PostgreSQL, health/ready, authentication, users, profiles, groups, friends/DMs/blocks, and persistent group chat with authenticated WebSockets. Matching, media uploads, and DM WebSockets are later phases.

## Prerequisites (Windows)

1. [Docker Desktop](https://www.docker.com/products/docker-desktop/) running (Linux containers).
2. [Rust](https://rustup.rs/) stable (`rustc --version` should work in PowerShell or Git Bash).
3. SQLx CLI (for migrations):

**PowerShell**

```powershell
cargo install sqlx-cli --no-default-features --features rustls,postgres
```

**Git Bash**

```bash
cargo install sqlx-cli --no-default-features --features rustls,postgres
```

Ensure `%USERPROFILE%\.cargo\bin` is on your `PATH`.

## Start infrastructure

From this repository root:

**PowerShell**

```powershell
cd E:\Desktop\studysync-backend-rs
docker compose up -d
docker compose ps
```

**Git Bash**

```bash
cd /e/Desktop/studysync-backend-rs
docker compose up -d
docker compose ps
```

Wait until `studysync-postgres`, `studysync-redis`, and `studysync-minio` are healthy. The object-storage service creates the `studysync-media` development bucket on first start.

The Compose service is named `minio` and exposes an S3-compatible API on port 9000. If `minio/minio` cannot be pulled (Docker Hub/Quay 401), this repo uses Adobe S3Mock as a local stand-in. Production still uses S3-compatible object storage.

Confirm pgvector inside Postgres:

**PowerShell**

```powershell
docker compose exec postgres psql -U studysync -d studysync -c "CREATE EXTENSION IF NOT EXISTS vector; SELECT extname FROM pg_extension;"
```

(The SQLx migration also enables `citext` and `vector`.)

## Configure the API

**PowerShell**

```powershell
Copy-Item .env.example .env
```

**Git Bash**

```bash
cp .env.example .env
```

Edit `.env` and set a unique `JWT_SECRET` (32+ characters). Do not commit `.env`.

## Run migrations

Migrations are **explicit**. The API does not change the schema on boot.

```powershell
sqlx migrate run
```

Git Bash: the same command. `sqlx` reads `DATABASE_URL` from `.env`.

## Run the API

```powershell
cargo run
```

The process listens on `0.0.0.0:8080`.

**Health** (process alive; does not need Postgres beyond a successful start):

```powershell
curl.exe http://127.0.0.1:8080/api/v1/health
```

Expected: `{"status":"ok"}` and HTTP 200.

**Ready** (Postgres reachable):

```powershell
curl.exe http://127.0.0.1:8080/api/v1/ready
```

Expected when Postgres is up: `{"status":"ready","postgres":"ok"}` and HTTP 200.

If Postgres is down after the process already started, `/api/v1/ready` returns HTTP 503:

```json
{"status":"notReady","postgres":"unavailable"}
```

Git Bash: `curl http://127.0.0.1:8080/api/v1/health`

## Tests

Unit tests (in `src/`, plus HTTP tests that use a closed-port lazy pool) do **not** require Redis or a live PostgreSQL instance.

Integration tests in `tests/auth.rs`, `tests/profile.rs`, and the live `/ready` success check **require** a reachable PostgreSQL. They **fail** (they do not skip) if `DATABASE_URL` is missing or the database is down.

1. Start Compose and run migrations (`sqlx migrate run`).
2. Export `DATABASE_URL`.
3. Run the full suite:

```powershell
$env:DATABASE_URL = "postgres://studysync:studysync_dev_only_change_me@127.0.0.1:5432/studysync?sslmode=disable"
cargo fmt
cargo clippy -- -D warnings
cargo test
```

Git Bash:

```bash
export DATABASE_URL="postgres://studysync:studysync_dev_only_change_me@127.0.0.1:5432/studysync?sslmode=disable"
cargo test
```

Lib-only unit tests (no live Postgres):

```powershell
cargo test --lib
```

## Availability model

Weekly slots store `day_of_week` (0 = Monday … 6 = Sunday), `start_time`, `end_time`, and an IANA timezone name (validated in Rust; PostgreSQL keeps `text`).

Same-day windows have `end_time > start_time`. Overnight windows (for example 23:00 → 01:00) are stored as **one row** with `end_time < start_time`. Matching must expand those as `[start, 24:00)` on `day_of_week` and `[00:00, end)` on the next day. Zero-length windows (`end == start`) are rejected.

## Profile pictures

`profilePictureKey` is a future object-storage key. It is not authorization. The media phase must own uploads, bind keys to the user, and issue signed URLs. Clients must not present arbitrary keys as proof of access.

## Friendships (Phase 4)

Accepted friendships are stored as one undirected row in `friendships (user_low, user_high)` with `user_low < user_high`. A→B and B→A cannot become two friendships. `are_friends(a, b)` is symmetric.

Pending friend requests use a unique index on `LEAST/GREATEST(requester_id, recipient_id)` so only one pending pair exists. Rejected requests can be sent again.

Direct messages are allowed when users are friends, share an active study group, or have an accepted message request (either direction). Otherwise, messaging an active group leader creates a **message request** (HTTP 202) instead of a DM. Blocking (`POST /api/v1/blocks`) is bidirectional for social actions: friend requests, DMs, and message requests fail with `user_blocked`. Blocked conversations cannot be listed.

## Group chat (Phase 5)

PostgreSQL is the source of truth. WebSocket delivery is best-effort.

- **REST:** `GET/POST /api/v1/groups/:groupId/messages`, `PATCH/DELETE /api/v1/messages/:id`, `POST /api/v1/messages/:id/reactions`.
- **WebSocket:** `GET /api/v1/ws?access_token=<jwt>` (query-string auth for React Native). Invalid or expired tokens are rejected before upgrade. Tokens are never logged.
- **History:** cursor pagination with `?before=<messageId>` (older) or `?after=<messageId>` (newer / reconnect recovery). Default page is the newest messages, returned **oldest-to-newest**. Limit defaults to 50 (max 100). UUID v7 ids are time-ordered cursors.
- **Idempotency:** `clientMsgId` unique per `(group_id, sender_id)`. Retries return the original canonical message.
- **Limits:** text/announcement bodies max **4000** characters. In-process per-connection rate limit: 20 sends and 60 ops per 10s. Multi-instance rate limiting will use Redis later.
- **Edit:** sender only, within 15 minutes. **Delete:** sender or current leader; soft tombstone (`deleted_at`, empty content).
- **Announcements:** current leader only. Other members get an in-app `group_announcement` notification. Ordinary text messages are not fanned out as notifications.
- **Hub:** in-memory rooms for a single API process. Bounded outbound queue (64). Slow clients are disconnected. Heartbeat: JSON `ping`/`pong`; idle sockets close after 90s.
- **Blocks:** members may still persist group messages; live events are not delivered to users who have a block with the actor, and blocked users cannot react to each other's messages. Re-check membership on every send (subscribe is not a permanent grant).
- **Media:** `message_attachments` exists for a later phase. `image`/`file` types are rejected until then.
- **Scaling (later):** Instance A/B/C → Redis pub/sub → local hub fan-out. Redis is not used for chat in this phase.

## Groups (Phase 3)

`POST /api/v1/groups` creates a group; the caller becomes the sole active leader. Recurring slots use the same overnight rules as personal availability (`end < start` wraps midnight).

Leave policy: members may leave (membership is archived). A leader with other active members cannot leave until leadership is transferred (not implemented yet). A leader who is the only remaining member may leave; the group is **archived**, not deleted.

Capacity: join approval locks the `study_groups` row (`FOR UPDATE`), then counts active members before inserting. Concurrent approvals cannot exceed `maxMembers`.

## Logout

`POST /api/v1/auth/logout` revokes **that refresh session / device** only. Repeating logout with the same token is a no-op success. It is not a global sign-out.

## Stop infrastructure

```powershell
docker compose down
```

Volumes persist until you run `docker compose down -v`.

## Later-phase crates

These will be added when needed: `redis`, `aws-sdk-s3` (or equivalent), `fastembed`, `pgvector` (Rust crate).

## Security notes

- No hardcoded JWT secrets or database passwords in source.
- `.env` is gitignored.
- HTTP errors never include SQL, paths, backtraces, or connection strings.
- There is no `/api/debug/token` (or any other debug auth route).
