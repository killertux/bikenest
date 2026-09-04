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
# `tini` is PID 1 so SIGTERM reaches the server (which drains HTTP and lets an
# in-flight background job finish) and zombie children are reaped.
RUN useradd -r -u 1001 bikenest \
    && apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates tini \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# The server binary.
COPY --from=builder /usr/local/bin/bikenest-web /usr/local/bin/bikenest-web

# Static assets, served from STATIC_ROOT (below) rather than any path baked
# into the binary — the binary is relocatable.
COPY --from=builder /app/web/static /app/web/static

# Uploaded media (object storage) lives on a mounted volume.
RUN mkdir -p /app/media && chown -R bikenest:bikenest /app
USER bikenest
ENV MEDIA_ROOT=/app/media
ENV STATIC_ROOT=/app/web/static

EXPOSE 8080

# Graceful shutdown: the runtime sends STOPSIGNAL, tini forwards it, and the
# server drains HTTP then gives the job worker up to 30 s. Allow at least 35 s
# of termination grace (`--stop-timeout` / `terminationGracePeriodSeconds`).
STOPSIGNAL SIGTERM
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/bikenest-web"]
