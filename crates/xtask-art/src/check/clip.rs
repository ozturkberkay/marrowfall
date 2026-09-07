//! The clip gates: what a fitted clip must be, measured against the file the
//! motion was bought in.
//!
//! Thirteen rules, measured at three boundaries, plus the three
//! [`foot`] owns beside them.
//!
//! - [`INTERPOLATION`] and [`REFERENCE_POSE_KEY`] are counted inside Blender
//!   at the retarget, because neither survives the export: the glTF exporter
//!   resamples every channel and writes its own interpolation. [`FPS_GRID`],
//!   [`FPS_GRID_RANGE`], [`FLOOR_SNAP`], [`STRIDE`] and [`STRIDE_RATIO`] are
//!   reported there too, with `clip.foot_contact.plants`, `.skate` and
//!   `.penetration`. `clip.py` and `plant.py` do the arithmetic with no
//!   `bpy`, so all ten have a negative control in CI.
//! - [`SWING`], [`TWIST`], [`OBJECT_TRANSFORM`], [`FPS_GRID`], [`LOOP`],
//!   [`FLOOR_SNAP`], [`STRIDE`] and [`STRIDE_RATIO`] are measured here, in
//!   Rust, on the delivered GLB. Between `transfer.py` and that file sit
//!   `write_keys`, the femur scale, the floor snap, the interpolation pass
//!   and the exporter, and nothing else measures any of them.
//! - [`ROOT_TRAVEL`] and [`ROOT_BOB`] are measured at the bake, in
//!   `bake_sprites.py`, because they read the copy `strip_root_motion` has
//!   just pinned and that copy is never written to disk.
//!
//! Four rules are in two of those lists on purpose. The retarget reads what
//! only the action and the pose it evaluated carry, and this module reads the
//! file that was written from them. [`FPS_GRID_RANGE`] stays Blender-only:
//! a render range is a scene property no file carries.
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

use anyhow::{Context as _, Result};
use glam::{DMat4, DQuat, DVec3};

use super::aim::AimTable;
use super::gltf_world::Skeleton;
use super::motion::{Motion, SourceLengths};
use super::profile::Profile;
use super::{Comparison, Finding, Rule, foot, gltf_clip, relative_to};

/// The stage these findings belong to, which names their report file.
pub const STAGE: &str = "retarget";

const CHANNELS: &str = "the F-curves of the output action, before export";

const RELATIVE: &str = "Blender Z-up world space, the output bone against the source bone, \
                        aligned by seconds from clip start, worst frame of the clip";

const NODES: &str = "every node of the clip that is not a joint, the armature and the skin \
                     carrier alike: its own transform against the committed rig, and whether \
                     any channel drives it";

const GROUND: &str = "every ground joint at every frame of the clip, in Blender Z-up world \
                      space, after the floor snap";

const SIZED: &str = "the root joint's horizontal travel, first frame to last, against the \
                     source's own travel sized by the femur ratio, in Blender Z-up world space";

const SEGMENT: &str = "the two stride_segment joints of each rig at rest, in Blender Z-up \
                       world space";

const BODY: &str = "every joint of the clip against the source's, each against its own root \
                    joint and the source's sized by the femur ratio, in Blender Z-up world \
                    space, worst frame of the clip";

/// Further apart than two rigs of one skeleton can be.
///
/// [`STRIDE_RATIO`] records rather than gates, and every finding is still
/// read against a limit, so this is the ceiling that cannot be crossed. The
/// readings are in the Test Plan row.
const A_HUNDREDFOLD: f64 = 100.0;

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
/// `armature.py` weights a tiny triangle to the root bone so the
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

/// Every key of the clip lands on a whole frame of its own rate.
///
/// glTF stores key times in seconds, so the scene's rate decides which frames
/// they land on: a 30 fps clip read in a 24 fps scene spans 0.8 to 16.8, and
/// rounding that to 1 to 17 drops four frames without a word.
///
/// Measured at two sites, because the retarget sees an off-grid import and
/// this module sees an export that resampled. Correction 12 has the detail.
pub const FPS_GRID: Rule = Rule {
    id: "clip.fps_grid",
    comparison: Comparison::Le,
    unit: "frames",
    space: "each key of the clip, against the frame grid its declared source_fps sets",
    limit: |profile| profile.clip.fps_grid_frames,
};

