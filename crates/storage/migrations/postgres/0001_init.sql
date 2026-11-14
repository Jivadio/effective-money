-- SPDX-License-Identifier: AGPL-3.0-or-later
-- Solde schema, version 1 (PostgreSQL).
--
-- Mirrored by ../sqlite/0001_init.sql: keep both files in step, same tables,
-- columns, constraints and indexes (`cargo xtask schema-parity` checks it).
-- Style the parity check relies on: one column or table constraint per line,
-- `CREATE TABLE` / `CREATE [UNIQUE] INDEX` statements only.
--
-- Conventions: ids are UUIDv7 (`UUID` here, canonical text in SQLite);
-- timestamps are UTC (`TIMESTAMPTZ`); booking dates are `DATE`, years
-- 0000-9999 only (checked by the store on write); money is `BIGINT` minor
-- units; enumerations are `TEXT` plus `CHECK`, never native enums.

CREATE TABLE schema_meta (
    version BIGINT PRIMARY KEY,
    name TEXT NOT NULL,
    applied_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE households (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE users (
    id UUID PRIMARY KEY,
    household_id UUID NOT NULL REFERENCES households (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    email TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('admin', 'member')),
    password_hash TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE UNIQUE INDEX users_email_key ON users (email);
CREATE INDEX users_household_idx ON users (household_id);

-- Only a hash of the session token is stored, never the token itself.
CREATE TABLE sessions (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE UNIQUE INDEX sessions_token_hash_key ON sessions (token_hash);
CREATE INDEX sessions_user_idx ON sessions (user_id);
CREATE INDEX sessions_expires_idx ON sessions (expires_at);

-- Bearer tokens (desktop app, scripts): hash only.
CREATE TABLE api_tokens (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    token_hash TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ,
    last_used_at TIMESTAMPTZ
);

CREATE UNIQUE INDEX api_tokens_token_hash_key ON api_tokens (token_hash);
CREATE INDEX api_tokens_user_idx ON api_tokens (user_id);

-- Invitations to join a household: hash of the invitation token only.
CREATE TABLE invitations (
    id UUID PRIMARY KEY,
    household_id UUID NOT NULL REFERENCES households (id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('admin', 'member')),
    token_hash TEXT NOT NULL,
    created_by UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    accepted_at TIMESTAMPTZ,
    accepted_by UUID REFERENCES users (id) ON DELETE SET NULL
);

CREATE UNIQUE INDEX invitations_token_hash_key ON invitations (token_hash);
CREATE INDEX invitations_household_idx ON invitations (household_id);

-- Accounts live at household level; who sees them is account_access.
CREATE TABLE accounts (
    id UUID PRIMARY KEY,
    household_id UUID NOT NULL REFERENCES households (id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('checking', 'savings', 'investment', 'life-insurance', 'loan', 'card')),
    name TEXT NOT NULL,
    iban TEXT,
    iban_normalized TEXT,
    currency TEXT NOT NULL CHECK (length(currency) = 3),
    balance_minor BIGINT,
    created_at TIMESTAMPTZ NOT NULL
);

-- Lookup by IBAN within a household. Not unique: members cannot see each
-- other's private accounts, so a unique key would reveal (and let someone
-- squat on) an IBAN they cannot see. Duplicates are refused in services,
-- among the accounts the member can see; bank-synced account identity is
-- matched in M3.
CREATE INDEX accounts_household_iban_idx ON accounts (household_id, iban_normalized) WHERE iban_normalized IS NOT NULL;
CREATE INDEX accounts_household_idx ON accounts (household_id);

-- One grant per (account, member); the strongest permission is the grant.
CREATE TABLE account_access (
    account_id UUID NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    permission TEXT NOT NULL CHECK (permission IN ('owner', 'write', 'read')),
    PRIMARY KEY (account_id, user_id)
);

CREATE INDEX account_access_user_idx ON account_access (user_id);

CREATE TABLE transactions (
    id UUID PRIMARY KEY,
    account_id UUID NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    booking_date DATE NOT NULL,
    value_date DATE,
    amount_minor BIGINT NOT NULL,
    currency TEXT NOT NULL CHECK (length(currency) = 3),
    label TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'booked')),
    remote_id TEXT,
    category_id UUID,
    category_source TEXT NOT NULL CHECK (category_source IN ('rule', 'manual', 'none')),
    created_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX transactions_account_booking_idx ON transactions (account_id, booking_date);
