//! The character art pipeline behind `cargo art`.
//!
//! Split into a library so the tests in `tests/` can reach it: an
//! integration test cannot import a binary-only crate.

pub mod blender;
pub mod check;
pub mod cli;
pub mod http;
pub mod library;
pub mod lock;
pub mod pack;
pub mod preview;
pub mod providers;
pub mod spec;
pub mod spike;
pub mod stages;
