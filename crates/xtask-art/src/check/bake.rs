//! The bake gates: what the rendered frames must be before packing sees them.
//!
//! Five of the seven rules are read here, off the PNGs and the spec, because
//! a PNG needs no Blender and CI has none. The other two need the scene:
//! [`SAMPLED_FRAMES_ARE_KEYS`] reads the clip's own action and
//! [`LANDMARK_GOLDEN`] projects joints through the bake camera, so
//! `bake_sprites.py` measures those and this module only publishes them.
//!
//! Content is whatever `pack::content_bounds` calls content, at
//! `pack::OPAQUE`, the one threshold the packer trims on. Two definitions of
//! a silhouette would let a frame pass here and be cropped differently there.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::profile::Profile;
use super::{Comparison, Finding, Rule};
use crate::pack::{OPAQUE, Rect, content_bounds};

const RENDERED: &str = "the PNGs one clip left in the staging directory, against the rectangle \
                        of directions by the frame count of its widest direction";
const COVERAGE: &str = "the pixels of each rendered frame of one clip at or above alpha 1, as a \
                        percent of its own canvas, worst frame";
const INSET: &str = "the gap between the content of each rendered frame of one clip and the \
                     nearest canvas border, worst frame";
const REFLECTED: &str = "each rendered frame of one clip against the same frame half a turn \
                         around, both content spans reflected about the canvas center, worst pair";
const PATCH: &str = "spec.bake.forearm_roll, the hand-derived patch the world-space transfer \
                     replaces";
const SAMPLED: &str = "the frames the bake renders, against the frames the clip's own action \
                       keys";
const PROJECTED: &str = "every joint of the rig at three sampled frames, projected through the \
                         bake camera to whole pixels, against the committed golden, the wider of \
                         the two axes of the worst joint";

/// Whether the bake rendered every frame of every direction.
///
/// A count of what is absent, not of what is there: the sampled frame count is
/// Blender's own, and re-deriving it here would be a second copy of
/// `framing.sampled_frames`. What the rendered set must be is a full
/// rectangle, and [`SAMPLED_FRAMES_ARE_KEYS`] is what holds the sampling
/// itself to the action.
pub const FRAME_COUNT: Rule = Rule {
    id: "bake.frame_count",
    comparison: Comparison::Eq,
    unit: "missing frames",
    space: RENDERED,
    limit: |_| 0.0,
};

/// Whether every frame drew the character at all.
///
/// A blank frame is what a bake that lost its camera, its lights or its mesh
/// leaves behind, and packing gives it a slot and ships it.
pub const NON_EMPTY: Rule = Rule {
    id: "bake.non_empty",
    comparison: Comparison::Ge,
    unit: "percent",
    space: COVERAGE,
    limit: |profile| profile.bake.frame_coverage_percent,
};

/// Whether the camera framed every pose it rendered.
///
/// Content touching a border is a limb cut off, and packing cannot repair it:
/// the crop is already missing the pixels.
pub const IN_FRAME: Rule = Rule {
    id: "bake.in_frame",
    comparison: Comparison::Ge,
    unit: "pixels",
    space: INSET,
    limit: |profile| profile.bake.in_frame_pixels,
};

/// Whether the character spins about the middle of the canvas.
///
/// An orthographic camera centered on the axis of rotation maps a point at
/// world `x` to the mirror of where it maps it half a turn around, so the two
/// content spans of opposite directions reflect about the canvas center
/// exactly, whatever the pose. That identity is the pivot: break it and every
/// sprite of that direction sits off its own tile.
pub const PIVOT: Rule = Rule {
    id: "bake.pivot",
    comparison: Comparison::Le,
    unit: "pixels",
    space: REFLECTED,
    limit: |profile| profile.bake.pivot_pixels,
};

/// Whether anything still asks for the forearm patch.
///
/// The transfer aims both rigs at one table of world directions, so a
/// palms-forward bind pose is corrected by the fit rather than by rolling two
/// bones afterwards. An error rather than a warning because the roll runs
/// after `clip.swing` has measured the clip, where nothing else can see it.
pub const FOREARM_ROLL: Rule = Rule {
    id: "bake.forearm_roll",
    comparison: Comparison::Eq,
    unit: "degrees",
    space: PATCH,
    limit: |_| 0.0,
};

/// Whether every rendered frame is a frame the clip actually keys.
///
/// There is no divisibility rule between the sprite rate and the clip's own
/// rate, and none is wanted: what matters is that a rendered frame is an
/// authored pose rather than an interpolation of two.
pub const SAMPLED_FRAMES_ARE_KEYS: Rule = Rule {
    id: "bake.sampled_frames_are_keys",
    comparison: Comparison::Eq,
    unit: "frames",
    space: SAMPLED,
    limit: |_| 0.0,
};