/// And the retarget samples exactly the frames the source authored.
///
/// A 30 fps clip read in a 24 fps scene spans 0.8 to 16.8, rounds to 1 to 17,
/// and drops four of its 21 frames. Measured inside Blender, on the action,
/// because the exported file carries what was sampled rather than what was
/// there.
pub const FPS_GRID_RANGE: Rule = Rule {
    id: "clip.fps_grid.range",
    comparison: Comparison::Eq,
    unit: "frames",
    space: "the frames the retarget samples, against the source's own key times",
    limit: |_| 0.0,
};

/// The root stays put once the bake has stripped its horizontal motion.
///
/// A maximum over frames rather than an endpoint difference, because an
/// endpoint difference cancels a symmetric excursion. It can only ever read a
/// residual, so it is not a second [`super::source::TRAVELING`], which asks
/// whether the vendor sent the travel, or [`super::source::WANDER`], which is
/// the excursion the pin removed.
pub const ROOT_TRAVEL: Rule = Rule {
    id: "clip.root_travel",
    comparison: Comparison::Le,
    unit: "meters",
    space: "the root bone's world head against its first frame, per horizontal axis, \
            after strip_root_motion",
    limit: |profile| profile.clip.root_travel_meters,
};

/// And the up axis keeps its bob, which is a rule of its own.
///
/// The strip keeps this axis because a bob is animation, so the limit is
/// calibrated on the bob rather than on a residual: wide enough for the
/// 0.0535 m a run reads, narrow enough to reject the 0.2911 m that pinning
/// the root's own channels 0 and 1 sinks a left strafe by.
pub const ROOT_BOB: Rule = Rule {
    id: "clip.root_bob",
    comparison: Comparison::Le,
    unit: "meters",
    space: "the root bone's world head against its first frame, on the up axis, \
            after strip_root_motion",
    limit: |profile| profile.clip.root_bob_meters,
};

/// A clip's lowest ground joint returns to the height its rig rests at.
///
/// The retarget lifts the whole clip until it does, and this is what is left
/// under that joint. A clip fitted an inch into the tile or an inch above it
/// looks the same alone and wrong beside every other clip on it. Measured at
/// two sites, like [`FPS_GRID`]: the retarget sees the snap itself, and this
/// module sees an export that resampled the root.
pub const FLOOR_SNAP: Rule = Rule {
    id: "clip.floor_snap",
    comparison: Comparison::Le,
    unit: "meters",
    space: GROUND,
    limit: |profile| profile.clip.floor_snap_meters,
};

/// The clip travels as far as its source did, sized by the femur.
///
/// A longer thigh takes a longer step, so a clip bought on a taller rig is
/// sized before its travel means anything on this body, and total height
/// cannot say by how much: it carries the head and the feet, and neither
/// takes a step. Relative, so 2 percent means the same on a 0.4 m shuffle
/// and a 2.3 m strafe. See the Test Plan row.
pub const STRIDE: Rule = Rule {
    id: "clip.stride",
    comparison: Comparison::Le,
    unit: "percent",
    space: SIZED,
    limit: |profile| profile.clip.stride_percent,
};

/// And what that femur ratio was, on record rather than printed.
///
/// Records, never gates: a Mixamo rig and a refit of our own clip are both
/// correct and no threshold reads both. The number is the point, and
/// [`STRIDE`] is meaningless without it. See the Test Plan row.
pub const STRIDE_RATIO: Rule = Rule {
    id: "clip.stride_ratio",
    comparison: Comparison::Le,
    unit: "ratio",
    space: SEGMENT,
    limit: |_| A_HUNDREDFOLD,
};

/// Where the fit's joints ended up, against the source's own.
///
/// ```text
/// body(m, role, t)   = joint(m, role, t) - joint(m, root, t)
/// clip.posture(role) = max over t of
///                      |body(out, role, t) - ratio * body(src, role, t)|
/// ```
///
/// Against the root joint, so a clip's travel and the floor snap cancel and
/// what is left is the shape of the body. Sized by the femur ratio
/// [`STRIDE_RATIO`] records, because our limbs are not the source's length.
///
/// **This is the rule no other clip gate could be.** [`SWING`] and [`TWIST`]
/// read each bone's own frame, and nothing else reads a joint chain: a fit
/// that pointed every bone correctly out of a hips frame 97.612 degrees off
/// the direction to its own spine passed all thirteen and shipped a hunch in
/// every frame of every direction of two clips. Per role and not per clip, so
/// the subject says which joint moved.
pub const POSTURE: Rule = Rule {
    id: "clip.posture",
    comparison: Comparison::Le,
    unit: "meters",
    space: BODY,
    limit: |profile| profile.clip.posture_meters,
};

