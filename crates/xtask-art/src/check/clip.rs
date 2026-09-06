//! The clip gates: what a fitted clip must be, measured against the file the
//! motion was bought in.
//!
//! Five rules, from two different measurements.
//!
//! - [`INTERPOLATION`] and [`REFERENCE_POSE_KEY`] are counted inside Blender,
//!   because the glTF exporter resamples every channel on the way out and
//!   writes its own interpolation, so a Bezier action exports as `LINEAR` and
//!   the defect is invisible in the file. `clip.py` does the counting with no
//!   `bpy`, so both have a negative control that runs in CI.
//! - [`SWING`], [`TWIST`] and [`OBJECT_TRANSFORM`] are measured here, in Rust,
//!   on the delivered GLB. Between `transfer.py` and that file sit
//!   `write_keys`, the travel scale, the interpolation pass and the exporter,
//!   and nothing else measures any of them.
//!
//! Rust owns every rule either way. The ids, units, limits and spaces live
//! here so `cargo art check --list-rules` prints them, and
//! [`Report::off_registry`](super::Report::off_registry) refuses a report that
//! disagrees with what that list says.
//!
//! # What `clip.swing` and `clip.twist` can and cannot catch
//!
//! **`clip.swing` can catch** a Blender shell that wrote something other than
//! what `transfer.py` computed, a role driving the wrong bone, a dropped or
//! duplicated frame, a key written at the wrong time, and an export that lost
//! or resampled the motion. On the shipped pre-rename output it reads 97.797
//! degrees on the hips and 76.154 and 78.216 on the wrists.
//!
//! **Neither rule can catch** a wrong aim table, and `clip.swing` cannot be
//! evidence that a clip is right. Aiming both rigs at one table makes every
//! offset a pure twist about the bone's own axis, which cannot move where the
//! bone points, so the transfer drives `clip.swing` to zero **by
//! construction**: measured, a table fed where a reference pose belongs
//! leaves the worst swing at 1.2e-14 degrees while moving the offsets by up to
//! 160.970 degrees. Neither rule sees a **joint** chain either, only each
//! bone's own frame, which is the blind spot the design names and `T15` plus
//! `source.child_axis` answer. A reading of 0.000 here means the file carries
//! the motion the transfer computed. It does not mean the clip looks right.
//!
//! # How the source reaches this module
//!
//! The vendor file is an FBX. The Rust `gltf` reader cannot open one and CI
//! has no Blender, so `retarget_animation.py` writes the source's own world
//! orientations into a sidecar beside its report and these rules read the GLB
//! and the sidecar together. Both are [`Motion`], both in Blender Z-up world
//! space, and both aligned by seconds from the clip's own start. **One space
//! is not decoration**: a twist about a bone's own axis is not invariant under
//! a change of world frame, so reading one side in glTF Y-up and the other in
//! Blender would give precise, wrong numbers.
//!
//! [`TWIST`]'s rest term comes off the delivered GLB's own joints, so the
//! export has to write the armature at its rest position. Both export scripts
//! say `export_rest_position_armature=True` rather than leaning on the
//! default, and a unit test in `test_blender.rs` fails on one that does not.
//!
//! In CI the same rules run on the synthetic cross-rig pair
//! `crates/xtask-art/tests/unit/clips.rs` builds, whose two expected values
//! are hand-computed rather than measured.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Result;
use glam::{DMat4, DQuat};

use super::aim::AimTable;
use super::gltf_world::Skeleton;
use super::motion::Motion;
use super::profile::Profile;
use super::{Comparison, Finding, Rule, gltf_clip, relative_to};

/// The stage these findings belong to, which names their report file.
pub const STAGE: &str = "retarget";

const CHANNELS: &str = "the F-curves of the output action, before export";

const RELATIVE: &str = "Blender Z-up world space, the output bone against the source bone, \
                        aligned by seconds from clip start, worst frame of the clip";