/// Where every joint landed on screen, against the pixels a human signed off.
///
/// The deterministic half of the art review: the contact sheet is the
/// judgment half. A missing golden is an error and never an auto-accept, and
/// `MARROWFALL_UPDATE_GOLDENS=1` is the only thing that rewrites one.
pub const LANDMARK_GOLDEN: Rule = Rule {
    id: "bake.landmark_golden",
    comparison: Comparison::Le,
    unit: "pixels",
    space: PROJECTED,
    limit: |profile| profile.bake.landmark_pixels,
};

/// Every bake rule, in the order `--list-rules` prints them.
pub const RULES: [&Rule; 7] = [
    &FRAME_COUNT,
    &NON_EMPTY,
    &IN_FRAME,
    &PIVOT,
    &FOREARM_ROLL,
    &SAMPLED_FRAMES_ARE_KEYS,
    &LANDMARK_GOLDEN,
];

/// The two `bake_sprites.py` reports, whose limits the runner publishes to it
/// on argv beside the two `clip::BAKE_RULES` ones.
pub const BLENDER_RULES: [&Rule; 2] = [&SAMPLED_FRAMES_ARE_KEYS, &LANDMARK_GOLDEN];

/// And the five [`check_files`] reads off the rendered PNGs and the spec.
pub const FILE_RULES: [&Rule; 5] = [&FRAME_COUNT, &NON_EMPTY, &IN_FRAME, &PIVOT, &FOREARM_ROLL];

/// One clip's rendered frames, as the bake left them on disk.
pub struct Rendered<'a> {
    pub name: &'a str,
    /// The staging directory the frames were written to.
    pub dir: &'a Path,
    /// The direction ring, in the order the bake turned through it.
    pub directions: &'a [&'a str],
}

/// Runs every rule this module owns: [`FILE_RULES`], over the rendered frames
/// of each clip and over the one spec field.
///
/// An unreadable or absent frame is an error under every rule that owns it,
/// never a skip: a gate that goes quiet on absent input proves nothing.
pub fn check_files(
    clips: &[Rendered<'_>],
    character: &str,
    forearm_roll: f64,
    profile: &Profile,
    attempt: u32,
) -> Vec<Finding> {
    let mut findings = vec![forearm_roll_patch(
        character,
        forearm_roll,
        profile,
        attempt,
    )];
    for clip in clips {
        let read = Set::read(clip);
        findings.push(read.frame_count(clip, profile, attempt));
        findings.push(read.non_empty(clip, profile, attempt));
        findings.push(read.in_frame(clip, profile, attempt));
        findings.push(read.pivot(clip, profile, attempt));
    }
    findings
}

/// Every subject the bake owes a finding on, beyond the per-axis ones
/// `clip::BAKE_RULES` own: the character, each clip, and each golden.
pub fn subjects(names: &[&str], character: &str, goldens: &[&str]) -> Vec<String> {
    let mut owed = vec![character.to_owned()];
    owed.extend(names.iter().map(|name| (*name).to_owned()));
    for name in names {
        owed.extend(goldens.iter().map(|direction| golden(name, direction)));
    }
    owed
}

/// What one golden names itself, which is also its file stem under
/// `art/goldens/<character>/`.
pub fn golden(clip: &str, direction: &str) -> String {
    format!("{clip}_{direction}")
}

/// The two directions a golden is taken in: the one facing the camera and the
/// one three quarters around, which is a quarter turn the other way.
///
/// Two, because a joint moved along the camera's own line of sight barely
/// moves on screen in that facing and moves fully in a facing 90 degrees off
/// it. `None` for a ring too short to have both.
pub fn golden_directions<'a>(directions: &[&'a str]) -> Option<[&'a str; 2]> {
    let count = directions.len();
    (count >= 4).then(|| [directions[0], directions[count * 3 / 4]])
}

fn forearm_roll_patch(character: &str, degrees: f64, profile: &Profile, attempt: u32) -> Finding {
    FOREARM_ROLL.measured(
        profile,
        character,
        degrees,
        attempt,
        format!(
            "spec.bake.forearm_roll asks for {degrees} degrees, and the transfer aims both rigs \
             at one table of world directions instead"
        ),
    )
}

/// One rendered frame, read down to what the rules need of it.
struct Frame {
    /// What share of the canvas is content, as a percent.
    coverage: f64,
    /// Where that content sits, or `None` for a frame with none at all.
    bounds: Option<Rect>,
    width: u32,
    height: u32,
}

