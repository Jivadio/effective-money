# S5 — One SQLx store for PostgreSQL and SQLite

*Spike S5, milestone M2. Status: done, 2026-11-13.*

**Verdict: go for A1.** One generic `Store` implementation over SQLx runs the
same SQL on PostgreSQL 16 and SQLite, with a short, explicit list of
per-backend pieces. Plan B (SeaORM) is not needed.

## The question

ADR-0001 picked option A1: a single repository generic over `sqlx::Database`,
written in a portable SQL subset, with mirrored migrations for each backend.
The risk was that "generic over SQLx" would drown in trait bounds or need so
many per-backend overrides that two implementations would have been simpler.
The spike had to show, on real queries, how much SQL can be written once.

## What was built

The real `crates/storage`, not a throwaway: the M2 schema (`schema_meta`,
households, users, sessions, API tokens, invitations, accounts with a partial
unique index on `(household_id, iban_normalized)`, account access, manual
transactions) and a `Store` trait with 34 methods covering it. That is well
past the "10 representative queries plus one partial unique index" the plan
asked for: inserts, lookups, conditional updates, `ON CONFLICT ... DO UPDATE`,
`INSERT ... SELECT ... JOIN`, `UPDATE ... RETURNING`, `LIMIT $N`, cascading
deletes and multi-statement transactions.

Contract tests (`crates/storage/tests/contract.rs`) run every method on SQLite
in memory, on a SQLite file in WAL mode, and on PostgreSQL 16 (when
`SOLDE_TEST_DATABASE_URL` is set, as in CI). All pass on all three.

## What is written once

Every query. In practice the portable subset was wider than expected:

- `$1`, `$2`, ... placeholders work on both (SQLx maps `$N` for SQLite).
- `ON CONFLICT (...) DO UPDATE SET x = excluded.x` is the same syntax on both,
  so the upsert needed **no** override, contrary to the plan's guess.
- `RETURNING`, partial unique indexes (`... WHERE iban_normalized IS NOT
  NULL`), `COALESCE`, `COUNT(*)` decoded as `i64`, `LIMIT $N` with an `i64`
  bind: identical.
- Dates go through `chrono::NaiveDate` (`DATE` in PostgreSQL, `YYYY-MM-DD`
  text in SQLite) with no special casing.
- Schema conventions from the plan held up: `TEXT` plus `CHECK` instead of
  native enums, money as `BIGINT`/`INTEGER` minor units, no triggers.

## What needed an override

All of it lives in `crates/storage/src/overrides.rs`, behind a small
`Dialect` trait implemented for `Postgres` and `Sqlite`:

| Override | PostgreSQL | SQLite | Why |
|---|---|---|---|
| Id column | `UUID` (`uuid::Uuid`) | canonical hyphenated `TEXT` (`uuid::fmt::Hyphenated`) | SQLite has no UUID type; SQLx would store `Uuid` as a blob |
| Timestamp column | `TIMESTAMPTZ` | fixed-width text `YYYY-MM-DDTHH:MM:SS.ffffffZ` | SQLx writes SQLite timestamps with a variable number of fraction digits, so text comparison (`expires_at > $1`) would be wrong |
| Migration files | `migrations/postgres/` | `migrations/sqlite/` | column types differ (by design) |
| "Does `schema_meta` exist?" | `information_schema.tables` | `sqlite_master` | no shared catalog |
| Write lock (migrations, bootstrap) | `pg_advisory_xact_lock` | none | SQLite has a single writer connection already |
| Rows affected | `PgQueryResult::rows_affected` | `SqliteQueryResult::rows_affected` | `Database::QueryResult` has no generic accessor |
| Connection setup | pool of 10 | WAL, `busy_timeout=5000`, `foreign_keys=ON`, one writer, read-only readers | backend tuning |

The write lock is load-bearing: with it removed, eight concurrent `bootstrap`
calls on PostgreSQL created eight households in every run; with it, exactly
one. The contract test `concurrent_bootstrap` keeps it that way.

Timestamps are truncated to microseconds before writing, so a value reads back
identical on both backends.

## Costs and caveats

- **Trait bounds.** The generic impl needs one `where` clause listing the plain
  types it binds and decodes (`i64`, `String`, `NaiveDate` and their
  `Option`s). Rust does not carry such bounds over from a trait, so the clause
  appears on both impl blocks of `SqlStore`. Ids and timestamps hide behind
  `Dialect` associated types, whose bounds *are* carried over. Verbose, but
  written once and never touched by new queries.
- **No compile-time checking.** `query!` needs one concrete database, so it
  is out. The contract tests are the safety net: each `Store` method has a
  test that runs on every backend, and a new method needs one.
- **Statements must be `&'static str`.** SQLx 0.9 refuses dynamic SQL unless
  wrapped in `AssertSqlSafe`; shared column lists are macros so `concat!` can
  build each statement at compile time.
- **Schema drift.** Two migration folders can drift. `cargo xtask
  schema-parity` parses both and fails on any difference in tables, columns
  (type mapped to its SQLite mirror, then constraints word for word) or
  indexes; it runs in CI. A unit test also checks every migration file is
  embedded in the binary.
- **Bundled SQLite advisory.** `sqlx-sqlite` 0.9.0, the latest release, caps
  `libsqlite3-sys` below 0.39, so it bundles SQLite 3.53, affected by
  RUSTSEC-2026-0459 (stale memory when opening a database with a crafted `-wal`
  file of another page size). Exploiting it needs someone who can already
  write Solde's data directory, so it is ignored in `deny.toml` and
  `.cargo/audit.toml` with that reasoning, until an SQLx release lifts the
  cap.

## Decisions taken along the way

- **Q-DATE:** booking and value dates are stored for years 0000 to 9999 only;
  anything else is refused on write with a typed error
  (`StoreError::DateOutOfRange`). That keeps SQLite's date text fixed-width
  (text order is date order) and both backends on the same range.
- **Schema version:** `schema_meta` records each applied migration. Opening a
  database whose version is newer than the binary fails with
  `StoreError::SchemaTooNew` and changes nothing.
- **PostgreSQL tests** use `SOLDE_TEST_DATABASE_URL` (a `postgres:16` service
  in the CI `test` job) rather than testcontainers; each test creates and
  drops its own schema. Without the variable they print why and pass.

## Try it

```sh
docker run -d --rm --name solde-pg-test -e POSTGRES_PASSWORD=test \
  -e POSTGRES_DB=solde_test -p 55432:5432 postgres:16-alpine
SOLDE_TEST_DATABASE_URL=postgres://postgres:test@127.0.0.1:55432/solde_test \
  cargo nextest run -p solde-storage
cargo xtask schema-parity
docker stop solde-pg-test
```
