//! The floating origin: what keeps screen coordinates small however far the
//! player has walked.
//!
//! Godot places nodes with `f32`. Screen coordinates grow about 107 pixels per
//! tile, so at roughly 20 km out they pass two million pixels and an `f32` can
//! no longer resolve a quarter of a pixel: tiles and sprites snap to a
//! coarsening grid and visibly jitter. The world's frontier is past that point,
//! so this is not a theoretical limit.
//!
//! The fix is the standard one, used by Kerbal Space Program, Star Citizen and
//! Unity's own guidance: draw everything relative to a point near the player and
//! move that point when the player wanders. Unreal 5 took the other route and
//! widened its own vectors, which is not available here because `Vector2` is
//! Godot's.
//!
//! Pure, and no Godot type appears in it, so all of it is unit tested.

use game::WorldVec;

/// How many chunk widths the player may drift from the origin before it moves.
///
/// Rebasing is not free: every node already drawn has to be repositioned, so a
/// jittery origin would repaint the world constantly. One chunk of slack means
/// walking a straight line rebases once per chunk crossed, and pacing back and
/// forth across a border does not thrash.
const SLACK_CHUNKS: f64 = 1.0;

/// Tiles along one chunk edge, as the frontend sees it.
const CHUNK_TILES: f64 = worldgen::CHUNK_TILES as f64;

/// The point everything is drawn relative to.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Origin {
    /// In whole chunks, so the origin lands on a chunk corner and a rebase moves
    /// it by an exact number of tiles.
    chunk: (i64, i64),
}

impl Origin {
    /// An origin on the chunk containing `tile`.
    #[must_use]
    pub fn at(tile: WorldVec) -> Self {
        Self {
            chunk: (
                (tile.x / CHUNK_TILES).floor() as i64,
                (tile.y / CHUNK_TILES).floor() as i64,
            ),
        }
    }

    /// The world tile this origin sits on.
    #[must_use]
    pub fn tile(self) -> WorldVec {
        WorldVec::new(
            self.chunk.0 as f64 * CHUNK_TILES,
            self.chunk.1 as f64 * CHUNK_TILES,
        )
    }

    /// Moves the origin if `tile` has drifted too far, and reports whether it
    /// moved.
    ///
    /// A `true` means every position already handed to Godot is now expressed
    /// against the wrong origin, so the caller must repaint. That is the whole
    /// cost of this mechanism, and why the slack exists.
    pub fn follow(&mut self, tile: WorldVec) -> bool {
        let offset = tile - self.tile();
        let limit = CHUNK_TILES * (SLACK_CHUNKS + 1.0);
        if offset.x.abs() <= limit && offset.y.abs() <= limit {
            return false;
        }
        let moved = Self::at(tile);
        let changed = moved != *self;
        *self = moved;
        changed
    }
}
