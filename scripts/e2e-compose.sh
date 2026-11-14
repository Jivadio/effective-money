#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# End-to-end check of a Docker Compose deployment (AC1), on a blank project:
#
#   scripts/e2e-compose.sh compose.yaml           # PostgreSQL
#   scripts/e2e-compose.sh compose.sqlite.yaml    # SQLite
#
# It starts the stack with `docker compose up -d --build`, then checks that
# /healthz answers within 60 s, that setup needs the one-time setup code
# printed in the server log, that an admin can set up the instance with it,
# sign in, create an account and list it, that the web app is served, that
# the master key never shows in the logs, that `admin export-key` returns the key
# held in the key volume, that a key you provide is used as is, and that the
# server refuses to start without a key. Everything is torn down at the end
# (volumes included).
#
# It uses its own Compose project (`solde-e2e-*`) and port (SOLDE_E2E_PORT,
# default 18080), so it never touches a real instance. Needs docker and curl.

set -euo pipefail

file=${1:?usage: scripts/e2e-compose.sh compose.yaml|compose.sqlite.yaml}
cd "$(dirname "$0")/.."
[ -f "$file" ] || { echo "no such compose file: $file" >&2; exit 2; }

project="solde-e2e-$(basename "$file" .yaml | tr '.' '-')"
port=${SOLDE_E2E_PORT:-18080}
base="http://127.0.0.1:$port"
export SOLDE_PORT=$port
# Never inherit settings meant for a real instance.
unset SOLDE_KEYS_SOURCE SOLDE_BIND_ADDRESS SOLDE_INSECURE_COOKIES SOLDE_TRUSTED_PROXIES

work=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/solde-e2e.XXXXXX")
jar="$work/cookies"
body="$work/body"
headers="$work/headers"

compose() { docker compose -f "$file" -p "$project" "$@"; }

cleanup() {
  compose down -v --remove-orphans >/dev/null 2>&1 || true
  rm -rf "$work"
}
trap cleanup EXIT

step() { printf '==> %s\n' "$*"; }
fail() {
  printf 'FAIL: %s\n' "$*" >&2
  if [ -s "$body" ]; then printf 'last response body: %s\n' "$(cat "$body")" >&2; fi
  compose logs --no-color >&2 || true
  exit 1
}

# api METHOD PATH [JSON]: prints the status code; body in $body.
api() {
  local method=$1 path=$2 data=${3:-}
  local args=(-sS -o "$body" -D "$headers" -w '%{http_code}' -X "$method" -b "$jar" -c "$jar")
  if [ -n "$data" ]; then
    args+=(-H 'Content-Type: application/json' --data "$data")
  fi
  curl "${args[@]}" "$base$path"
}

expect() {
  local want=$1 got=$2 what=$3
  [ "$got" = "$want" ] || fail "$what: expected HTTP $want, got $got"
}

contains() {
  grep -qF -- "$1" "$2" || fail "$3"
}

step "[$file] starting from a blank project ($project)"
compose down -v --remove-orphans >/dev/null 2>&1 || true
compose up -d --build

step "waiting for /healthz (at most 60 s)"
start=$(date +%s)
until [ "$(curl -s -o /dev/null -w '%{http_code}' "$base/healthz" || true)" = 200 ]; do
  if [ $(($(date +%s) - start)) -ge 60 ]; then
    fail "/healthz did not answer 200 within 60 s"
  fi
  sleep 1
done
echo "    healthy after $(($(date +%s) - start)) s"

step "web app and API boundary"
expect 200 "$(api GET /)" "GET /"
contains '<div id="root">' "$body" "GET / is not the web app"
grep -qi "^content-security-policy: .*frame-ancestors 'none'" "$headers" || fail "GET / has no CSP"
expect 404 "$(api GET /api/v1/nope)" "unknown API path"
contains '"code":"not_found"' "$body" "unknown API path is not a JSON 404"

