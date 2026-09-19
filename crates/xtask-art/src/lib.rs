//! The character art pipeline behind `cargo art`.
//!
//! Split into a library so the tests in `tests/` can reach it: an
//! integration test cannot import a binary-only crate.

pub mod blender;
pub mod check;
pub mod cli;
pub mod conform;
pub mod glb;
pub mod godot;
pub mod http;
pub mod library;
pub mod lock;
pub mod pack;
pub mod posture;
pub mod preview;
pub mod providers;
pub mod spec;
pub mod spike;
pub mod stages;

/// Where an external tool is: the path its own `MARROWFALL_*_BIN` variable
/// names, or the bare name for `PATH` to resolve. One function, so Blender,
/// Godot and Bun are all found the same way and a test can stub any of them.
pub fn tool_binary(variable: &str, default: &str) -> String {
    std::env::var(variable).unwrap_or_else(|_| default.to_owned())
}
