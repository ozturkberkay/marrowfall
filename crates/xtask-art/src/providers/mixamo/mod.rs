//! Mixamo: free motion, exported one clip at a time.
//!
//! [`client`] calls the API. [`session`] reads the credential that API needs
//! out of Chrome, which is where a browser login leaves it.

pub mod client;
pub mod session;

pub use client::Client;

/// The site a human logs in to, and the origin Chrome stores the session
/// under. No trailing slash: `session` builds the Local Storage key prefix
/// from this, and Chrome keys that by bare origin, so a slash finds nothing.
pub const SITE_URL: &str = "https://www.mixamo.com";
