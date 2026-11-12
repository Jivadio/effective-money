# syntax=docker/dockerfile:1.7
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Browser helper image. Build it from its own directory, which is its whole
# build context (the helper has no dependency on the rest of the repo):
#
#   docker build -t solde-browser-helper services/browser-helper
#
# The base is the Playwright image: it ships the browsers at the version this
# package pins, and the unprivileged `pwuser` account. The service asks for
# Chromium's process sandbox (`chromiumSandbox: true`), which refuses to start
# as root, so the unprivileged user is part of the contract, not a nicety.

ARG PLAYWRIGHT_VERSION=1.65.0

FROM mcr.microsoft.com/playwright:v${PLAYWRIGHT_VERSION}-noble AS build
WORKDIR /app
COPY package.json package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY tsconfig.json ./
COPY src ./src
RUN npm run build && npm prune --omit=dev

FROM mcr.microsoft.com/playwright:v${PLAYWRIGHT_VERSION}-noble AS runtime
LABEL org.opencontainers.image.title="solde-browser-helper" \
      org.opencontainers.image.description="Local browser helper for Solde bank login flows; knows no bank" \
      org.opencontainers.image.source="https://github.com/Jivadio/solde" \
      org.opencontainers.image.licenses="AGPL-3.0-or-later"

WORKDIR /app
COPY --from=build --chown=pwuser:pwuser /app/package.json /app/package-lock.json ./
COPY --from=build --chown=pwuser:pwuser /app/node_modules ./node_modules
COPY --from=build --chown=pwuser:pwuser /app/dist ./dist
COPY --chown=pwuser:pwuser scripts/healthcheck.mjs ./scripts/healthcheck.mjs

ENV NODE_ENV=production \
    SOLDE_BROWSER_HELPER_PORT=8765

# Chromium's shared memory needs a real /dev/shm; run with `--ipc=host` or a
# large `--shm-size`, otherwise pages fail to render under load.
USER pwuser
EXPOSE 8765
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
  CMD ["node", "scripts/healthcheck.mjs"]
ENTRYPOINT ["node", "dist/src/main.js"]
