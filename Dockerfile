# syntax=docker/dockerfile:1.7
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Multi-stage build of the effective-money server (apps/server).
# For now the image only serves /healthz; the web app will be embedded later.

ARG RUST_VERSION=1.97.1

FROM rust:${RUST_VERSION}-slim-trixie AS build
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked --bin em-server \
    && cp target/release/em-server /em-server \
    && mkdir -p /rootfs/data

# Distroless, non-root (uid/gid 65532), no shell.
FROM gcr.io/distroless/cc-debian13:nonroot AS runtime
LABEL org.opencontainers.image.title="effective-money" \
      org.opencontainers.image.description="Self-hosted household finance server" \
      org.opencontainers.image.source="https://github.com/Jivadio/effective-money" \
      org.opencontainers.image.licenses="AGPL-3.0-or-later"
COPY --from=build /em-server /usr/local/bin/em-server
# Writable data directory for the SQLite variant (named volumes copy this
# ownership on first use).
COPY --from=build --chown=65532:65532 /rootfs/data /data
USER nonroot:nonroot
ENV EM_LISTEN_ADDR=0.0.0.0:8080
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/em-server"]