/// A looping clip ends in the pose it started in.
///
/// Playback jumps from the last key back to the first, so a pose that has not
/// come back around is a visible hitch once a loop. `skipped` for a clip that
/// does not loop, where the two ends have no reason to agree at all.
pub const LOOP: Rule = Rule {
    id: "clip.loop",
    comparison: Comparison::Le,
    unit: "degrees",
    space: "the local rotation of each joint at the clip's last key, against its first",
    limit: |profile| profile.clip.loop_degrees,
};

/// Every rule this module owns, in the order `--list-rules` prints them.
pub const RULES: [&Rule; 14] = [
    &INTERPOLATION,
    &REFERENCE_POSE_KEY,
    &FPS_GRID,
    &FPS_GRID_RANGE,
    &FLOOR_SNAP,
    &STRIDE,
    &STRIDE_RATIO,
    &SWING,
    &TWIST,
    &POSTURE,
    &OBJECT_TRANSFORM,
    &ROOT_TRAVEL,
    &ROOT_BOB,
    &LOOP,
];

/// One fitted clip, as the file-side rules need it: the file itself, the
/// sidecar the retarget wrote beside it, the rig it was fitted to, and the
/// two things the library declares about it.
///
/// The declarations arrive as the two values they are rather than as the
/// record that holds them: this module measures, and what an animation
/// library looks like is not its business.
pub struct Fitted<'a> {
    pub output: &'a Path,
    pub source_motion: &'a Path,
    pub rig: &'a Path,
    pub repo_root: &'a Path,
    pub source_fps: u32,
    pub loops: bool,
    pub travels: bool,
}

/// The rules `retarget_animation.py` reports, whose limits the runner
/// publishes to it on argv. [`FPS_GRID`], [`FLOOR_SNAP`] and the three
/// [`foot`] rules are in both lists on purpose: the retarget reads the pose it
/// evaluated and this module reads the delivered file.
pub const RETARGET_RULES: [&Rule; 10] = [
    &INTERPOLATION,
    &REFERENCE_POSE_KEY,
    &FPS_GRID,
    &FPS_GRID_RANGE,
    &FLOOR_SNAP,
    &STRIDE,
    &STRIDE_RATIO,
    &foot::PLANTS,
    &foot::SKATE,
    &foot::PENETRATION,
];

/// And the two `bake_sprites.py` reports.
pub const BAKE_RULES: [&Rule; 2] = [&ROOT_TRAVEL, &ROOT_BOB];

/// The rules [`check_files`] reads off the delivered GLB, which is also the
/// list it reports as undefined when that file is not there.
pub const FILE_RULES: [&Rule; 12] = [
    &SWING,
    &TWIST,
    &POSTURE,
    &OBJECT_TRANSFORM,
    &FPS_GRID,
    &LOOP,
    &FLOOR_SNAP,
    &STRIDE,
    &STRIDE_RATIO,
    &foot::PLANTS,
    &foot::SKATE,
    &foot::PENETRATION,
];

/// What a per-axis bake subject names itself, after the clip's own name.
/// `framing.AXES` spells the same three.
pub const AXES: [&str; 3] = ["x", "y", "z"];

