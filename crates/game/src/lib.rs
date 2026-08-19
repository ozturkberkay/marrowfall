//! The Marrowfall simulation: the entire game, engine-agnostic. Never depends
//! on Godot. A frontend calls [`Sim::new`], [`Sim::tick`] once per
//! [`TICK_DT`], then [`Sim::snapshot`].
//!
//! Determinism: no wall clock, no OS randomness, no I/O. Identical seeds and
//! intent streams must replay identical worlds.

mod components;
mod sim;
mod snapshot;
mod terrain;

pub use components::Facing;
pub use sim::{Sim, Spawn, TICK_DT, TICK_HZ};
pub use snapshot::{EntityView, RenderSnapshot};
pub use terrain::{GROUND_VARIANTS, TerrainGrid};

/// Re-exported because it appears in the boundary protocol: frontends must be
/// able to name it without picking their own `glam` version.
pub use glam::Vec2;

/// A player input the simulation can act on. Uninhabited until the first
/// gameplay milestone; frontends already pass `&[]`.
///
/// Input splits in two when it lands, and the split is what keeps movement
/// independent of frame rate. Held state (movement, cursor) is latest-wins
/// through a second triple buffer, written once per frame and read once per
/// tick. Discrete actions stay reliable `Intent`s on the existing channel. One
/// reliable message per frame would deliver 2.4 per tick at 144 fps and half of
/// one at 30, making speed a function of frame rate.
/// Target: `Sim::tick(&mut self, input: Input, intents: &[Intent])`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {}
