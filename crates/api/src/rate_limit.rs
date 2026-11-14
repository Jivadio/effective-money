// SPDX-License-Identifier: AGPL-3.0-or-later
//! In-memory sliding-window rate limit for login-like attempts: per client
//! address, plus one ceiling across all clients.
//!
//! - IPv6 clients are counted per /64, the block one subscriber usually
//!   gets, so cycling through addresses of one block does not help.
//! - At most [`MAX_TRACKED`] addresses are tracked. When the map is still
//!   full after dropping idle entries, new addresses are refused rather
//!   than tracked, so a flood of addresses cannot grow memory without bound.
//! - The global ceiling bounds the total, whatever the number of addresses.
//!
//! Kept in the process: one server instance per household, and a restart
//! only resets the counters.

use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, Ipv6Addr};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// Most addresses tracked at once.
const MAX_TRACKED: usize = 10_000;

pub(crate) struct RateLimiter {
    limit: usize,
    global_limit: usize,
    window: Duration,
    max_tracked: usize,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    per_client: HashMap<IpAddr, VecDeque<Instant>>,
    all: VecDeque<Instant>,
}

impl RateLimiter {
    /// At most `limit` attempts per `window` and client, and `global_limit`
    /// per `window` in total.
    pub(crate) fn new(limit: usize, global_limit: usize, window: Duration) -> Self {
        Self {
            limit,
            global_limit,
            window,
            max_tracked: MAX_TRACKED,
            state: Mutex::new(State::default()),
        }
    }

    /// Counts one attempt from `ip`, or refuses it with the time to wait.
    pub(crate) fn check(&self, ip: IpAddr) -> Result<(), Duration> {
        self.check_at(ip, Instant::now())
    }

    fn check_at(&self, ip: IpAddr, now: Instant) -> Result<(), Duration> {
        let key = bucket(ip);
        let window = self.window;
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let State { per_client, all } = &mut *state;

        expire(all, now, window);
        if all.len() >= self.global_limit {
            return Err(wait(all, now, window));
        }
        if per_client.len() >= self.max_tracked && !per_client.contains_key(&key) {
            per_client.retain(|_, times| {
                times
                    .back()
                    .is_some_and(|last| now.duration_since(*last) < window)
            });
            if per_client.len() >= self.max_tracked {
                return Err(window);
            }
        }
        let times = per_client.entry(key).or_default();
        expire(times, now, window);
        if times.len() >= self.limit {
            return Err(wait(times, now, window));
        }
        times.push_back(now);
        all.push_back(now);
        Ok(())
    }
}

/// The key an address is counted under: itself for IPv4, its /64 for IPv6.
fn bucket(ip: IpAddr) -> IpAddr {
    match ip.to_canonical() {
        IpAddr::V6(v6) => {
            let prefix = u128::from(v6) & !((1_u128 << 64) - 1);
            IpAddr::V6(Ipv6Addr::from(prefix))
        }
        v4 => v4,
    }
}

fn expire(times: &mut VecDeque<Instant>, now: Instant, window: Duration) {
    while times
        .front()
        .is_some_and(|first| now.duration_since(*first) >= window)
    {
        times.pop_front();
    }
}

fn wait(times: &VecDeque<Instant>, now: Instant, window: Duration) -> Duration {
    times.front().map_or(window, |first| {
        window.saturating_sub(now.duration_since(*first))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> IpAddr {
        text.parse().expect("ip")
    }

    #[test]
    fn limits_per_address_and_window() {
        let limiter = RateLimiter::new(5, 1000, Duration::from_secs(60));
        let start = Instant::now();
        let (a, b) = (ip("192.0.2.1"), ip("192.0.2.2"));
        for _ in 0..5 {
            assert!(limiter.check_at(a, start).is_ok());
        }
        let wait = limiter.check_at(a, start).expect_err("sixth refused");
        assert_eq!(wait, Duration::from_secs(60));
        assert!(
            limiter.check_at(b, start).is_ok(),
            "other address unaffected"
        );
        let later = start + Duration::from_secs(60);
        assert!(limiter.check_at(a, later).is_ok(), "window slid");
    }

    #[test]
    fn ipv6_counts_per_64() {
        let limiter = RateLimiter::new(2, 1000, Duration::from_secs(60));
        let start = Instant::now();
        assert!(limiter.check_at(ip("2001:db8:1:2::1"), start).is_ok());
        assert!(limiter.check_at(ip("2001:db8:1:2::ffff"), start).is_ok());
        assert!(
            limiter.check_at(ip("2001:db8:1:2:abcd::9"), start).is_err(),
            "same /64"
        );
        assert!(
            limiter.check_at(ip("2001:db8:1:3::1"), start).is_ok(),
            "next /64"
        );
        // IPv4-mapped addresses count as their IPv4 address.
        assert_eq!(bucket(ip("::ffff:192.0.2.7")), ip("192.0.2.7"));
    }

    #[test]
    fn global_ceiling() {
        let limiter = RateLimiter::new(5, 3, Duration::from_secs(60));
        let start = Instant::now();
        for n in 1..=3 {
            assert!(limiter.check_at(ip(&format!("192.0.2.{n}")), start).is_ok());
        }
        assert!(limiter.check_at(ip("192.0.2.9"), start).is_err());
        let later = start + Duration::from_secs(60);
        assert!(limiter.check_at(ip("192.0.2.9"), later).is_ok());
    }

    #[test]
    fn refuses_new_addresses_when_full() {
        let mut limiter = RateLimiter::new(5, 1000, Duration::from_secs(60));
        limiter.max_tracked = 2;
        let start = Instant::now();
        assert!(limiter.check_at(ip("192.0.2.1"), start).is_ok());
        assert!(limiter.check_at(ip("192.0.2.2"), start).is_ok());
        assert!(
            limiter.check_at(ip("192.0.2.3"), start).is_err(),
            "full, nothing idle"
        );
        assert!(
            limiter.check_at(ip("192.0.2.1"), start).is_ok(),
            "known address"
        );
        let later = start + Duration::from_secs(60);
        assert!(
            limiter.check_at(ip("192.0.2.3"), later).is_ok(),
            "idle entries pruned"
        );
    }
}