/// Runs every file-side clip rule on one fitted clip, which is [`FILE_RULES`].
///
/// A missing or unreadable input is one error finding per rule, never a skip:
/// a gate that goes quiet on absent input proves nothing.
pub fn check_files(
    fitted: &Fitted<'_>,
    profile: &Profile,
    table: &AimTable,
    attempt: u32,
) -> Result<Vec<Finding>> {
    let &Fitted {
        output,
        source_motion,
        rig,
        repo_root,
        source_fps,
        loops,
        travels,
    } = fitted;
    let bones = table.bones(table.canonical())?.clone();
    // The roles that stand on the floor are skeleton data, so the toes are
    // named by the table rather than by anything spelling "toe".
    let toes: BTreeSet<String> = table
        .ground_roles()
        .iter()
        .map(|role| filled_by(&bones, role))
        .collect::<Result<_>>()?;
    // The role every joint is read against. `[profile] single_root` names the
    // bone, and a role is the only key the source's own record also carries.
    let root = role_of(&bones, &profile.single_root)?;
    let clip = relative_to(output, repo_root);
    let bytes = match std::fs::read(output) {
        Ok(bytes) => bytes,
        Err(error) => {
            return Ok(FILE_RULES
                .map(|rule| {
                    rule.undefined(&clip, attempt, format!("{clip} cannot be read: {error}"))
                })
                .to_vec());
        }
    };
    // Before the pair, because `clip.posture` is sized by the same ratio: our
    // limbs are not the source's length, so an unsized comparison would hold
    // our own body to a taller one.
    let sized = stride_of(&bytes, fitted, &bones, table.stride_segment(), profile);
    let ratio = sized.as_ref().ok().and_then(Stride::ratio);
    let compared = match (gltf_clip::read(&bytes, &bones), Motion::read(source_motion)) {
        (Ok(output), Ok(source)) => compare(
            &Compared {
                output: &output,
                source: &source,
                bones: &bones,
                root: &root,
                ratio,
            },
            profile,
            attempt,
        ),
        (output, source) => {
            let error = output
                .err()
                .or(source.err())
                .expect("one of the two failed");
            [&SWING, &TWIST, &POSTURE]
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
    let grid = match gltf_clip::keys(&bytes) {
        Ok(keys) => [
            fps_grid(&keys, source_fps, profile, attempt),
            closes_the_loop(&keys, loops, profile, attempt),
        ]
        .concat(),
        Err(error) => [&FPS_GRID, &LOOP]
            .map(|rule| {
                rule.undefined(
                    &clip,
                    attempt,
                    format!("{clip} carries no readable key grid: {error:#}"),
                )
            })
            .to_vec(),
    };
    let floor = match gltf_clip::soles(&bytes, &toes) {
        Ok(soles) => floor_snap(&soles, &clip, profile, attempt),
        Err(error) => FLOOR_SNAP.undefined(
            &clip,
            attempt,
            format!("{clip} carries no readable sole point: {error:#}"),
        ),
    };
    let sized = match sized {
        Ok(reading) => vec![
            stride(&reading, &clip, profile, attempt),
            stride_ratio(&reading, &clip, profile, attempt),
        ],
        Err(error) => [&STRIDE, &STRIDE_RATIO]
            .map(|rule| {
                rule.undefined(
                    &clip,
                    attempt,
                    format!(
                        "{clip} cannot be measured against the travel its source \
                         had: {error:#}"
                    ),
                )
            })
            .to_vec(),
    };
    let planted = match soles_and_span(&bytes, rig, &toes) {
        Ok((soles, height)) => {
            foot::check(&clip, &soles, height, source_fps, travels, profile, attempt)
        }
        Err(error) => foot::undefined(
            &clip,
            attempt,
            &format!("{clip} carries no readable sole to stand on: {error:#}"),
        ),
    };
    Ok([compared, objects, grid, vec![floor], sized, planted].concat())
}

/// One clip's soles, and the joint span of the rig it was fitted to.
///
/// The span comes off the rig rather than off the clip, so this and
/// `retarget_animation.py::rig_height` scale the contact thresholds by one
/// number read on one file.
fn soles_and_span(
    bytes: &[u8],
    rig: &Path,
    toes: &BTreeSet<String>,
) -> Result<(gltf_clip::Soles, f64)> {
    let soles = gltf_clip::soles(bytes, toes)?;
    let span = gltf_clip::joint_span(
        &std::fs::read(rig).with_context(|| format!("reading {}", rig.display()))?,
    )?;
    Ok((soles, span))
}

/// The bone filling one role, on the canonical convention.
///
/// [`AimTable`] refuses a `ground_roles` or `stride_segment` row naming
/// something no convention maps, so this cannot fail on a committed table.
/// It still says which role went missing rather than dropping it.
fn filled_by(bones: &BTreeMap<String, String>, role: &str) -> Result<String> {
    bones
        .get(role)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("the canonical convention maps no bone to {role}"))
}

/// The role one bone fills, on the canonical convention.
///
/// The other way around from [`filled_by`]: `[profile] single_root` names a
/// bone of our own rig, and every absolute rule is keyed by role because that
/// is the only key the source's own record shares.
fn role_of(bones: &BTreeMap<String, String>, bone: &str) -> Result<String> {
    bones
        .iter()
        .find(|(_, filled)| filled.as_str() == bone)
        .map(|(role, _)| role.clone())
        .ok_or_else(|| anyhow::anyhow!("the canonical convention gives {bone} no role"))
}

/// The two lengths and the two travels [`STRIDE`] reads, out of three files:
/// the delivered clip, the rig it was fitted to, and the sidecar the retarget
/// wrote for the vendor file.
///
/// Our own segment comes off the rig GLB and not the sidecar, so the two
/// sides of the ratio have two readers.
fn stride_of(
    bytes: &[u8],
    fitted: &Fitted<'_>,
    bones: &BTreeMap<String, String>,
    roles: &[String; 2],
    profile: &Profile,
) -> Result<Stride> {
    let rig = Skeleton::read(fitted.rig)?;
    let joint = |role: &String| -> Result<DVec3> {
        let bone = filled_by(bones, role)?;
        Ok(rig
            .get(&bone)
            .with_context(|| format!("{bone} fills {role} and the rig has no such joint"))?
            .position())
    };
    let (upper, lower) = (joint(&roles[0])?, joint(&roles[1])?);
    let source = SourceLengths::read(fitted.source_motion)?;
    Ok(Stride {
        travel: gltf_clip::travel(bytes, &profile.single_root)?,
        segment: ((lower - upper).length(), source.stride_segment),
        source_travel: source.travel,
        travels: fitted.travels,
    })
}

/// What [`STRIDE`] and [`STRIDE_RATIO`] are read from, once per clip.
#[derive(Debug, Clone, Copy)]
pub struct Stride {
    /// How far the delivered clip's root got, horizontally, in meters.
    pub travel: f64,
    /// Our stride segment at rest and the source's, in meters.
    pub segment: (f64, f64),
    /// How far the source's own root got, in meters.
    pub source_travel: f64,
    /// What the library declares about the source's root motion.
    pub travels: bool,
}

impl Stride {
    /// How much longer our own stride segment is than the source's, which is
    /// what every location key was sized by. `None` when either rig has no
    /// segment to measure, where there is no ratio at all.
    fn ratio(&self) -> Option<f64> {
        let (ours, theirs) = self.segment;
        (ours > 0.0 && theirs > 0.0).then(|| ours / theirs)
    }

    /// The travel this fit has to reproduce: the source's, sized.
    fn wanted(&self) -> Option<f64> {
        self.ratio()
            .map(|ratio| self.source_travel * ratio)
            .filter(|wanted| *wanted > 0.0)
    }
}

/// `clip.stride`, on the delivered file's own root positions.
pub fn stride(reading: &Stride, clip: &str, profile: &Profile, attempt: u32) -> Finding {
    if !reading.travels {
        return STRIDE.skipped(
            profile,
            clip,
            attempt,
            format!(
                "the library declares travels: false, so {clip} has no \
                 source travel to be sized against"
            ),
        );
    }
    let (Some(ratio), Some(wanted)) = (reading.ratio(), reading.wanted()) else {
        return STRIDE.undefined(
            clip,
            attempt,
            format!(
                "the source of {clip} travels {:.4} m on a stride segment of \
                 {:.4} m against our {:.4} m, so there is no sized travel to \
                 read it against",
                reading.source_travel, reading.segment.1, reading.segment.0
            ),
        );
    };
    STRIDE.measured(
        profile,
        clip,
        (reading.travel - wanted).abs() / wanted * 100.0,
        attempt,
        format!(
            "{clip} travels {:.4} m against the {wanted:.4} m its source's \
             {:.4} m comes to at a femur ratio of {ratio:.4}",
            reading.travel, reading.source_travel
        ),
    )
}

/// `clip.stride_ratio`, with our own length read off the rig rather than
/// taken from the retarget.
pub fn stride_ratio(reading: &Stride, clip: &str, profile: &Profile, attempt: u32) -> Finding {
    let (ours, theirs) = reading.segment;
    let Some(ratio) = reading.ratio() else {
        return STRIDE_RATIO.undefined(
            clip,
            attempt,
            format!("a stride segment of {ours:.4} m against {theirs:.4} m is no ratio"),
        );
    };
    STRIDE_RATIO.measured(
        profile,
        clip,
        ratio,
        attempt,
        format!(
            "our stride segment is {ours:.4} m against the source's \
             {theirs:.4} m, so every length of {clip} is sized by {ratio:.4}"
        ),
    )
}

/// `clip.floor_snap`, on the lowest sole point the delivered file carries.
///
/// The sole and not the joint: a joint's rest height is only where the
/// contact patch is while the foot keeps its rest pitch, and a cross-rig fit
/// matches the source's pitch instead. One model, read here and by
/// `clip.foot_contact.penetration`, so standing on the floor means one thing.
///
/// One finding, not one per foot: what the retarget lifted is the whole clip,
/// so the reading is the lowest any sole point gets and the subject says
/// which foot that was.
pub fn floor_snap(
    soles: &gltf_clip::Soles,
    clip: &str,
    profile: &Profile,
    attempt: u32,
) -> Finding {
    let low = soles
        .feet
        .iter()
        .flat_map(|foot| {
            soles
                .seconds
                .iter()
                .zip(foot.ball.iter().zip(&foot.heel))
                .flat_map(|(seconds, (ball, heel))| {
                    [
                        (foot.toe.as_str(), ball.z, *seconds),
                        (foot.toe.as_str(), heel.z, *seconds),
                    ]
                })
        })
        .min_by(|a, b| a.1.total_cmp(&b.1));
    let Some((toe, height, seconds)) = low else {
        return FLOOR_SNAP.undefined(
            clip,
            attempt,
            format!("{clip} carries no sole point, so it has no floor to be read against"),
        );
    };
    FLOOR_SNAP.measured(
        profile,
        &format!("{toe} at {seconds:.6} s"),
        height.abs(),
        attempt,
        format!(
            "{toe} at {seconds:.6} s is the lowest any sole point of the clip \
             gets, {height:.4} m from the floor the snap aims at"
        ),
    )
}

/// `clip.fps_grid`, one finding per key of the delivered file.
///
/// Per key rather than per worst, because which key drifted is what says
/// whether the rate is wrong or one key is.
pub fn fps_grid(
    keys: &gltf_clip::Keys,
    source_fps: u32,
    profile: &Profile,
    attempt: u32,
) -> Vec<Finding> {
    if source_fps == 0 {
        return vec![FPS_GRID.undefined(
            "the whole clip",
            attempt,
            "the library declares a source_fps of 0, which is no rate at all".to_owned(),
        )];
    }
    keys.seconds
        .iter()
        .map(|seconds| {
            let frame = seconds * f64::from(source_fps);
            let off = (frame - frame.round()).abs();
            FPS_GRID.measured(
                profile,
                &format!("key at {seconds:.6} s"),
                off,
                attempt,
                format!(
                    "{seconds:.6} s is frame {frame:.4} at {source_fps} fps, {off:.6} frames off a whole one"
                ),
            )
        })
        .collect()
}

/// `clip.loop`, one finding per joint, or one skip per joint when the library
/// says this clip does not repeat.
pub fn closes_the_loop(
    keys: &gltf_clip::Keys,
    loops: bool,
    profile: &Profile,
    attempt: u32,
) -> Vec<Finding> {
    keys.ends
        .iter()
        .map(|(bone, (first, last))| {
            if !loops {
                return LOOP.skipped(
                    profile,
                    bone,
                    attempt,
                    format!("{bone}: the library declares this clip does not repeat"),
                );
            }
            // A quaternion and its negation are the same rotation, hence the
            // absolute value: without it half of these read a full turn out.
            let apart = 2.0 * first.dot(*last).abs().min(1.0).acos().to_degrees();
            LOOP.measured(
                profile,
                bone,
                apart,
                attempt,
                format!("{bone} ends {apart:.3} degrees from the pose it started in"),
            )
        })
        .collect()
}

/// The pair of clips the three absolute rules read, and what sizes them.
pub struct Compared<'a> {
    /// The delivered GLB, as [`gltf_clip::read`] read it.
    pub output: &'a Motion,
    /// The vendor file, as the retarget's sidecar recorded it.
    pub source: &'a Motion,
    /// Role to the bone that fills it on our own rig, for the messages.
    pub bones: &'a BTreeMap<String, String>,
    /// The role every joint is measured against, which is our rig's root.
    pub root: &'a str,
    /// Our stride segment over the source's, which every length was sized by.
    /// `None` when either rig has no segment to measure, where
    /// [`POSTURE`] has no scale to read and says so.
    pub ratio: Option<f64>,
}