const NODES: &str = "every node of the clip that is not a joint, the armature and the skin \
                     carrier alike: its own transform against the committed rig, and whether \
                     any channel drives it";

/// How far two frames may sit apart and still be the same instant, in
/// seconds. A tenth of a millisecond, which is 0.3 percent of a frame at 30
/// fps, so a key one frame out cannot pass as aligned.
const ALIGNED_SECONDS: f64 = 1e-4;

/// Where a swing leaves no twist to read, in degrees.
///
/// At exactly half a turn about an axis square to +Y the twist does not
/// exist: every twist gives the same swing. The band is a real one rather
/// than a point because the file stores `f32`: within a tenth of a degree of
/// half a turn the twist has fewer digits left than the tightest limit any
/// gate publishes. A swing that far out is already an error of its own.
const SINGULAR_SWING_DEGREES: f64 = 179.9;

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

/// Where the bone points, absolutely, against the source.
///
/// ```text
/// relative(b,t) = world(src,b,t)^-1 @ world(out,b,t)   # the source's frame
/// swing, twist  = split(relative(b,t), +Y)             # one split, fact 19
/// clip.swing(b) = max over t of angle(swing)
/// ```
///
/// The swing of that rotation is the angle between the output bone's own +Y
/// in world space and the source bone's. One combined orientation angle
/// cannot serve: our legs sit 171 to 175 degrees of pure roll from Mixamo's at
/// rest and a correct fit preserves that, so a combined metric would have to
/// reject every correct clip.
pub const SWING: Rule = Rule {
    id: "clip.swing",
    comparison: Comparison::Le,
    unit: "degrees",
    space: RELATIVE,
    limit: |profile| profile.clip.swing_degrees,
};

/// How far the bone is rolled about its own length, against its own bind pose.
///
/// ```text
/// clip.twist(b) = max over t of |wrap( angle(twist at t)
///                                    - angle(twist at rest) )|
/// ```
///
/// The same split as [`SWING`], on the same rotation, about the same axis:
/// the source bone's own +Y. Its twist is the roll the clip carries, and the
/// rest term is the roll the two **bind poses** carry, so the 174 degrees of
/// convention difference cancels and a re-rolled thigh does not.
///
/// **The rest term is a composition, not a difference of two angles.**
/// Subtracting `twist(out, rest) - twist(src, rest)` is only valid when the
/// two rotations share their swing, which `out` and `src` do at every frame,
/// because the offset is a pure twist, and which the two bind poses do not.
/// Measured, that form reads 108.805 degrees on a correct Mixamo fit while
/// this one reads 11.411, and a limit above the first would let the 90 degree
/// injected twist this rule exists to catch pass at 95.5.
pub const TWIST: Rule = Rule {
    id: "clip.twist",
    comparison: Comparison::Le,
    unit: "degrees",
    space: RELATIVE,
    limit: |profile| profile.clip.twist_degrees,
};

/// Every node beside the joints carries the transform the committed rig
/// gives it, and nothing animates one.
///
/// `transform_apply(scale=True)` on a rig that owns an action moves the
/// armature's 0.01 scale out of the object and into the rest geometry, and
/// leaves every location key byte identical, so 2.316 m of travel silently
/// becomes 231.599 m. That move is invisible in world space and plain here.
///
/// The channel half is what the static reading cannot see: an action on the
/// armature object moves the whole clip while every node's own transform
/// still matches the rig's.
///
/// `strip_animation.py` weights a tiny triangle to the root bone so the
/// armature exports at all, so `skin_carrier` is a subject too. It is a node
/// the file really carries, and applying a transform to it is the same defect
/// on a different object.
pub const OBJECT_TRANSFORM: Rule = Rule {
    id: "clip.object_transform",
    comparison: Comparison::Eq,
    unit: "nodes",
    space: NODES,
    limit: |_| 0.0,
};

/// Every rule this module owns, in the order `--list-rules` prints them.
pub const RULES: [&Rule; 5] = [
    &INTERPOLATION,
    &REFERENCE_POSE_KEY,
    &SWING,
    &TWIST,
    &OBJECT_TRANSFORM,
];

