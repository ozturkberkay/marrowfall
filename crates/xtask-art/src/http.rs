//! HTTP and environment helpers with no vendor in them.
//!
//! A setting's name is vendor specific, so it stays in the provider that
//! owns it. The mechanism that reads the setting is not, so it lives here.

use std::time::Duration;

/// How long to wait out a rate limit that carries no usable `Retry-After`.
const DEFAULT_BACKOFF: Duration = Duration::from_secs(5);

/// An API root, overridable so a test can serve the API locally. Only a test
/// sets one.
pub fn base_url(variable: &str, default: &str) -> String {
    std::env::var(variable).unwrap_or_else(|_| default.to_owned())
}

/// A duration read from the environment, in milliseconds.
///
/// Overridable so a test can exercise a poll loop in milliseconds rather than
/// minutes. Only a test sets one.
pub fn millis_from(variable: &str, fallback: Duration) -> Duration {
    std::env::var(variable)
        .ok()
        .and_then(|ms| ms.parse().ok())
        .map_or(fallback, Duration::from_millis)
}

/// How long to wait after a rate limit (RFC 6585 section 4).
///
/// Seconds only. RFC 9110 also allows an HTTP-date, which is rare enough that
/// the fixed backoff covers it.
pub fn retry_after(header: Option<&str>) -> Duration {
    header
        .and_then(|value| value.trim().parse().ok())
        .map_or(DEFAULT_BACKOFF, Duration::from_secs)
}

/// Shortens a response body for an error message.
pub fn truncate(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        // Slice on a char boundary: this runs while reporting another error,
        // and panicking here would hide it.
        Some((index, _)) => format!("{}…", &text[..index]),
        None => text.to_owned(),
    }
}
