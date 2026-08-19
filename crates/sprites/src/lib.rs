//! The sprite manifest format: one definition, shared by the art pipeline that
//! writes it and the game that reads it.
//!
//! Two definitions of one file format break on the next pipeline change, so
//! these types live here and not in either program. Nothing here is
//! engine-specific and none of it is game logic, so every line is testable
//! without an engine.
//!
//! [`parse`] is the only way in. It checks the invariants the readers below
//! rely on, so a manifest that gets past it draws without a panic path.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Where the sprite touches the ground, in pixels within an atlas cell. The
/// renderer puts this point on the entity's tile, which keeps feet planted
/// while the body bobs.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Anchor {
    pub x: u32,
    pub y: u32,
}

/// Where one frame lives in the atlas, and where those pixels sit inside the
/// untrimmed cell.
///
/// Trimming each frame to its own content keeps the atlas small, and
/// `off_x`/`off_y` keep it safe: a frame draws back at the position it filled
/// before the trim, so the anchor still means the same thing.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FrameRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub off_x: u32,
    pub off_y: u32,
}

/// Layout of one packed animation atlas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimationAtlas {
    /// Atlas filename, relative to the character's asset directory.
    pub file: String,
    /// Direction names in row order.
    pub directions: Vec<String>,
    /// Frames per direction, the column count.
    pub frames: u32,
    /// Playback rate the animation was sampled at.
    pub fps: u32,
    /// Whether playback repeats.
    pub loops: bool,
    /// The untrimmed cell every frame is positioned within. The anchor is
    /// relative to this, not to any trimmed rect.
    pub cell_width: u32,
    pub cell_height: u32,
    pub anchor: Anchor,
    /// One entry per frame, indexed `direction * frames + frame`.
    pub rects: Vec<FrameRect>,
}

/// Everything the game needs to draw one character.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterAssets {
    pub name: String,
    pub animations: BTreeMap<String, AnimationAtlas>,
}

/// Why a manifest is not usable.
#[derive(Debug)]
pub enum Error {
    /// Not a manifest: malformed RON, or the wrong shape of one.
    Syntax(ron::error::SpannedError),
    /// A manifest whose numbers contradict each other.
    Invalid {
        animation: String,
        detail: &'static str,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Syntax(error) => write!(f, "not a sprite manifest: {error}"),
            Self::Invalid { animation, detail } => {
                write!(f, "animation {animation:?} is invalid: {detail}")
            }
        }
    }
}

impl std::error::Error for Error {}

/// Reads a manifest and checks that every animation in it can be drawn.
///
/// This one check at the edge is what makes [`frame_at`] and [`row_for`] total.
/// No caller has to guard against a manifest that contradicts itself.
pub fn parse(text: &str) -> Result<CharacterAssets, Error> {
    let assets: CharacterAssets = ron::from_str(text).map_err(Error::Syntax)?;
    for (animation, atlas) in &assets.animations {
        if let Some(detail) = problem_with(atlas) {
            return Err(Error::Invalid {
                animation: animation.clone(),
                detail,
            });
        }
    }
    Ok(assets)
}

/// Which row of `atlas` holds `direction`, or `None` when it has no such row.
/// That is how a four-direction atlas answers "se". The caller then leaves the
/// sprite on the frame it had.
#[must_use]
pub fn row_for(atlas: &AnimationAtlas, direction: &str) -> Option<usize> {
    atlas.directions.iter().position(|name| name == direction)
}

/// Which frame of `atlas` shows `seconds` into playback.
///
/// Negative seconds clamp to zero, which absorbs the seed snapshot from before
/// the first tick. A looping clip wraps. A clip that does not loop holds its
/// last frame.
#[must_use]
pub fn frame_at(atlas: &AnimationAtlas, seconds: f64) -> usize {
    // `parse` rejects a frameless atlas, but the fields are public. This keeps
    // the modulo and the subtraction below safe either way.
    let frames = atlas.frames.max(1) as usize;
    // A float-to-int cast saturates in Rust, so a nonsense time cannot wrap.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let elapsed = (seconds.max(0.0) * f64::from(atlas.fps)) as usize;
    if atlas.loops {
        elapsed % frames
    } else {
        elapsed.min(frames - 1)
    }
}

/// The frame at `row` and `frame`, or `None` when this atlas has no such cell.
///
/// An `Option`, because a row and frame taken from one atlas can outrun
/// another. A clip change does exactly that when the two atlases differ in
/// direction count or length.
#[must_use]
pub fn frame(atlas: &AnimationAtlas, row: usize, frame: usize) -> Option<&FrameRect> {
    let frames = atlas.frames as usize;
    // `rects` is one flat run, so the column needs its own bounds check. An
    // overrunning frame index otherwise aliases the next row.
    if frame >= frames {
        return None;
    }
    // Saturating, so an absurd row answers `None` instead of wrapping into a
    // cell that does exist.
    atlas
        .rects
        .get(row.saturating_mul(frames).saturating_add(frame))
}

/// The first invariant `atlas` breaks, or `None`. Ordered so the most basic
/// problem is the one reported.
fn problem_with(atlas: &AnimationAtlas) -> Option<&'static str> {
    if atlas.frames == 0 {
        return Some("frames must be at least 1");
    }
    if atlas.fps == 0 {
        return Some("fps must be at least 1");
    }
    if atlas.directions.is_empty() {
        return Some("directions must name at least one row");
    }
    let cells = atlas.directions.len().saturating_mul(atlas.frames as usize);
    if atlas.rects.len() != cells {
        return Some("rects must hold one entry per direction per frame");
    }
    if atlas.anchor.x >= atlas.cell_width || atlas.anchor.y >= atlas.cell_height {
        return Some("anchor must sit inside the cell");
    }
    None
}
