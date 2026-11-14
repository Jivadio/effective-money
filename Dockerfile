# syntax=docker/dockerfile:1.7
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Multi-stage build of the Solde server (apps/server) with the web app
# (apps/web): build the web app, build the server, then copy both into a
# distroless, non-root runtime image.

ARG NODE_VERSION=24.22.3
ARG RUST_VERSION=1.97.1

# 1. Web app: static files in /src/apps/web/dist.
FROM node:${NODE_VERSION}-trixie-slim AS web
WORKDIR /src
# Corepack installs the pnpm version pinned by `packageManager`.
ENV COREPACK_ENABLE_DOWNLOAD_PROMPT=0
RUN corepack enable
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY apps/web/package.json apps/web/
RUN --mount=type=cache,target=/root/.local/share/pnpm/store \
    pnpm install --frozen-lockfile --filter @solde/web
COPY apps/web apps/web
RUN pnpm --dir apps/web build

# 2. Server binary.
FROM rust:${RUST_VERSION}-slim-trixie AS build
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked --bin solde-server \
    && cp target/release/solde-server /solde-server \
    && mkdir -p /rootfs/data

# 3. Runtime. Distroless, non-root (uid/gid 65532), no shell.
FROM gcr.io/distroless/cc-debian13:nonroot AS runtime
LABEL org.opencontainers.image.title="solde" \
      org.opencontainers.image.description="Self-hosted household finance server" \
      org.opencontainers.image.source="https://github.com/Jivadio/solde" \
      org.opencontainers.image.licenses="AGPL-3.0-or-later"
COPY --from=build /solde-server /usr/local/bin/solde-server
# The server reads these files into memory at startup (SOLDE_WEB_DIR).
COPY --from=web /src/apps/web/dist /usr/share/solde/web
# Writable data directory for the SQLite variant (named volumes copy this
# ownership on first use).
COPY --from=build --chown=65532:65532 /rootfs/data /data
USER nonroot:nonroot
ENV SOLDE_LISTEN_ADDR=0.0.0.0:8080 \
    SOLDE_WEB_DIR=/usr/share/solde/web
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/solde-server"]
