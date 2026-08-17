//! The character art pipeline behind `cargo art`.
//!
//! Split into a library so the tests in `tests/` can reach it: an
//! integration test cannot import a binary-only crate.

pub mod cli;
pub mod library;
pub mod lock;
pub mod meshy;
pub mod openai;
pub mod pack;
pub mod preview;
pub mod spec;
pub mod stages;
