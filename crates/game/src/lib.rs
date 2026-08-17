//! The Marrowfall simulation: the entire game, engine-agnostic. Never depends
//! on Godot. A frontend calls [`Sim::new`], [`Sim::tick`] once per
//! [`TICK_DT`], then [`Sim::snapshot`].
//!
//! Determinism: no wall clock, no OS randomness, no I/O. Identical seeds and
//! intent streams must replay identical worlds.

pub mod components;

mod sim;
mod snapshot;
mod terrain;

pub use sim::{Sim, TICK_DT, TICK_HZ};
pub use snapshot::{EntityView, RenderSnapshot};
pub use terrain::{GROUND_VARIANTS, TerrainGrid};

/// A player input the simulation can act on. Uninhabited until the first
/// gameplay milestone; frontends already pass `&[]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {}
