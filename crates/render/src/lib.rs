//! Godot frontend for Marrowfall: rendering and input, nothing else.
//!
//! This is the only crate allowed to touch Godot APIs, and only ever from the
//! main thread (most Godot classes are not thread-safe). The simulation
//! (`game`) runs on a thread owned by `host`, and everything crossing that
//! boundary is plain data.

mod bridge;

// Public so the separate unit-test crate can reach them; both are engine-free
// pure logic, which is what keeps `bridge.rs` the only unmeasured file.
pub mod draw;
pub mod iso;

use godot::prelude::*;

struct MarrowfallRender;

#[gdextension]
unsafe impl ExtensionLibrary for MarrowfallRender {}
