//! The source gates: what the vendor file must be, before a single credit or
//! minute is spent fitting it.
//!
//! Six rules, all of them measured in `tools/blender/src/check_source.py`
//! and all of them owned here. The vendor file is an FBX: no Rust reader
//! opens one, so rule four's "CI-side measurement is Rust" cannot apply and
//! the measuring is Blender's. What stays here are the ids, the limits, the
//! units and the spaces, so `cargo art check --list-rules` prints them and
//! [`Report::off_registry`](super::Report::off_registry) refuses a report
//! that disagrees with what that list says.
//!
//! Three of the six gate and three record.
//!
//! - [`FPS_DECLARED`], [`TRAVELING`] and [`IN_PLACE`] gate. A clip whose own
//!   rate is not the one the library declares plays at the wrong speed, and a
//!   clip whose root motion is not what `travels` says is either a walk that
//!   never leaves the origin or an in-place cycle sliding out of frame.
//! - [`WANDER`], [`CHILD_AXIS`] and [`POSTURE`] record, with ceilings set so
//!   far out that every reading is information. That is deliberate. A vendor
//!   skeleton is not ours to regenerate, so a limit that could fail would
//!   fail forever: `T5` measured Mixamo's `Neck` 16.933 degrees off the
//!   direction to its own `Head` by hand, and after `T15` regenerates our rig
//!   that difference is still there, in every clip, and nothing else in the
//!   pipeline can see it. And how far the hips wander on the way is 0.0276 m
//!   for a run cycle against 2.3117 for a strafe, so no one threshold reads
//!   both.
//!
//! # Why travel is two rules and not one
//!
//! `travels` in the library declares which side of the threshold the vendor
//! file must sit, and a mistyped flag has to fail either way round. One id
//! carrying two comparisons cannot be held to the registry, which is the one
//! thing standing between a hand-written Python finding and a limit nobody
//! published. So the declaration picks the rule: a clip that travels is read
//! by [`TRAVELING`] while [`IN_PLACE`] reports `skipped`, and a clip that does
//! not is read the other way round. Both are printed, both carry the same
//! threshold, and every clip reports on both.

use std::collections::BTreeMap;

use super::profile::Profile;
use super::{Comparison, Rule};

/// The stage these findings belong to, which names their report file.
pub const STAGE: &str = "fetch";

const RATE: &str = "the vendor file's own key spacing, against the rate the library declares";

const HIPS: &str = "the vendor file's hips, first frame to last, in Blender Z-up world space";

const REST: &str = "the vendor rig's own bone axes at rest, against the direction to each \
                    mapped child, in Blender Z-up world space";

const POSE: &str = "the vendor rig's joints over the clip, in Blender Z-up world space";

const PATH: &str = "the vendor file's hips at every frame, against the first, horizontally, \
                    in Blender Z-up world space";

/// The largest angle two directions can be apart.
///
/// A recording rule still needs a limit, because every finding is read
/// against one. This is the ceiling that cannot be crossed, so the severity
/// is always `info` and the number is always on record.
const HALF_A_TURN: f64 = 180.0;

/// Further than any clip in any library moves its hips.
///
/// [`WANDER`]'s ceiling, for the same reason: the reading is the point, and
/// the largest one measured is `strafe_left.fbx` at 2.3117 m.
const A_KILOMETER: f64 = 1000.0;

/// The clip runs at the rate the library says it does.
///
/// Measured as the gap between the two rates rather than as the rate itself:
/// a rule's limit is published in `[profile]` and read by everyone, and the
/// declared rate is per clip, so the number that can be compared against a
/// published zero is the disagreement.
pub const FPS_DECLARED: Rule = Rule {
    id: "source.fps_declared",
    comparison: Comparison::Eq,
    unit: "frames per second",
    space: RATE,
    limit: |_| 0.0,
};

/// A clip the library declares traveling carries its root motion.
///
/// Decision 12 fetches every source traveling, so the femur ratio can size
/// the step to our own body. An in-place export declared this way would be a
/// character walking on the spot with nothing to scale.
pub const TRAVELING: Rule = Rule {
    id: "source.traveling",
    comparison: Comparison::Ge,
    unit: "meters",
    space: HIPS,
    limit: |profile| profile.source.travel_meters,
};

/// And a clip the library declares in place carries none.
///
/// The same threshold read the other way, so a `travels` flag typed wrong
/// fails whichever way it was typed. Without this half, `travels: false` on a
/// traveling clip would simply switch the gate off.
pub const IN_PLACE: Rule = Rule {
    id: "source.in_place",
    comparison: Comparison::Le,
    unit: "meters",
    space: HIPS,
    limit: |profile| profile.source.travel_meters,
};

/// How far the hips get from where they started on the way, horizontally.
///
/// Records, never gates: an in-place cycle wanders 0.0276 m and a strafe
/// 2.3117, so no one threshold reads both. This is the reading [`TRAVELING`]
/// cancels, and the only boundary that can take it: the bake pins the
/// horizontal axes onto the first frame before
/// [`super::clip::ROOT_TRAVEL`] sees them.
pub const WANDER: Rule = Rule {
    id: "source.wander",
    comparison: Comparison::Le,
    unit: "meters",
    space: PATH,
    limit: |_| A_KILOMETER,
};

/// How far each vendor bone's own axis sits from the direction to its mapped
/// child.
///
/// Records, never gates. No other published rule sees a joint chain, so with
/// the vendor's `Neck` axis 16.933 degrees off the direction to its own
/// `Head`, a correct transfer still leans our head about 17 degrees
/// differently on every Mixamo clip. `T15` regenerates our half of that; this
/// is the other half, on record rather than silent.
pub const CHILD_AXIS: Rule = Rule {
    id: "source.child_axis",
    comparison: Comparison::Le,
    unit: "degrees",
    space: REST,
    limit: |_| HALF_A_TURN,
};

/// What the clip's own posture is: head pitch, spine lean, arm swing.
///
/// Records, never gates, for the same reason: the hunch is in the source.
/// `strafe_left` is authored with the head 34 to 37 degrees forward and our
/// output copies it faithfully, so the number belongs beside the purchase
/// rather than in a gate that would reject every Mixamo clip.
pub const POSTURE: Rule = Rule {
    id: "source.posture",
    comparison: Comparison::Le,
    unit: "degrees",
    space: POSE,
    limit: |_| HALF_A_TURN,
};

/// Every rule this module owns, in the order `--list-rules` prints them.
pub const RULES: [&Rule; 6] = [
    &FPS_DECLARED,
    &TRAVELING,
    &IN_PLACE,
    &WANDER,
    &CHILD_AXIS,
    &POSTURE,
];

/// Which role each role's own axis must point at, for [`CHILD_AXIS`].
///
/// `[profile.tails]` chooses the child, because `hips` has three and only one
/// of them continues the body, and it names bones of our own rig. `bones` is
/// the canonical convention, so this reads the pairs back out as roles, which
/// is the only key a vendor rig also carries.
pub fn mapped_children(
    profile: &Profile,
    bones: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let roles: BTreeMap<&str, &str> = bones
        .iter()
        .map(|(role, bone)| (bone.as_str(), role.as_str()))
        .collect();
    profile
        .tails
        .iter()
        .filter_map(|(bone, tail)| {
            Some((
                (*roles.get(bone.as_str())?).to_owned(),
                (*roles.get(tail.as_str())?).to_owned(),
            ))
        })
        .collect()
}