impl Frame {
    fn read(path: &Path) -> Result<Self, String> {
        let image = image::open(path)
            .map_err(|error| format!("{} holds no readable image: {error}", path.display()))?
            .to_rgba8();
        let opaque = image.pixels().filter(|pixel| pixel.0[3] >= OPAQUE).count();
        let (width, height) = (image.width(), image.height());
        Ok(Self {
            coverage: opaque as f64 / f64::from(width * height) * 100.0,
            bounds: content_bounds(&image, OPAQUE),
            width,
            height,
        })
    }

    /// How far the content sits from the nearest canvas border.
    fn inset(&self) -> Option<u32> {
        let bounds = self.bounds?;
        Some(
            bounds
                .x
                .min(bounds.y)
                .min(self.width - (bounds.x + bounds.width))
                .min(self.height - (bounds.y + bounds.height)),
        )
    }

    /// The leftmost and rightmost column holding content.
    fn span(&self) -> Option<(u32, u32)> {
        let bounds = self.bounds?;
        Some((bounds.x, bounds.x + bounds.width - 1))
    }
}

/// Every frame of one clip, by direction and frame index.
struct Set {
    /// One row per direction, in the ring's own order. A slot is absent, or
    /// unreadable with the reason, or read.
    frames: Vec<Vec<Option<Result<Frame, String>>>>,
    /// How wide the rectangle is, which is the widest direction's own count.
    width: usize,
}

impl Set {
    fn read(clip: &Rendered<'_>) -> Self {
        let indexes: Vec<BTreeSet<u32>> = clip
            .directions
            .iter()
            .map(|direction| rendered_indexes(clip.dir, clip.name, direction))
            .collect();
        let width = indexes
            .iter()
            .filter_map(|found| found.last().copied())
            .max()
            .map_or(0, |highest| highest as usize + 1);
        let frames = indexes
            .iter()
            .zip(clip.directions)
            .map(|(found, direction)| {
                (0..width)
                    .map(|index| {
                        found.contains(&(index as u32)).then(|| {
                            Frame::read(&frame_path(clip.dir, clip.name, direction, index as u32))
                        })
                    })
                    .collect()
            })
            .collect();
        Self { frames, width }
    }

    /// Every frame that was read, with the direction and index it came from.
    fn read_frames(&self) -> impl Iterator<Item = (usize, usize, &Result<Frame, String>)> {
        self.frames.iter().enumerate().flat_map(|(row, frames)| {
            frames
                .iter()
                .enumerate()
                .filter_map(move |(index, frame)| Some((row, index, frame.as_ref()?)))
        })
    }

    /// The first reason a frame could not be read, if any could not.
    fn unreadable(&self) -> Option<&String> {
        self.read_frames()
            .find_map(|(_, _, frame)| frame.as_ref().err())
    }

    fn frame_count(&self, clip: &Rendered<'_>, profile: &Profile, attempt: u32) -> Finding {
        if self.width == 0 {
            return FRAME_COUNT.undefined(
                clip.name,
                attempt,
                format!(
                    "{} left no frame at all in {}, so there is no rendered set to count",
                    clip.name,
                    clip.dir.display()
                ),
            );
        }
        let missing: Vec<String> = self
            .frames
            .iter()
            .zip(clip.directions)
            .flat_map(|(frames, direction)| {
                frames
                    .iter()
                    .enumerate()
                    .filter(|(_, frame)| frame.is_none())
                    .map(move |(index, _)| format!("{direction} {index:02}"))
            })
            .collect();
        FRAME_COUNT.measured(
            profile,
            clip.name,
            missing.len() as f64,
            attempt,
            format!(
                "{} rendered {} of {} frames, {} directions x {} frames{}",
                clip.name,
                self.frames.len() * self.width - missing.len(),
                self.frames.len() * self.width,
                self.frames.len(),
                self.width,
                if missing.is_empty() {
                    String::new()
                } else {
                    format!(", missing {}", missing.join(", "))
                }
            ),
        )
    }

    fn non_empty(&self, clip: &Rendered<'_>, profile: &Profile, attempt: u32) -> Finding {
        if let Some(reason) = self.unreadable() {
            return NON_EMPTY.undefined(clip.name, attempt, reason.clone());
        }
        let Some((row, index, coverage)) = self
            .read_frames()
            .filter_map(|(row, index, frame)| Some((row, index, frame.as_ref().ok()?.coverage)))
            .min_by(|a, b| a.2.total_cmp(&b.2))
        else {
            return NON_EMPTY.undefined(
                clip.name,
                attempt,
                format!(
                    "{} left no frame at all, so none has any coverage",
                    clip.name
                ),
            );
        };
        NON_EMPTY.measured(
            profile,
            clip.name,
            coverage,
            attempt,
            format!(
                "the emptiest frame of {} is {} {index:02}, at {coverage:.4} percent of its canvas",
                clip.name, clip.directions[row]
            ),
        )
    }

