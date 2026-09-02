//! The clip gates. Two of them today, both owned by the retarget.
//!
//! These two are the only rules in the pipeline that a Blender script
//! measures rather than Rust. Everything else moved to Rust so its negative
//! control runs on the arm64 CI runner with no Blender, and these cannot: the
//! glTF exporter resamples every channel on the way out and writes its own
//! interpolation, so a Bezier action exports as LINEAR and the defect is
//! invisible in the file. The honest place to read them is the action itself,
//! inside Blender, which is what `retarget_animation.py` does.
//!
//! Rust still owns the rules. The ids, units, limits and spaces live here so
//! `cargo art check --list-rules` prints them, and
//! [`Report::off_registry`](super::Report::off_registry) refuses a report
//! that disagrees with what that list says.

use super::{Comparison, Rule};

const CHANNELS: &str = "the F-curves of the output action, before export";

/// Straight lines between the frames the transfer wrote, and a held pose
/// outside them. Bezier handles round off every joint's path between two
/// sampled frames, and `keyframe_insert` writes Bezier by default.
pub const INTERPOLATION: Rule = Rule {
    id: "clip.interpolation",
    comparison: Comparison::Eq,
    unit: "channels",
    space: CHANNELS,
    limit: |_| 0.0,
};

/// No key at a frame the source does not have.
///
/// The reference pose is computed algebraically and never posed at a scene
/// frame, so it cannot be keyed by accident. This rule is what says so:
/// retarget_bvh leaves its own reference pose keyed at frame 0, which is one
/// frame outside a Mixamo clip's own 1 to 21.
pub const REFERENCE_POSE_KEY: Rule = Rule {
    id: "clip.reference_pose_key",
    comparison: Comparison::Eq,
    unit: "keys",
    space: CHANNELS,
    limit: |_| 0.0,
};

/// Every rule this module owns, in the order `--list-rules` prints them.
pub const RULES: [&Rule; 2] = [&INTERPOLATION, &REFERENCE_POSE_KEY];
