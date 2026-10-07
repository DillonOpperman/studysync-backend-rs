# StudySync backend (Rust)

This is the **new** production API. It is a separate Git repository from the legacy Flask app in `StudySync/studysync-backend`.

Do not copy this project into the old Flask tree. Do not push it to `DillonOpperman/studysync-backend`.

Phase 1 provides configuration, tracing, SQLx migrations, a PostgreSQL pool, and `/api/v1/health` plus `/api/v1/ready`. Authentication, users, chat, matching, and media APIs are not implemented yet.

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

`cargo test` does **not** require Redis. The readiness failure test uses a lazy PostgreSQL pool pointed at a closed port.

The live readiness success test runs only when `DATABASE_URL` points at a reachable database; otherwise it skips.

```powershell
cargo fmt
cargo clippy -- -D warnings
cargo test
```

With Compose running:

```powershell
$env:DATABASE_URL = "postgres://studysync:studysync_dev_only_change_me@127.0.0.1:5432/studysync?sslmode=disable"
cargo test
```

Git Bash:

```bash
export DATABASE_URL="postgres://studysync:studysync_dev_only_change_me@127.0.0.1:5432/studysync?sslmode=disable"
cargo test
```

## Stop infrastructure

```powershell
docker compose down
```

Volumes persist until you run `docker compose down -v`.

## Later-phase crates (not in Phase 1)

These will be added when needed: `jsonwebtoken`, `argon2`, `bcrypt`, `redis`, `aws-sdk-s3` (or equivalent), `fastembed`, `pgvector` (Rust crate), `validator`.

## Security notes

- No hardcoded JWT secrets or database passwords in source.
- `.env` is gitignored.
- HTTP errors never include SQL, paths, backtraces, or connection strings.
- There is no `/api/debug/token` (or any other debug auth route).
