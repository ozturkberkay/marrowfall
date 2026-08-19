// Test binary: `unwrap()` is the idiomatic assertion here, and a panic is the
// failure report. The workspace forbids it in real code, where a panic is a
// crash.
#![allow(clippy::unwrap_used)]

/// Proves this crate can be tested at all: CI has no Godot installed, so the
/// open question is whether a gdext-linked test binary builds and runs without
/// an engine. It does, because `Vector2` is a plain `#[repr(C)]` Rust struct.
#[test]
fn a_gdext_builtin_is_usable_without_an_engine() {
    assert_eq!(godot::builtin::Vector2::new(3.0, -4.0).x, 3.0);
}