/// Runs the three file-side clip rules on one fitted clip.
///
/// A missing or unreadable input is one error finding per rule, never a skip:
/// a gate that goes quiet on absent input proves nothing.
pub fn check_files(
    output: &Path,
    source_motion: &Path,
    rig: &Path,
    repo_root: &Path,
    profile: &Profile,
    table: &AimTable,
    attempt: u32,
) -> Result<Vec<Finding>> {
    let bones = table.bones(table.canonical())?.clone();
    let clip = relative_to(output, repo_root);
    let bytes = match std::fs::read(output) {
        Ok(bytes) => bytes,
        Err(error) => {
            return Ok([&SWING, &TWIST, &OBJECT_TRANSFORM]
                .map(|rule| {
                    rule.undefined(&clip, attempt, format!("{clip} cannot be read: {error}"))
                })
                .to_vec());
        }
    };
    let compared = match (gltf_clip::read(&bytes, &bones), Motion::read(source_motion)) {
        (Ok(output), Ok(source)) => compare(&output, &source, &bones, profile, attempt),
        (output, source) => {
            let error = output
                .err()
                .or(source.err())
                .expect("one of the two failed");
            [&SWING, &TWIST]
                .map(|rule| {
                    rule.undefined(
                        &clip,
                        attempt,
                        format!(
                            "{clip} and its source motion cannot be measured against \
                             each other: {error:#}"
                        ),
                    )
                })
                .to_vec()
        }
    };
    let objects = match (Skeleton::from_slice(&bytes), Skeleton::read(rig)) {
        (Ok(fitted), Ok(rig)) => object_transform(&fitted, &rig, profile, attempt),
        (fitted, rig) => {
            let error = fitted.err().or(rig.err()).expect("one of the two failed");
            vec![OBJECT_TRANSFORM.undefined(
                &clip,
                attempt,
                format!("{clip} cannot be read against the committed rig: {error:#}"),
            )]
        }
    };
    Ok([compared, objects].concat())
}

/// `clip.swing` and `clip.twist`, one finding each per role. Pure: both
/// motions are already read.
///
/// The subjects are every role the canonical convention maps together with
/// every role the source drove, so a role one side is missing is reported
/// rather than dropped. Nothing here is skipped for being absent.
pub fn compare(
    output: &Motion,
    source: &Motion,
    bones: &BTreeMap<String, String>,
    profile: &Profile,
    attempt: u32,
) -> Vec<Finding> {
    let roles: BTreeSet<&str> = bones
        .keys()
        .map(String::as_str)
        .chain(source.roles())
        .collect();
    if output.frames().len() != source.frames().len() {
        return misaligned(
            &roles,
            attempt,
            &format!(
                "the clip carries {} frame(s) and the source has {}",
                output.frames().len(),
                source.frames().len()
            ),
        );
    }
    if let Some(apart) = worst_time_gap(output, source) {
        return misaligned(
            &roles,
            attempt,
            &format!(
                "a frame of the clip sits {apart} s from the source frame it \
                 should share, against a tolerance of {ALIGNED_SECONDS} s"
            ),
        );
    }
    roles
        .into_iter()
        .flat_map(|role| measure(role, output, source, bones, profile, attempt))
        .collect()
}

/// The worst reading of one rule over one clip, and when it happened.
#[derive(Default)]
struct Worst {
    degrees: f64,
    seconds: f64,
}

impl Worst {
    fn keep(&mut self, degrees: f64, seconds: f64) {
        if degrees > self.degrees {
            *self = Self { degrees, seconds };
        }
    }
}

