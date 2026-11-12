// SPDX-License-Identifier: AGPL-3.0-or-later
//! Stable transaction ids for banks that provide none.
//!
//! `remote_id = "syn1:" + hex(sha256(booking_date, amount_minor, currency, normalized_label))[..32] + ":" + occurrence_index`
//!
//! `occurrence_index` is the rank of the transaction among its identical twins
//! (same day, amount, currency and normalized label), in the order the bank
//! returned them. Two identical coffees on the same day therefore get `...:0` and
//! `...:1` and are both kept.
//!
//! The index is only meaningful over a COMPLETE booking day: if a day is cut
//! by pagination or is still in progress (today), the same transaction could
//! get a different index on the next sync. Id-less booked transactions dated
//! after `last_complete_day` are therefore dropped; the next sync fetches them
//! again once their day is complete. Callers pass `since` = the start of a full
//! booking day, so the first day is complete too.
//!
//! Ids are unique per account, not globally: the sync engine keys
//! transactions on `(account, remote_id)`, so call this once per account.
//!
//! Pending transactions without a bank id keep `remote_id = None`. On each
//! sync the engine replaces an account's id-less pending transactions as a
//! whole set, and pairs them with their booked version by amount and date.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use sha2::{Digest, Sha256};

use crate::dto::{RemoteTransaction, TransactionStatus};

/// Prefix of synthesized ids (`syn` + algorithm version).
pub const SYNTHETIC_PREFIX: &str = "syn1:";

/// Fills `remote_id` for booked transactions that have none (see the
/// [module docs](self)).
///
/// - Transactions with a bank id are kept unchanged. An empty or
///   whitespace-only id counts as missing.
/// - Pending transactions without a bank id are kept with `remote_id = None`
///   (see the [module docs](self)).
/// - Booked transactions without a bank id dated after `last_complete_day`
///   are dropped (truncated day).
///
/// The input order is preserved.
pub fn synthesize_remote_ids(
    transactions: Vec<RemoteTransaction>,
    last_complete_day: NaiveDate,
) -> Vec<RemoteTransaction> {
    let mut occurrences: BTreeMap<String, usize> = BTreeMap::new();
    transactions
        .into_iter()
        .filter_map(|mut tx| {
            if tx
                .remote_id
                .as_deref()
                .is_some_and(|id| id.trim().is_empty())
            {
                tx.remote_id = None;
            }
            if tx.remote_id.is_some() || tx.status == TransactionStatus::Pending {
                return Some(tx);
            }
            if tx.booking_date > last_complete_day {
                return None;
            }
            let digest = content_hash(&tx);
            let index = occurrences.entry(digest.clone()).or_insert(0);
            tx.remote_id = Some(format!("{SYNTHETIC_PREFIX}{digest}:{index}"));
            *index += 1;
            Some(tx)
        })
        .collect()
}

/// Upper-case, trimmed, inner whitespace collapsed to one space.
pub fn normalize_label(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_uppercase()
}

fn content_hash(tx: &RemoteTransaction) -> String {
    let mut hasher = Sha256::new();
    hasher.update(tx.booking_date.to_string().as_bytes());
    hasher.update([0x1f]);
    hasher.update(tx.amount_minor.to_string().as_bytes());
    hasher.update([0x1f]);
    hasher.update(tx.currency.as_str().as_bytes());
    hasher.update([0x1f]);
    hasher.update(normalize_label(&tx.label).as_bytes());
    hasher
        .finalize()
        .iter()
        .take(16)
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::money::Currency;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 1, d).unwrap()
    }

    fn tx(d: u32, amount: i64, label: &str, status: TransactionStatus) -> RemoteTransaction {
        RemoteTransaction::new(day(d), amount, Currency::EUR, label, status)
    }

    #[test]
    fn twins_get_distinct_stable_ids() {
        use TransactionStatus::*;
        let input = vec![
            tx(16, -100, "TODAY", Booked),
            tx(15, -1250, "CARD PAYMENT SUPERMARKET", Pending),
            tx(14, -350, "CARD PAYMENT  coffee shop", Booked),
            tx(14, -350, "card payment coffee shop ", Booked),
            tx(14, -900, "CARD PAYMENT COFFEE SHOP", Booked),
            tx(16, -1, "WITH ID", Booked).with_remote_id(Some("bank-42".to_owned())),
        ];
        let out = synthesize_remote_ids(input.clone(), day(15));
        // Truncated day dropped, pending kept without id, bank id kept.
        assert_eq!(out.len(), 5);
        assert_eq!(out[0].remote_id, None);
        assert_eq!(out[4].remote_id.as_deref(), Some("bank-42"));
        let (a, b, c) = (
            out[1].remote_id.clone().unwrap(),
            out[2].remote_id.clone().unwrap(),
            out[3].remote_id.clone().unwrap(),
        );
        assert!(a.starts_with(SYNTHETIC_PREFIX) && a.ends_with(":0"), "{a}");
        assert_eq!(a.trim_end_matches(":0"), b.trim_end_matches(":1"));
        assert_ne!(a, c);
        // Deterministic across runs.
        assert_eq!(synthesize_remote_ids(input, day(15)), out);
    }

    #[test]
    fn blank_bank_ids_get_distinct_synthetic_ids() {
        use TransactionStatus::Booked;
        let mut blank = tx(14, -350, "COFFEE", Booked);
        blank.remote_id = Some(" ".to_owned());
        let out = synthesize_remote_ids(vec![blank.clone(), blank], day(15));
        let (a, b) = (
            out[0].remote_id.clone().unwrap(),
            out[1].remote_id.clone().unwrap(),
        );
        assert!(a.starts_with(SYNTHETIC_PREFIX), "{a}");
        assert_ne!(a, b);
    }

    #[test]
    fn last_complete_day_is_included_and_currency_matters() {
        use TransactionStatus::Booked;
        let usd = Currency::new("USD").unwrap();
        let input = vec![
            tx(15, -350, "COFFEE", Booked),
            RemoteTransaction::new(day(15), -350, usd, "COFFEE", Booked),
        ];
        let out = synthesize_remote_ids(input, day(15));
        assert_eq!(
            out.len(),
            2,
            "a transaction dated last_complete_day is kept"
        );
        // Same day, amount and label but another currency: not twins.
        let (a, b) = (
            out[0].remote_id.clone().unwrap(),
            out[1].remote_id.clone().unwrap(),
        );
        assert!(a.ends_with(":0") && b.ends_with(":0"));
        assert_ne!(a, b);
        assert!(synthesize_remote_ids(Vec::new(), day(15)).is_empty());
    }

    #[test]
    fn label_normalization() {
        assert_eq!(normalize_label("  card\tpayment  x "), "CARD PAYMENT X");
    }
}