step "first start: set up the instance"
expect 200 "$(api GET /api/v1/meta)" "GET /api/v1/meta"
contains '"bootstrapped":false' "$body" "a blank instance should not be set up"
# The server prints a one-time setup code in its log while not set up.
code=$(compose logs --no-color server 2>/dev/null |
  grep -oE 'setup code: [0-9A-Z]{4}(-[0-9A-Z]{4}){3}' | tail -n 1 | sed 's/^setup code: //') || true
[ -n "$code" ] || fail "no setup code in the server log"
# Built in variables: bash 3.2 (macOS) brace-expands JSON inside "$(...)".
fields='"household_name":"E2E household","name":"Jane Doe","email":"jane@example.test","password":"correct horse battery staple"'
wrong_setup="{\"setup_code\":\"0000-0000-0000-0000\",$fields}"
setup="{\"setup_code\":\"$code\",$fields}"
login='{"email":"jane@example.test","password":"correct horse battery staple"}'
account='{"kind":"checking","name":"E2E checking","currency":"EUR","balance_minor":12345}'
expect 401 "$(api POST /api/v1/setup "$wrong_setup")" "setup with a wrong code"
contains '"code":"invalid_setup_code"' "$body" "a wrong setup code is not refused as such"
status=$(api POST /api/v1/setup "$setup")
case "$status" in 2??) ;; *) fail "POST /api/v1/setup: HTTP $status" ;; esac

step "sign in"
: > "$jar"
expect 200 "$(api POST /api/v1/auth/login "$login")" "login"
grep -q solde_session "$jar" || fail "login set no session cookie"
expect 200 "$(api GET /api/v1/me)" "GET /api/v1/me"

step "create an account and list it"
status=$(api POST /api/v1/accounts "$account")
case "$status" in 2??) ;; *) fail "POST /api/v1/accounts: HTTP $status" ;; esac
expect 200 "$(api GET /api/v1/accounts)" "GET /api/v1/accounts"
contains '"name":"E2E checking"' "$body" "the new account is not listed"

step "admin export-key returns the key from the key volume"
exported=$(compose run --rm -T server admin export-key 2>/dev/null) || fail "admin export-key failed"
# As the server user: the key file is 0400, owned by uid 65532.
stored=$(compose run --rm -T --no-deps --user 65532:65532 --entrypoint cat keygen /keys/master.key 2>/dev/null) ||
  fail "cannot read the key volume"
[ -n "$exported" ] || fail "admin export-key printed nothing"
[ "$exported" = "$stored" ] || fail "admin export-key does not match the key volume"

step "the key never shows in the logs"
compose logs --no-color > "$work/logs" 2>&1
grep -q "keygen: master key generated" "$work/logs" || fail "keygen did not generate a key"
if grep -qF -- "$exported" "$work/logs"; then
  fail "the master key appears in docker compose logs"
fi

step "without a key the server refuses to start, with an actionable message"
if compose run --rm -T --no-deps -e SOLDE_MASTER_KEY_FILE= server > "$work/nokey" 2>&1; then
  fail "the server started without a master key"
fi
contains "no master key" "$work/nokey" "no actionable message without a key"
contains "SOLDE_MASTER_KEY_FILE" "$work/nokey" "the message does not name SOLDE_MASTER_KEY_FILE"

step "a key you provide (SOLDE_KEYS_SOURCE) is used as is"
own="$work/own-keys"
mkdir -m 0755 "$own"
head -c 32 /dev/urandom | base64 > "$own/master.key"
chmod 0444 "$own/master.key"
own_key=$(cat "$own/master.key")
SOLDE_KEYS_SOURCE="$own" compose run --rm -T --no-deps keygen > "$work/keygen" 2>&1 ||
  fail "keygen failed with a provided key"
contains "already present" "$work/keygen" "keygen did not leave the provided key alone"
[ "$(cat "$own/master.key")" = "$own_key" ] || fail "keygen changed the provided key"
exported=$(SOLDE_KEYS_SOURCE="$own" compose run --rm -T --no-deps server admin export-key 2>/dev/null) ||
  fail "admin export-key failed with a provided key"
[ "$exported" = "$own_key" ] || fail "admin export-key does not return the provided key"

step "[$file] all checks passed"
