//! Godot frontend for Marrowfall: rendering and input, nothing else.
//!
//! This is the only crate allowed to touch Godot APIs, and only ever from the
//! main thread (most Godot classes are not thread-safe). The simulation
//! (`game`) runs on a thread owned by `host`, and everything crossing that
//! boundary is plain data.

mod bridge;

// Public so the separate unit-test crate can reach them. Both hold pure logic
// with no engine in it, which leaves `bridge.rs` the only unmeasured file.
pub mod draw;
pub mod iso;

use godot::prelude::*;

struct MarrowfallRender;

#[gdextension]
unsafe impl ExtensionLibrary for MarrowfallRender {}