/// Both rules on one role, or the reason neither can be measured.
fn measure(
    role: &str,
    output: &Motion,
    source: &Motion,
    bones: &BTreeMap<String, String>,
    profile: &Profile,
    attempt: u32,
) -> Vec<Finding> {
    let bone = bones.get(role).map_or(role, String::as_str);
    let (Some(out_rest), Some(src_rest)) = (output.rest(role), source.rest(role)) else {
        let missing = if output.rest(role).is_none() {
            format!("the clip has no bone for the {role} role, so nothing carries its motion")
        } else {
            format!("the source clip drives no {role}, so there is nothing to measure against")
        };
        return [&SWING, &TWIST]
            .map(|rule| rule.undefined(role, attempt, missing.clone()))
            .to_vec();
    };

    let at_rest = Split::of(src_rest, out_rest);
    let mut swing = Worst::default();
    let mut twist = Worst::default();
    // Where the twist stops existing, if it does: the first frame at half a
    // turn of swing, and the swing it read there.
    let mut singular = at_rest
        .twist
        .is_none()
        .then_some(("at rest".to_owned(), at_rest.swing));
    for (out, src) in output.frames().iter().zip(source.frames()) {
        let (Some(out_rotation), Some(src_rotation)) =
            (out.rotations.get(role), src.rotations.get(role))
        else {
            // `Motion` refuses a frame whose roles differ from its rest pose,
            // and both rest poses answered above, so this cannot arise.
            continue;
        };
        let split = Split::of(*src_rotation, *out_rotation);
        swing.keep(split.swing, out.seconds);
        match (split.twist, at_rest.twist) {
            (Some(rolled), Some(rest)) => {
                twist.keep(wrapped(rolled - rest).abs(), out.seconds);
            }
            _ => {
                singular.get_or_insert_with(|| (format!("at {} s", out.seconds), split.swing));
            }
        }
    }
    vec![
        SWING.measured(
            profile,
            role,
            swing.degrees,
            attempt,
            format!(
                "{bone} points {:.6} degrees from the source's {role} at {} s",
                swing.degrees, swing.seconds
            ),
        ),
        match singular {
            Some((when, apart)) => TWIST.undefined(
                role,
                attempt,
                format!("{when} {bone} swing is {apart:.3} degrees, twist undefined"),
            ),
            None => TWIST.measured(
                profile,
                role,
                twist.degrees,
                attempt,
                format!(
                    "{bone} is rolled {:.3} degrees from the {:.3} degrees the two bind \
                     poses call for, at {} s",
                    twist.degrees,
                    at_rest
                        .twist
                        .expect("a rest roll, or `singular` would carry the reason"),
                    twist.seconds
                ),
            ),
        },
    ]
}

/// One undefined finding per role per rule, for a fault that makes the whole
/// pair unmeasurable. Every subject is still reported, because a rule that
/// goes quiet cannot be told from one that never ran.
fn misaligned(roles: &BTreeSet<&str>, attempt: u32, why: &str) -> Vec<Finding> {
    roles
        .iter()
        .flat_map(|role| [&SWING, &TWIST].map(|rule| rule.undefined(role, attempt, why.to_owned())))
        .collect()
}

/// The worst distance between two clips' frames, or `None` when every pair is
/// inside [`ALIGNED_SECONDS`]. Both sides are already counted at the clip's
/// own start, so this compares elapsed time and never a frame number.
fn worst_time_gap(output: &Motion, source: &Motion) -> Option<f64> {
    output
        .frames()
        .iter()
        .zip(source.frames())
        .map(|(out, src)| (out.seconds - src.seconds).abs())
        .max_by(f64::total_cmp)
        .filter(|apart| *apart > ALIGNED_SECONDS)
}

/// One bone's output against the source's, split about the bone's own +Y.
///
/// Both rules read this one rotation, `source^-1 @ output`, in the source
/// bone's own frame. Its swing is how far the two bones point apart, which is
/// `clip.swing`, and its twist is how far the output is rolled from the
/// source, which `clip.twist` reads against the same roll at rest.
struct Split {
    /// Degrees, always in `0..=180`.
    swing: f64,
    /// Degrees, in `(-180, 180]`, or `None` at half a turn of swing.
    twist: Option<f64>,
}

