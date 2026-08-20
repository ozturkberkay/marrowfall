//! Render-facing view of one simulation tick. These types are the sim→render
//! boundary protocol: plain copyable data, no engine handles, no references
//! back into the world.
//!
//! Each view carries both ends of the tick it describes, because the
//! simulation is the only party that knows which two ticks are adjacent: a
//! frontend pairing entities across snapshots would interpolate over a
//! multi-tick gap after a dropped frame.
//!
//! These types may describe their own data, including the domain it is valid
//! over. They may not decide how a frontend draws it: no pixels, no screen
//! space, no depth ordering.
//!
//! Every live entity is in every snapshot, so absent means despawned and a
//! frontend may free its node. Culling is render-only and must never drop one.
//! Safe against id reuse, because hecs packs a generation into the id. The cost
//! is one `Vec` per tick: when zones stream the answer is one snapshot per
//! active zone, and if the allocation ever bites, buffer reuse. Neither is
//! deltas.
//!
//! One-tick facts (a hit, a death, an impact) do not belong here. Latest-wins
//! drops snapshots by design, so an event in a superseded one is lost. Those
//! take a bounded channel drained once per frame, dropped and counted when
//! full because they are cosmetic. A fact belongs here only if a frontend
//! could still see it two snapshots later.

use crate::WorldVec;
use crate::components::Facing;

/// What a character does. Simulation state, not a clip filename, so it is still
/// correct at a wall: a held key there produces no motion, and a frontend that
/// reads displacement shows idle.
///
/// Two variants, because two exist. Dodge, stagger and airborne extend this.
/// The first state that a velocity cannot answer turns this into a component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locomotion {
    Idle,
    Running,
}

/// One drawable entity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityView {
    /// Stable identifier; frontends key scene objects on it across snapshots.
    pub id: u64,
    /// World-space position in tile units at this tick.
    pub pos: WorldVec,
    /// Position when this tick began. Equal to `pos` for an entity that
    /// spawned or held still, so it draws in place instead of sliding.
    pub prev_pos: WorldVec,
    /// Which way it looks. Kept across a stop, so it never snaps back to a
    /// default when an entity stands still.
    pub facing: Facing,
    /// What it does. Which clip that picks is the frontend's decision.
    pub locomotion: Locomotion,
    /// The height of the ground it stands on, in whole steps.
    ///
    /// Here rather than derived by the frontend, because the frontend may not
    /// hold the chunk the simulation used, and a character drawn at the wrong
    /// height stands in the air or sinks into a plateau.
    pub height: i8,
}

impl EntityView {
    /// Draw position `alpha` of the way through this tick: `0` is
    /// [`Self::prev_pos`], `1` is [`Self::pos`].
    ///
    /// `alpha` is clamped because this view describes exactly one tick and
    /// knows nothing outside it. A frontend whose simulation thread fell
    /// behind will ask for more than a tick, and gets the newest known
    /// position instead of an invented one.
    #[must_use]
    pub fn lerp(&self, alpha: f64) -> WorldVec {
        self.prev_pos.lerp(self.pos, alpha.clamp(0.0, 1.0))
    }
}

/// Everything a frontend needs to draw one tick.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RenderSnapshot {
    /// Tick this snapshot describes.
    pub tick: u64,
    /// Simulation time in seconds at that tick.
    pub time: f64,
    pub entities: Vec<EntityView>,
    /// [`EntityView::id`] of the entity that held input drives. Only the
    /// simulation knows which one that is, and a frontend needs it to aim a
    /// camera.
    pub player: Option<u64>,
}
