# Production image for the BikeNest server (crates/web → `bikenest-web`).
#
# Runtime-checked SQL queries (no compile-time `sqlx::query!` macros) mean the
# build needs NO database: `cargo build` is self-contained.
#
#   docker build -t bikenest .
#
# See docs/deployment.md for the full runbook (secrets as env, TLS, migration
# step, health checks, rollback).

# --- Builder ---------------------------------------------------------------
FROM rust:1.95 AS builder
WORKDIR /app

COPY . .

# Build only the server binary in release mode. Cargo.lock is committed; the
# base image name pins the toolchain so builds are reproducible.
RUN cargo build --release --locked -p bikenest-web \
    && mv /app/target/release/bikenest-web /usr/local/bin/bikenest-web

# --- Runtime ---------------------------------------------------------------
# slim runtime: the binary is self-contained (templates + migrations are
# embedded by Askama / sqlx::migrate!). Only static assets are read from disk.
FROM debian:bookworm-slim
RUN useradd -r -u 1001 bikenest \
    && apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# The server binary.
COPY --from=builder /usr/local/bin/bikenest-web /usr/local/bin/bikenest-web

# Static assets must land at the path baked into the binary at compile time:
# `env!("CARGO_MANIFEST_DIR")/../../web/static` = /app/web/static here.
COPY --from=builder /app/web/static /app/web/static

# Uploaded media (object storage) lives on a mounted volume.
RUN mkdir -p /app/media && chown -R bikenest:bikenest /app
USER bikenest
ENV MEDIA_ROOT=/app/media

EXPOSE 8080
CMD ["bikenest-web"]
