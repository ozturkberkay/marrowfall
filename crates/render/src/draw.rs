//! What the frontend draws each frame, and which nodes must exist. Measuring
//! the cursor against the survivor is here too, because it needs the same fact:
//! the node origin is the sprite's feet.
//!
//! Pure functions over plain data, with no `Gd<T>`, so every decision here is
//! unit testable without an engine. `bridge.rs` keeps the property writes.

use std::collections::HashSet;

use game::{EntityView, Locomotion, Vec2};
use godot::builtin::{Rect2, Vector2};
use sprites::{AnimationAtlas, FrameRect};

/// An animation of the one character everything draws. One atlas each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Clip {
    Idle,
    Run,
    WalkBack,
    StrafeLeft,
    StrafeRight,
}

impl Clip {
    /// Every clip the frontend can show, which is what startup preloads.
    pub const ALL: [Clip; 5] = [
        Self::Idle,
        Self::Run,
        Self::WalkBack,
        Self::StrafeLeft,
        Self::StrafeRight,
    ];

    /// Which clip a simulation state draws as. This mapping is render policy.
    /// The simulation publishes what he does, never which PNG shows it.
    #[must_use]
    pub fn for_locomotion(locomotion: Locomotion) -> Self {
        match locomotion {
            Locomotion::Idle => Self::Idle,
            Locomotion::Forward => Self::Run,
            Locomotion::Backward => Self::WalkBack,
            Locomotion::StrafeLeft => Self::StrafeLeft,
            Locomotion::StrafeRight => Self::StrafeRight,
        }
    }

    /// The animation's name in the sprite manifest.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Run => "run",
            Self::WalkBack => "walk_back",
            Self::StrafeLeft => "strafe_left",
            Self::StrafeRight => "strafe_right",
        }
    }

    /// Whether this clip is a stride, so it plays on the shared gait cycle
    /// rather than at its own authored rate. Exhaustive on purpose: an attack
    /// clip has to answer this too.
    #[must_use]
    pub fn is_locomotion(self) -> bool {
        match self {
            Self::Idle => false,
            Self::Run | Self::WalkBack | Self::StrafeLeft | Self::StrafeRight => true,
        }
    }

    /// This clip if baked, else the run as a stand-in, else `None`. Feet
    /// pushing the wrong way read better than a body frozen mid step. Nothing
    /// stands in for `idle` or for the run itself.
    #[must_use]
    pub fn with_art(self, has_art: impl Fn(Self) -> bool) -> Option<Self> {
        if has_art(self) {
            return Some(self);
        }
        if !self.is_locomotion() {
            return None;
        }
        has_art(Self::Run).then_some(Self::Run)
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

/// Half his baked height: the middle of his body, above the feet he stands on.
pub const BODY_MIDDLE: f32 = 120.0;

/// Dead zone around his body. Small, or it would swallow the tile in front of
/// him and a near enemy could not be pointed at.
pub const DEAD_RADIUS: f32 = 24.0;

/// Where the cursor sits relative to his body, or zero when it is too close to
/// mean a direction. The camera centres his feet, so the dead zone is lifted
/// onto his body.
#[must_use]
pub fn cursor_offset(mouse: Vector2, viewport: Vector2) -> Vector2 {
    // Up the screen is negative y.
    let body = viewport / 2.0 - Vector2::new(0.0, BODY_MIDDLE);
    let offset = mouse - body;
    if offset.length() < DEAD_RADIUS {
        return Vector2::ZERO;
    }
    offset
}

/// Which atlas row shows `aim`. Quantised in tile space, where the ring is even.
/// Row 0 is south. Render-only, so `atan2` is fine.
#[must_use]
pub fn row_for_aim(atlas: &AnimationAtlas, aim: Vec2) -> Option<usize> {
    let count = atlas.directions.len();
    if count == 0 || aim == Vec2::ZERO {
        return None;
    }
    let step = std::f32::consts::TAU / count as f32;
    let angle = aim.y.atan2(aim.x) - std::f32::consts::FRAC_PI_4;
    let index = (angle / step).round() as i32;
    Some(index.rem_euclid(count as i32) as usize)
}

/// One stride, in seconds: `run`'s 20 frames at 24 fps. Every locomotion clip
/// plays over this cycle, so the legs do not jump when the clip changes.
pub const GAIT_SECONDS: f64 = 20.0 / 24.0;

/// Which frame of `atlas` shows `seconds` into the shared gait cycle.
///
/// Only for a stride: `idle` is not one and keeps `sprites::frame_at`, which
/// plays a clip at its authored rate. Every stride loops, so this always wraps.
#[must_use]
pub fn locomotion_frame(atlas: &AnimationAtlas, seconds: f64) -> usize {
    // `sprites::parse` rejects a frameless atlas, but the fields are public.
    let frames = atlas.frames.max(1) as usize;
    // Negative seconds clamp to zero, which absorbs the seed snapshot from
    // before the first tick.
    let phase = (seconds.max(0.0) / GAIT_SECONDS).fract();
    // A float-to-int cast saturates in Rust, so a nonsense time cannot wrap.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let frame = (phase * frames as f64) as usize;
    // A phase of exactly one would otherwise index past the last frame.
    frame.min(frames - 1)
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