impl Split {
    /// The vector part is projected, never a normalized axis: `Quaternion.axis`
    /// drops the `sin(angle / 2)` factor and is worst near identity, which is
    /// exactly where a correct clip sits
    /// (`docs/research/agent_reports/proof_swing_twist_vector_part.md`).
    ///
    /// The twist is `normalize(w, project(v, +Y))`, so `hypot(w, y)` is the
    /// cosine of half the swing and `hypot(x, z)` is its sine. `atan2` on
    /// that pair rather than `acos` on a dot product: near zero, which is
    /// where a correct clip sits, `acos` keeps half the digits of its
    /// argument and turns 5e-5 degrees of real error into a reading of 5e-2.
    fn of(source: DQuat, output: DQuat) -> Self {
        let relative = source.inverse() * output;
        let along = relative.w.hypot(relative.y);
        let across = relative.x.hypot(relative.z);
        Self {
            swing: 2.0 * across.atan2(along).to_degrees(),
            twist: (along > (SINGULAR_SWING_DEGREES.to_radians() / 2.0).cos())
                .then(|| wrapped(2.0 * relative.y.atan2(relative.w).to_degrees())),
        }
    }
}

/// An angle brought into (-180, 180]. A rotation and its negation are the
/// same rotation, so without this half of these read a full turn out.
fn wrapped(degrees: f64) -> f64 {
    -(-degrees + 180.0).rem_euclid(360.0) + 180.0
}

/// `clip.object_transform`, one finding per node that is not a joint.
///
/// The committed rig is the answer key, not the identity matrix: both files
/// carry a 0.01 scale on their armature node, and it is `transform_apply`
/// removing that scale which changes what every location key means.
pub fn object_transform(
    clip: &Skeleton,
    rig: &Skeleton,
    profile: &Profile,
    attempt: u32,
) -> Vec<Finding> {
    let names: BTreeSet<&String> = clip
        .object_nodes()
        .keys()
        .chain(rig.object_nodes().keys())
        .collect();
    names
        .into_iter()
        .map(|name| {
            let (same, message) = verdict(clip, rig, name);
            OBJECT_TRANSFORM.measured(profile, name, f64::from(!same), attempt, message)
        })
        .collect()
}

/// One node's verdict: whether it is as the rig has it, and why.
fn verdict(clip: &Skeleton, rig: &Skeleton, name: &str) -> (bool, String) {
    let driven = clip.object_channels().get(name).copied().unwrap_or(0);
    if driven > 0 {
        return (
            false,
            format!(
                "{driven} channel(s) drive {name}, which moves the whole clip while \
                 its own transform still reads like the rig's"
            ),
        );
    }
    match (clip.object_nodes().get(name), rig.object_nodes().get(name)) {
        // Byte equality, because both files write this value from the same
        // stored object transform: six exported GLBs carry the armature scale
        // as the identical `0.009999999776482582`.
        (Some(theirs), Some(ours)) if theirs == ours => (
            true,
            format!("{name} carries the transform the rig gives it"),
        ),
        (Some(theirs), Some(ours)) => (
            false,
            format!(
                "{name} carries {} where the rig has {}",
                scale_and_offset(*theirs),
                scale_and_offset(*ours)
            ),
        ),
        (Some(_), None) => (
            false,
            format!("{name} sits beside the clip's joints and the rig has no such node"),
        ),
        _ => (
            false,
            format!("the clip has no {name}, which the rig carries beside its joints"),
        ),
    }
}

/// A transform as a message names it: what it scales by and where it sits.
fn scale_and_offset(matrix: DMat4) -> String {
    let (scale, _, translation) = matrix.to_scale_rotation_translation();
    format!(
        "scale [{:.9}, {:.9}, {:.9}] at [{:.4}, {:.4}, {:.4}]",
        scale.x, scale.y, scale.z, translation.x, translation.y, translation.z
    )
}
