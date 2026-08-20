//! Godot frontend for Marrowfall: rendering and input, nothing else.
//!
//! This is the only crate allowed to touch Godot APIs, and only ever from the
//! main thread (most Godot classes are not thread-safe). The simulation
//! (`game`) runs on a thread owned by `host`, and everything crossing that
//! boundary is plain data.

mod bridge;

// Public so the separate unit-test crate can reach them. All of these hold pure
// logic, which leaves `bridge.rs` the only unmeasured file: it is a Godot node,
// and instantiating one needs a running engine.
pub mod draw;
pub mod iso;
pub mod origin;
pub mod tiles;

use godot::prelude::*;

struct MarrowfallRender;

#[gdextension]
unsafe impl ExtensionLibrary for MarrowfallRender {}
