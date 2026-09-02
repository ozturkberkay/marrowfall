//! One module per vendor, and the only place a vendor is named in an
//! address.
//!
//! A provider module owns everything that identifies its vendor: the host,
//! the endpoint paths, the credential, and any fixed id the vendor assigned.
//! It may not reach into another provider, and it may not decide when it
//! runs. [`crate::stages`] and [`crate::cli`] are the callers, and the
//! helpers with no vendor in them live in [`crate::http`].
//!
//! One crossing goes the other way, and is accepted: [`crate::lock`] names
//! [`meshy::Endpoint`] to record where a task's result is polled from. It
//! predates this boundary, and T13 is the next task to touch `lock.rs`.
//!
//! Adding a vendor means adding a module here and a
//! [`MotionSource`](crate::library::MotionSource) variant. A unit test greps
//! `src/` and fails on a vendor host, id or key outside this directory.

pub mod meshy;
pub mod mixamo;
pub mod openai;
