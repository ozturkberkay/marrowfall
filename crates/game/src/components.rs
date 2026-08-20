//! ECS components: plain data only. Systems live with the code that runs
//! them, not here.

use crate::WorldVec;

/// Where an entity is, in tile units (1.0 = one tile edge), and where it was
/// when the current tick began.
///
/// `previous` is simulation state, not a render convenience: swept collision
/// and threshold crossing read it too, and every reader must run after the
/// tick has carried it forward.
///
/// Anything that *jumps* `current` outside integration (teleport, knockback,
/// collision snap-out) must assign `previous` to match, or the entity
/// interpolates across the whole jump for one tick.
///
/// A shortened move is the opposite case, and it must leave `previous` alone.
/// The field clamp writes a nearer `current` on the same move. If `previous`
/// moved with it, the motion that facing reads is gone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position {
    pub current: WorldVec,
    pub previous: WorldVec,
}

impl Position {
    /// Starts an entity at rest, with both ends of its first tick at `at`.
    #[must_use]
    pub fn new(at: WorldVec) -> Self {
        Self {
            current: at,
            previous: at,
        }
    }
}

/// Tile units per second, integrated into [`Position`] once per tick.
///
/// Its presence is what marks an entity as moving. Entities without it are
/// skipped by integration and drawn where they stand.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Velocity(pub WorldVec);

/// Marks the entity that held input drives.
///
/// A marker and not an id on [`crate::Sim`], so the world stays the one source
/// of truth and nothing needs clearing on despawn. A possessed entity or a
/// second local player needs no change here.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Player;

/// Which way an entity looks, one of eight, named for the screen direction it
/// points.
///
/// Simulation state, not something a frontend derives: an entity that stops
/// still faces somewhere, and cones, backstabs and line of sight will read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Facing {
    South,
    SouthEast,
    East,
    NorthEast,
    North,
    NorthWest,
    West,
    SouthWest,
}

/// tan(22.5 degrees): half of one 45 degree sector.
const SECTOR_EDGE: f64 = 0.414_213_562_373_095_1;

/// -1, 0 or 1: the sign of `value`, or 0 when it is short beside `other`.
fn quantised_sign(value: f64, other: f64) -> i8 {
    if value.abs() <= other.abs() * SECTOR_EDGE {
        0
    } else if value > 0.0 {
        1
    } else {
        -1
    }
}

/// Indexed `(y + 1) * 3 + (x + 1)`. The centre is unreachable: a zero
/// direction returns before this, and one sign is 0 only if both are.
const BY_SIGN: [Facing; 9] = [
    Facing::North,
    Facing::NorthEast,
    Facing::East,
    Facing::NorthWest,
    Facing::South,
    Facing::SouthEast,
    Facing::West,
    Facing::SouthWest,
    Facing::South,
];

impl Facing {
    /// Which way `direction` points, or `None` when it points nowhere, which
    /// leaves the caller's existing facing alone.
    ///
    /// Quantised in tile space, where all eight sectors are equal 45 degree
    /// wedges. On screen they are not, so a frontend must never quantise a
    /// screen angle. Comparisons only, so the result is bit-identical
    /// everywhere.
    pub(crate) fn from_direction(direction: WorldVec) -> Option<Self> {
        if direction == WorldVec::ZERO {
            return None;
        }
        let x = quantised_sign(direction.x, direction.y);
        let y = quantised_sign(direction.y, direction.x);
        Some(BY_SIGN[((y + 1) * 3 + (x + 1)) as usize])
    }

    /// The compass name the art pipeline gives this direction's atlas row.
    ///
    /// Here and not in a frontend, so these names are not spelled out a third
    /// time. The pipeline's packer and its Blender bake already hold a copy.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::South => "s",
            Self::SouthEast => "se",
            Self::East => "e",
            Self::NorthEast => "ne",
            Self::North => "n",
            Self::NorthWest => "nw",
            Self::West => "w",
            Self::SouthWest => "sw",
        }
    }
}