    fn in_frame(&self, clip: &Rendered<'_>, profile: &Profile, attempt: u32) -> Finding {
        if let Some(reason) = self.unreadable() {
            return IN_FRAME.undefined(clip.name, attempt, reason.clone());
        }
        let Some((row, index, inset)) = self
            .read_frames()
            .filter_map(|(row, index, frame)| Some((row, index, frame.as_ref().ok()?.inset()?)))
            .min_by_key(|(_, _, inset)| *inset)
        else {
            return IN_FRAME.undefined(
                clip.name,
                attempt,
                format!(
                    "no frame of {} holds any content, so none has an inset to read",
                    clip.name
                ),
            );
        };
        IN_FRAME.measured(
            profile,
            clip.name,
            f64::from(inset),
            attempt,
            format!(
                "the tightest frame of {} is {} {index:02}, {inset} px clear of the nearest border",
                clip.name, clip.directions[row]
            ),
        )
    }

    fn pivot(&self, clip: &Rendered<'_>, profile: &Profile, attempt: u32) -> Finding {
        if let Some(reason) = self.unreadable() {
            return PIVOT.undefined(clip.name, attempt, reason.clone());
        }
        let count = self.frames.len();
        if count < 2 || !count.is_multiple_of(2) {
            return PIVOT.undefined(
                clip.name,
                attempt,
                format!(
                    "a ring of {count} direction(s) has no opposite facing to reflect {} about",
                    clip.name
                ),
            );
        }
        let mut worst: Option<(usize, usize, u64)> = None;
        let mut pairs = 0;
        for half in 0..count / 2 {
            for index in 0..self.width {
                let Some((near, far)) = self
                    .frame(half, index)
                    .zip(self.frame(half + count / 2, index))
                else {
                    continue;
                };
                let Some(reading) = reflected(near, far) else {
                    continue;
                };
                pairs += 1;
                if worst.is_none_or(|(_, _, seen)| reading > seen) {
                    worst = Some((half, index, reading));
                }
            }
        }
        let Some((half, index, reading)) = worst else {
            return PIVOT.undefined(
                clip.name,
                attempt,
                format!(
                    "no pair of opposite frames of {} both hold content, so nothing reflects",
                    clip.name
                ),
            );
        };
        PIVOT.measured(
            profile,
            clip.name,
            reading as f64,
            attempt,
            format!(
                "the worst of {pairs} opposite pair(s) of {} is {} against {} at frame {index:02}, \
                 {reading} px off its own reflection",
                clip.name,
                clip.directions[half],
                clip.directions[half + count / 2],
            ),
        )
    }

    /// One frame that was read and decoded, or `None`.
    fn frame(&self, row: usize, index: usize) -> Option<&Frame> {
        self.frames.get(row)?.get(index)?.as_ref()?.as_ref().ok()
    }
}

/// How far two opposite frames sit from being each other's reflection, in
/// pixels.
///
/// Both sides of the silhouette are read, because a pivot that drifted moves
/// one edge in and the other out by the same amount.
fn reflected(near: &Frame, far: &Frame) -> Option<u64> {
    let ((near_left, near_right), (far_left, far_right)) = (near.span()?, far.span()?);
    let last = i64::from(near.width.min(far.width)) - 1;
    let apart = |one: u32, other: u32| (i64::from(one) + i64::from(other) - last).unsigned_abs();
    Some(apart(near_right, far_left).max(apart(near_left, far_right)))
}

/// Every frame index one direction of one clip left on disk.
fn rendered_indexes(dir: &Path, name: &str, direction: &str) -> BTreeSet<u32> {
    let prefix = format!("{name}_{direction}_");
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter_map(|file| {
            file.strip_suffix(".png")?
                .strip_prefix(&prefix)?
                .parse()
                .ok()
        })
        .collect()
}

/// Where the bake writes one frame. `framing.frame_filename` spells the same
/// name, and `pack::load_animation_frames` reads it back.
fn frame_path(dir: &Path, name: &str, direction: &str, index: u32) -> PathBuf {
    dir.join(format!("{name}_{direction}_{index:02}.png"))
}
