# Fake Bank fixtures

Hand-written, entirely fictional responses of the imaginary Fake Bank API
(`https://fake-bank.invalid`). No real person, account or transaction.
They follow the fixture format documented in `solde_connector_sdk::http`.

Each directory is a scenario; tests load `day1/` and then overlay another
directory on top of it (`FixtureSet::from_dirs`).

| Directory | Scenario |
|---|---|
| `day1/` | First sync, on 2026-01-15 (tests fix the clock to that date): session check, login with SMS code, three accounts, two pages of checking transactions (identical twins on 2026-01-13, one pending, one transaction on the still-open day), savings, and an investment account with two positions. |
| `day2/` | Next day (2026-01-16): the pending supermarket payment is now booked (new date and label), the bakery day is complete, a new open-day transaction appears. |
| `expired/` | The saved session is no longer valid (`401` on `/api/session`). |
| `auth-failed/` | Wrong username or password (`401` on `/api/login`). |
| `empty-page/` | The checking account has no transaction at all. |
| `looping/` | Broken pagination: page 1 points to page 1 again. |
| `bad-currency/` | The bank returns an unknown currency code. |
| `otp-rejected/` | The SMS code is refused (`401` on `/api/login/otp`). |

The session token and the `SID` cookie in `day1/03-login-otp.json` contain the
test canary `CANARY-7f3a9c`: tests check it never reaches the logs.
Replay is strict: each fixture answers once, in file-name order.
