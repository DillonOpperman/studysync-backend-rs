# Optional image for later deploy phases. Phase 1 development uses `cargo run` on the host.

FROM rust:1-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/studysync-backend /usr/local/bin/studysync-backend
COPY migrations ./migrations
USER 65534:65534
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/studysync-backend"]
