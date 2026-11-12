# Security

This app is designed to hold bank credentials, so we take security reports seriously, and we're grateful when you take the time to send one.

## Reporting a problem

Please use GitHub's **private vulnerability reporting** (Security tab, then "Report a vulnerability") rather than a public issue, so we can fix things before they're widely known. And please leave real credentials and personal financial data out of your report.

## What to expect from us

We aim to reply within 72 hours and to fix critical problems within 14 days. We'll keep you posted, credit you if you wish, and publish an advisory once a fix is out.

## Supported versions

There is no release yet. For now, `main` gets best-effort fixes. Once we ship, the latest minor version will be supported.

We also run GitHub CodeQL code scanning on every PR and weekly, as an extra pair of eyes.

## What we care most about

- Anything that could leak bank credentials, session data or the master key (logs, API or MCP responses, backups, error messages).
- Weaknesses in encryption or key handling.
- One household member seeing another's private accounts, or bypassing access rules.
- MCP token scope bypasses, and prompt injection through bank-provided text.
- Risky dependencies.

Out of scope: a bank changing or blocking its private API, and attacks that already have full code execution on the server (the key lives in memory by design, see [docs/security-model.md](docs/security-model.md)).
