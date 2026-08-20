//! The chunks the simulation currently holds, and what it can answer from them.
//!
//! Terrain streams, so at any moment the simulation knows about a moving window
//! of the world and nothing outside it. Everything that reads the ground goes
//! through here, so there is one answer to "what is at this tile" and one place
//! that knows a tile might not be loaded yet.
//!
//! Ordered by coordinate rather than hashed. Hash map iteration order is
//! randomly seeded per process, so anything that walked one and affected the
//! world would replay differently every run.

use std::collections::BTreeMap;
use std::sync::Arc;

use worldgen::{ChunkCoord, ChunkView, IVec2, Tile};

/// The resident window of the world.
#[derive(Debug, Default)]
pub struct Chunks {
    resident: BTreeMap<ChunkCoord, Arc<ChunkView>>,
}

impl Chunks {
    /// Takes a generated chunk, replacing any earlier copy of it.
    ///
    /// An `Arc` because the frontend holds the same chunk to paint it, and
    /// generating it twice would be the same work done twice.
    pub fn insert(&mut self, view: Arc<ChunkView>) {
        self.resident.insert(view.coord, view);
    }

    /// Drops a chunk. Silent if it was never held, so an eviction can be sent
    /// without first checking.
    pub fn remove(&mut self, coord: ChunkCoord) {
        self.resident.remove(&coord);
    }

    /// How many chunks are held. For tests and for reporting.
    #[must_use]
    pub fn len(&self) -> usize {
        self.resident.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.resident.is_empty()
    }

    /// The tile at a world coordinate, or `None` when its chunk is not resident.
    ///
    /// `None` means "not known yet", not "empty". Callers must not read it as
    /// open ground: a streaming gap would then be a hole the player falls into.
    #[must_use]
    pub fn tile(&self, tile: IVec2) -> Option<Tile> {
        let coord = ChunkCoord::of(tile);
        let view = self.resident.get(&coord)?;
        view.tile(tile - coord.origin())
    }
}

/// How many height steps a character may climb or fall in one step.
///
/// Symmetric on purpose. If falls were unlimited while climbs were capped, the
/// generator could produce a basin the player walks into and can never leave,
/// which in a game with permadeath and no ground editing is unrecoverable.
pub const STEP_LIMIT: i8 = 1;

impl Chunks {
    /// Whether an entity standing on `from` may step to `to`.
    ///
    /// `to` unknown means refused: a chunk that has not arrived is not open
    /// ground, and treating it as such would let the player walk into a void.
    /// `from` unknown is allowed, because the tile underfoot may have been
    /// evicted while the player stands on it, and refusing would freeze him
    /// permanently.
    #[must_use]
    pub fn can_step(&self, from: IVec2, to: IVec2) -> bool {
        let Some(destination) = self.tile(to) else {
            return false;
        };
        if destination.flags.blocks_walk() {
            return false;
        }
        let standing = self.tile(from).unwrap_or(destination);
        // `i16`, so the subtraction cannot overflow at the ends of the height
        // range even though both sides are `i8`.
        let climb = i16::from(destination.height) - i16::from(standing.height);
        if climb.abs() > i16::from(STEP_LIMIT) {
            return false;
        }
        // A diagonal must clear both orthogonal neighbours, or an entity slips
        // through the corner where two cliffs meet. Movement resolves one axis
        // at a time so it never asks a diagonal, but pathfinding will.
        if from.x != to.x && from.y != to.y {
            return self.can_step(from, IVec2::new(to.x, from.y))
                && self.can_step(from, IVec2::new(from.x, to.y));
        }
        true
    }
}
