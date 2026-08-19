//! What the frontend draws each frame, and which nodes must exist.
//!
//! Pure functions over plain data, with no `Gd<T>`, so every decision here is
//! unit testable without an engine. `bridge.rs` keeps the property writes.

use std::collections::HashSet;

use game::{EntityView, Locomotion};
use godot::builtin::{Rect2, Vector2};
use sprites::{AnimationAtlas, FrameRect};

/// An animation of the one character everything draws. One atlas each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Clip {
    Idle,
    Run,
}

impl Clip {
    /// Every clip the frontend can show, which is what startup preloads.
    pub const ALL: [Clip; 2] = [Self::Idle, Self::Run];

    /// Which clip a simulation state draws as. This mapping is render policy.
    /// The simulation publishes what he does, never which PNG shows it.
    #[must_use]
    pub fn for_locomotion(locomotion: Locomotion) -> Self {
        match locomotion {
            Locomotion::Idle => Self::Idle,
            Locomotion::Running => Self::Run,
        }
    }

    /// The animation's name in the sprite manifest.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Run => "run",
        }
    }
}

/// Where one frame's pixels are in its atlas, and where to put them relative to
/// the node's origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub region: Rect2,
    pub offset: Vector2,
}

/// Puts a trimmed frame's anchor on the entity's tile.
///
/// The cell's top left goes at minus the anchor, and the frame sits at its own
/// offset inside that cell. `offset` means the top left only with
/// `centered = false`, which is what puts the node origin on the feet.
#[must_use]
pub fn placement(atlas: &AnimationAtlas, rect: &FrameRect) -> Placement {
    Placement {
        region: Rect2::new(
            Vector2::new(rect.x as f32, rect.y as f32),
            Vector2::new(rect.w as f32, rect.h as f32),
        ),
        offset: Vector2::new(
            rect.off_x as f32 - atlas.anchor.x as f32,
            rect.off_y as f32 - atlas.anchor.y as f32,
        ),
    }
}

/// Ids that need a node, and nodes to free.
///
/// Both vectors stay unallocated on a frame where nothing changed, which is
/// every frame once the cast settles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changes {
    pub added: Vec<u64>,
    pub removed: Vec<u64>,
}

/// What to create and what to free, from one snapshot and the ids already
/// drawn.
///
/// Absent from the snapshot means despawned, because every live entity is in
/// every snapshot. A recycled entity slot is safe: hecs packs a generation into
/// the id, so the slot comes back as a different `u64`. That is one removal and
/// one addition, not a node quietly inherited.
#[must_use]
pub fn reconcile(views: &[EntityView], drawn: impl IntoIterator<Item = u64>) -> Changes {
    let drawn: HashSet<u64> = drawn.into_iter().collect();
    let live: HashSet<u64> = views.iter().map(|view| view.id).collect();
    Changes {
        // From `views`, not from the set, so creation order follows the
        // snapshot rather than a hash.
        added: views
            .iter()
            .map(|view| view.id)
            .filter(|id| !drawn.contains(id))
            .collect(),
        removed: drawn.difference(&live).copied().collect(),
    }
}
