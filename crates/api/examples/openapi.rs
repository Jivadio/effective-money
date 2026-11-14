// SPDX-License-Identifier: AGPL-3.0-or-later
//! Prints the OpenAPI document of the API. Used by `cargo xtask openapi`,
//! which writes it to (or checks it against) `apps/web/src/api/openapi.json`.

fn main() {
    print!("{}", solde_api::openapi_json());
}