/// `clip.swing`, `clip.twist` and `clip.posture`, one finding each per role.
/// Pure: both motions are already read.
///
/// The subjects are every role the canonical convention maps together with
/// every role the source drove, so a role one side is missing is reported
/// rather than dropped. Nothing here is skipped for being absent.
pub fn compare(pair: &Compared<'_>, profile: &Profile, attempt: u32) -> Vec<Finding> {
    let roles: BTreeSet<&str> = pair
        .bones
        .keys()
        .map(String::as_str)
        .chain(pair.source.roles())
        .collect();
    if pair.output.frames().len() != pair.source.frames().len() {
        return misaligned(
            &roles,
            attempt,
            &format!(
                "the clip carries {} frame(s) and the source has {}",
                pair.output.frames().len(),
                pair.source.frames().len()
            ),
        );
    }
    if let Some(apart) = worst_time_gap(pair.output, pair.source) {
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
        .flat_map(|role| measure(role, pair, profile, attempt))
        .collect()
}

/// The worst reading of one rule over one clip, and when it happened. In
/// whatever unit the rule publishes: degrees for two of the three, meters for
/// [`POSTURE`].
#[derive(Default)]
struct Worst {
    reading: f64,
    seconds: f64,
}

impl Worst {
    fn keep(&mut self, reading: f64, seconds: f64) {
        if reading > self.reading {
            *self = Self { reading, seconds };
        }
    }
}

/// All three rules on one role, or the reason none can be measured.
fn measure(role: &str, pair: &Compared<'_>, profile: &Profile, attempt: u32) -> Vec<Finding> {
    let (output, source) = (pair.output, pair.source);
    let bone = pair.bones.get(role).map_or(role, String::as_str);
    let (Some(out_rest), Some(src_rest)) = (output.rest(role), source.rest(role)) else {
        let missing = if output.rest(role).is_none() {
            format!("the clip has no bone for the {role} role, so nothing carries its motion")
        } else {
            format!("the source clip drives no {role}, so there is nothing to measure against")
        };
        return [&SWING, &TWIST, &POSTURE]
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
            swing.reading,
            attempt,
            format!(
                "{bone} points {:.6} degrees from the source's {role} at {} s",
                swing.reading, swing.seconds
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
                twist.reading,
                attempt,
                format!(
                    "{bone} is rolled {:.3} degrees from the {:.3} degrees the two bind \
                     poses call for, at {} s",
                    twist.reading,
                    at_rest
                        .twist
                        .expect("a rest roll, or `singular` would carry the reason"),
                    twist.seconds
                ),
            ),
        },
        posture(role, bone, pair, profile, attempt),
    ]
}

