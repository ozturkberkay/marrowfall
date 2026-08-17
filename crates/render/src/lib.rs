//! Godot frontend for Marrowfall: rendering and input, nothing else.
//!
//! This is the only crate allowed to touch Godot APIs, and only ever from
//! the main thread (most Godot classes are not thread-safe). The simulation
//! (`game` crate) runs on a dedicated thread — see [`sim_thread`] — and all
//! communication crosses that boundary as plain data.

mod bridge;
pub mod sim_thread;

use godot::prelude::*;

struct MarrowfallRender;

#[gdextension]
unsafe impl ExtensionLibrary for MarrowfallRender {}
