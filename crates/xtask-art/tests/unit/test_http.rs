//! The HTTP and environment helpers every provider shares.

use std::time::Duration;

use xtask_art::http::{base_url, millis_from, retry_after, truncate};

use crate::support::EnvGuard;

// --- Rate limiting --------------------------------------------------------

#[test]
fn a_rate_limit_is_waited_out_for_as_long_as_the_server_asks() {
    assert_eq!(retry_after(Some("7")).as_secs(), 7);
    assert_eq!(retry_after(Some("0")), Duration::ZERO);
}

#[test]
fn a_rate_limit_with_no_usable_hint_backs_off_anyway() {
    // RFC 6585 makes Retry-After optional, and RFC 9110 also allows a date.
    assert!(retry_after(None) > Duration::ZERO);
    assert_eq!(
        retry_after(Some("Wed, 21 Oct 2026 07:28:00 GMT")),
        retry_after(None)
    );
}

// --- Reading settings out of the environment ------------------------------

#[test]
fn an_api_root_defaults_to_the_providers_own() {
    let mut env = EnvGuard::new();
    env.remove("MARROWFALL_TEST_BASE_URL");
    assert_eq!(
        base_url("MARROWFALL_TEST_BASE_URL", "https://example.test"),
        "https://example.test"
    );

    env.set("MARROWFALL_TEST_BASE_URL", "http://127.0.0.1:9");
    assert_eq!(
        base_url("MARROWFALL_TEST_BASE_URL", "https://example.test"),
        "http://127.0.0.1:9"
    );
}

#[test]
fn a_poll_interval_is_read_in_milliseconds() {
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_TEST_POLL_MS", "25");
    assert_eq!(
        millis_from("MARROWFALL_TEST_POLL_MS", Duration::from_secs(5)),
        Duration::from_millis(25)
    );
}

/// A typo in a variable nobody but a test sets must not stop the pipeline.
#[test]
fn an_unset_or_unparseable_interval_falls_back() {
    let mut env = EnvGuard::new();
    env.remove("MARROWFALL_TEST_POLL_MS");
    let fallback = Duration::from_secs(5);
    assert_eq!(millis_from("MARROWFALL_TEST_POLL_MS", fallback), fallback);

    env.set("MARROWFALL_TEST_POLL_MS", "soon");
    assert_eq!(millis_from("MARROWFALL_TEST_POLL_MS", fallback), fallback);
}

// --- Quoting a response body ----------------------------------------------

/// Truncation runs while reporting another error, so it must never panic.
#[test]
fn truncate_never_splits_a_character() {
    let text = "é".repeat(500);
    let cut = truncate(&text, 300);
    assert!(cut.ends_with('…'));
    assert_eq!(truncate("short", 300), "short");
}
