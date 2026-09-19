//! E2E tier: a black box, driving `godot` over the committed project.
//!
//! **gdext gotcha.** Godot loads the shared library `rust.gdextension` names,
//! and building this test target does not write it: measured on this
//! workspace, `cargo build -p render --tests` refreshes the rlib and leaves
//! the shared library as it was. So [`engine::build_extension`] runs a plain
//! build first. The same staleness is why a running Godot editor has to be
//! restarted after a recompile.

// Test binary: `unwrap()` is the idiomatic assertion here, and a panic is the
// failure report. The workspace forbids it in real code, where a panic is a
// crash.
#![allow(clippy::unwrap_used)]

mod engine;
mod fixture;
mod sheet;
mod test_smoke;
mod test_visual;
