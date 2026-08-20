//! What one square of ground is: what it is made of, how high it sits, and what
//! it stops.
//!
//! Every field is an integer on purpose. Intermediate generation maths may use
//! floats, but nothing float-valued is ever stored, so a one-bit rounding
//! difference on another platform cannot change a world.

/// Which ground material a tile is made of: an index into the material table the
/// tuning data defines.
///
/// An id rather than an enum, so adding a material is a table row and some art
/// rather than a code change. Which pixels it draws as is the frontend's
/// business and is resolved from the same name on that side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MaterialId(pub u8);

/// What a tile stops. Three independent questions, because the answers differ:
/// a knee-high ruin wall stops walking but not a jump or an arrow, and a full
/// wall stops all three.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileFlags(u8);

impl TileFlags {
    /// Open ground.
    pub const NONE: Self = Self(0);
    /// Cannot be stood on.
    pub const BLOCKS_WALK: Self = Self(1 << 0);
    /// Cannot be passed even while airborne. Without this a blocked tile is a
    /// low obstacle, which is what makes jumping over one mean anything.
    pub const BLOCKS_JUMP: Self = Self(1 << 1);
    /// Stops projectiles and sight, so archers on a ledge cannot shoot through
    /// the cliff they stand on.
    pub const BLOCKS_SHOT: Self = Self(1 << 2);

    /// Both sets of flags together.
    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// The raw bits, for hashing a world and for writing a save. Not for
    /// deciding anything: use the named questions below.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    #[must_use]
    pub const fn blocks_walk(self) -> bool {
        self.0 & Self::BLOCKS_WALK.0 != 0
    }

    #[must_use]
    pub const fn blocks_jump(self) -> bool {
        self.0 & Self::BLOCKS_JUMP.0 != 0
    }

    #[must_use]
    pub const fn blocks_shot(self) -> bool {
        self.0 & Self::BLOCKS_SHOT.0 != 0
    }
}

/// One square of ground.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tile {
    pub material: MaterialId,
    /// Surface level in whole steps. Signed, so a pit or a ravine is just a
    /// negative height rather than its own concept.
    pub height: i8,
    pub flags: TileFlags,
}
