//! Godot frontend for Marrowfall: rendering and input, nothing else.
//!
//! This is the only crate allowed to touch Godot APIs, and only ever from the
//! main thread (most Godot classes are not thread-safe). The simulation
//! (`game`) runs on a thread owned by `host`, and everything crossing that
//! boundary is plain data.

mod bridge;

// The visual harness Godot loads as a scene, never part of the game itself.
mod pose;

// Public so the separate unit-test crate can reach them. All of these hold pure
// logic, which leaves the two Godot nodes above the only unmeasured files:
// instantiating one needs a running engine, so the e2e tier is what drives
// them.
pub mod draw;
pub mod iso;
pub mod origin;
pub mod tiles;

use godot::prelude::*;

struct MarrowfallRender;

#[gdextension]
unsafe impl ExtensionLibrary for MarrowfallRender {}
