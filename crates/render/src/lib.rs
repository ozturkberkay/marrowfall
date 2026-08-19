//! Godot frontend for Marrowfall: rendering and input, nothing else.
//!
//! This is the only crate allowed to touch Godot APIs, and only ever from the
//! main thread (most Godot classes are not thread-safe). The simulation
//! (`game`) runs on a thread owned by `host`, and everything crossing that
//! boundary is plain data.

mod bridge;

use godot::prelude::*;

struct MarrowfallRender;

#[gdextension]
unsafe impl ExtensionLibrary for MarrowfallRender {}