/// `clip.posture` on one role: how far our joint sits from the source's, both
/// read against their own root joint and the source's sized by the femur.
fn posture(
    role: &str,
    bone: &str,
    pair: &Compared<'_>,
    profile: &Profile,
    attempt: u32,
) -> Finding {
    let Some(ratio) = pair.ratio else {
        return POSTURE.undefined(
            role,
            attempt,
            format!(
                "neither rig has a stride segment to size {bone} by, so its joint \
                 has nothing to be read against"
            ),
        );
    };
    let mut worst = Worst::default();
    for (out, src) in pair.output.frames().iter().zip(pair.source.frames()) {
        let body = |frame: &super::motion::Frame| {
            Some(*frame.joints.get(role)? - *frame.joints.get(pair.root)?)
        };
        let (Some(ours), Some(theirs)) = (body(out), body(src)) else {
            return POSTURE.undefined(
                role,
                attempt,
                format!(
                    "one of the two clips puts no joint on {bone} or on its {} root \
                     at {} s",
                    pair.root, out.seconds
                ),
            );
        };
        worst.keep((ours - theirs * ratio).length(), out.seconds);
    }
    POSTURE.measured(
        profile,
        role,
        worst.reading,
        attempt,
        format!(
            "{bone} sits {:.4} m from where the source's {role} has it, against \
             the root joint and sized by {ratio:.4}, at {} s",
            worst.reading, worst.seconds
        ),
    )
}

/// One undefined finding per role per rule, for a fault that makes the whole
/// pair unmeasurable. Every subject is still reported, because a rule that
/// goes quiet cannot be told from one that never ran.
fn misaligned(roles: &BTreeSet<&str>, attempt: u32, why: &str) -> Vec<Finding> {
    roles
        .iter()
        .flat_map(|role| {
            [&SWING, &TWIST, &POSTURE].map(|rule| rule.undefined(role, attempt, why.to_owned()))
        })
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
