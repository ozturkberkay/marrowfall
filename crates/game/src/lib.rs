//! The Marrowfall simulation: the entire game, engine-agnostic. Never depends
//! on Godot. A frontend calls [`Sim::new`], [`Sim::tick`] once per
//! [`TICK_DT`], then [`Sim::snapshot`].
//!
//! Determinism: no wall clock, no OS randomness, no I/O. Identical seeds and
//! intent streams must replay identical worlds.

mod chunks;
mod components;
mod sim;
mod snapshot;

pub use chunks::{Chunks, STEP_LIMIT};
pub use components::Facing;
pub use sim::{PLAYER_SPEED, Sim, Spawn, TICK_DT, TICK_HZ};
pub use snapshot::{EntityView, Locomotion, RenderSnapshot};

/// A position or displacement in the world, in tile units.
///
/// `f64`, because the world outlives `f32`. Screen coordinates grow about 107
/// pixels per tile, so by roughly 20 km an `f32` can no longer place a sprite to
/// the nearest quarter pixel, and the frontier is past that. Minecraft and
/// Unreal 5 both landed here for the same reason.
///
/// An alias rather than a bare re-export, so a frontend names this instead of
/// the concrete type and the width can change again without touching callers.
pub type WorldVec = glam::DVec2;

/// A direction, always at or near unit length, which is what [`Input`] carries.
///
/// Stays `f32`: a direction is never large, so it has nothing to gain from the
/// extra width. Re-exported because it appears in the boundary protocol, so a
/// frontend must be able to name it without picking its own `glam` version.
pub use glam::Vec2;

/// Everything a player holds down this tick.
///
/// Latest-wins across the boundary: written once per frame, read once per tick.
/// So a skipped tick loses no held state, and speed never tracks frame rate.
/// One reliable message per frame delivers 2.4 per tick at 144 fps and half of
/// one at 30.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Input {
    /// Private so it cannot be built out of range: see [`Input::new`].
    move_dir: Vec2,
}

impl Input {
    /// A direction in tile space, at most unit length.
    ///
    /// A direction that is not finite becomes still. One longer than unit
    /// length is scaled back.
    ///
    /// This is a trust boundary against a malformed frontend, not input
    /// shaping. Making diagonals the same speed as cardinals is the frontend's
    /// job, because only it knows the projection the keys point along.
    #[must_use]
    pub fn new(move_dir: Vec2) -> Self {
        if !move_dir.is_finite() {
            // A non-finite position spreads through every later tick and makes
            // a snapshot unequal to itself, which breaks any replay comparison.
            return Self::default();
        }
        Self {
            move_dir: move_dir.clamp_length_max(1.0),
        }
    }

    /// Which way the player asks to move, in tile units.
    #[must_use]
    pub fn move_dir(self) -> Vec2 {
        self.move_dir
    }
}

/// A discrete one-shot action, reliable across the boundary. Uninhabited until
/// something needs one. Frontends already pass `&[]`.
///
/// Held state is not an `Intent`. It travels as [`Input`], latest-wins, which
/// keeps movement independent of frame rate. Attack, dodge and jump land here,
/// where every message matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {}
