//! The rig gates: fourteen rules, every limit read from `[profile]`.
//!
//! The committed rig is auto-rigger output that nothing ever measured. It
//! carries elbows bent 24 degrees at rest, arms 59 degrees below horizontal
//! against a spec of 40, up to 3.7 percent left against right asymmetry, a
//! `Hips` whose own +Y points out of a hip socket, a `Head` pitched 26
//! degrees off its own child, and three bones under names no convention
//! uses. Every one of those went undetected because nothing here existed.
//!
//! **Every rule reports on every subject it can resolve, whether or not the
//! measurement holds.** A rule that stays quiet on good art is
//! indistinguishable from a rule that never ran, and "reports success having
//! done nothing" is a failure this pipeline has already shipped. The
//! comparison decides the severity, and only an error stops a build.
//!
//! Nothing here reads a bone tail. Directions come from joint positions, in
//! world space, through the whole glTF node chain.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use glam::DVec3;

use super::gltf_world::{
    SHORTEST_SEGMENT_METERS, Skeleton, blender_to_gltf, degrees_between, gltf_to_blender,
};
use super::profile::{Axis, LEFT, Profile, RIGHT, reflected};
use super::{Comparison, Finding, HALF_A_TURN, NOT_MIRRORED, Rule, Symmetry, relative_to};

/// The stage these findings belong to, which names their report file: the
/// rigged file as the vendor shipped it.
pub const STAGE: &str = "rig";

/// The same rules on the file the rename and the conform wrote, which is the
/// one that gates. Two stage names over one rule set, the way `mesh` and
/// `cleaned` already read the mesh before and after the fixer.
pub const CONFORMED_STAGE: &str = "conformed";

/// What each of those two files is called in a sentence. A stage name is not
/// one: "no conformed yet at ..." reads as a missing word.
pub const NOUN: &str = "vendor rig";
pub const CONFORMED_NOUN: &str = "conformed rig";

/// The upper arm. Its angle below horizontal is what the concept prompt asks
/// for and what nothing ever checked.
const HUMERUS: &str = "Arm";
/// The forearm, and what it ends in. [`ELBOW_BEND`] needs all three joints,
/// so it names them rather than taking a tail from the profile.
const FOREARM: &str = "ForeArm";
const HAND: &str = "Hand";
/// The bone whose tail direction says which way the character faces.
const FOOT: &str = "Foot";

const WORLD: &str = "world space, through the whole glTF node chain, from joint positions";
const BLENDER: &str = "Blender Z-up world space, after the glTF Y-up conversion";
const GRAPH: &str = "the joint names in the glTF node graph";

pub const NAMES_STANDARD: Rule = Rule {
    id: "rig.names_standard",
    comparison: Comparison::Eq,
    unit: "bones",
    space: GRAPH,
    limit: |_| 0.0,
};

pub const BONE_SET: Rule = Rule {
    id: "rig.bone_set",
    comparison: Comparison::Eq,
    unit: "bones",
    space: GRAPH,
    limit: |_| 1.0,
};

pub const SINGLE_ROOT: Rule = Rule {
    id: "rig.single_root",
    comparison: Comparison::Eq,
    unit: "bones",
    space: "the joint ancestry in the glTF node graph",
    limit: |_| 0.0,
};

pub const PARENTS: Rule = Rule {
    id: "rig.parents",
    comparison: Comparison::Eq,
    unit: "bones",
    space: "the nearest joint above each joint in the glTF node graph",
    limit: |_| 0.0,
};

pub const CHILD_AXIS: Rule = Rule {
    id: "rig.child_axis",
    comparison: Comparison::Le,
    unit: "degrees",
    space: WORLD,
    limit: |profile| profile.child_axis_tolerance_degrees,
};

pub const MIRROR_LENGTH: Rule = Rule {
    id: "rig.mirror_length",
    comparison: Comparison::Le,
    unit: "percent",
    space: WORLD,
    limit: |profile| profile.mirror_tolerance_percent,
};

pub const MIRROR_DIRECTION: Rule = Rule {
    id: "rig.mirror_direction",
    comparison: Comparison::Le,
    unit: "degrees",
    space: "world space, the left segment against the right one reflected across X = 0",
    limit: |profile| profile.mirror_tolerance_degrees,
};

pub const HUMERUS_ANGLE: Rule = Rule {
    id: "rig.humerus_angle",
    comparison: Comparison::Le,
    unit: "degrees",
    space: "world space, below the horizontal plane of the up axis",
    limit: |profile| profile.humerus_below_horizontal.tolerance,
};

/// How far the forearm sits out of line with its own upper arm at rest.
///
/// Records, never gates. The bend a generator leaves is the second
/// acceptance item of the `pose_mode` spike, and a published limit would
/// fail the committed rig, which bends 24 degrees, on every run until that
/// rig is regenerated. The reading is what the decision needs.
pub const ELBOW_BEND: Rule = Rule {
    id: "rig.elbow_bend",
    comparison: Comparison::Le,
    unit: "degrees",
    space: "world space, the forearm direction against the upper arm direction",
    limit: |_| HALF_A_TURN,
};

pub const FACING: Rule = Rule {
    id: "rig.facing",
    comparison: Comparison::Eq,
    unit: "axes",
    space: "glTF Y-up world space, the horizontal part of the foot to toe direction",
    limit: |_| 0.0,
};

pub const UP_AXIS: Rule = Rule {
    id: "rig.up_axis",
    comparison: Comparison::Eq,
    unit: "axes",
    space: BLENDER,
    limit: |_| 0.0,
};

pub const BIND_DEVIATION: Rule = Rule {
    id: "rig.bind_deviation",
    comparison: Comparison::Le,
    unit: "degrees",
    space: BLENDER,
    limit: |profile| profile.max_bind_deviation_degrees,
};

pub const WORLD_HEIGHT: Rule = Rule {
    id: "rig.world_height",
    comparison: Comparison::Le,
    unit: "percent",
    space: "world space, the joint span along the up axis against spec.subject.height_meters",
    limit: |profile| profile.height_tolerance_percent,
};

pub const OBJECT_TRANSFORM: Rule = Rule {
    id: "rig.object_transform",
    comparison: Comparison::Eq,
    unit: "channels",
    space: "the glTF animation channels, targets above the skeleton",
    limit: |_| 0.0,
};

/// Every rig rule, in the order `--list-rules` prints them.
pub const RULES: [&Rule; 14] = [
    &NAMES_STANDARD,
    &BONE_SET,
    &SINGLE_ROOT,
    &PARENTS,
    &CHILD_AXIS,
    &MIRROR_LENGTH,
    &MIRROR_DIRECTION,
    &HUMERUS_ANGLE,
    &ELBOW_BEND,
    &FACING,
    &UP_AXIS,
    &BIND_DEVIATION,
    &WORLD_HEIGHT,
    &OBJECT_TRANSFORM,
];

/// The three rules a rename closes, and the only ones a bought rig is held to
/// before the conform runs: every profile row below them is keyed by bone
/// name, so a rename that half worked would measure the wrong joints.
pub const NAME_RULES: [&Rule; 3] = [&NAMES_STANDARD, &BONE_SET, &PARENTS];

/// Runs every rig rule on one file.
///
/// A missing or unreadable file is one error finding, never a skip: a gate
/// that goes quiet on absent input proves nothing.
pub fn check_file(
    file: &Path,
    repo_root: &Path,
    profile: &Profile,
    height_meters: f64,
    symmetry: Symmetry,
    attempt: u32,
) -> Result<Vec<Finding>> {
    let subject = relative_to(file, repo_root);
    match Skeleton::read(file) {
        Ok(skeleton) => Ok(check(
            &subject,
            &skeleton,
            profile,
            height_meters,
            symmetry,
            attempt,
        )),
        // Under `bone_set`, because a file that cannot be read holds none of
        // the bones the profile requires.
        Err(error) => Ok(vec![BONE_SET.undefined(
            &subject,
            attempt,
            format!("{subject} holds no readable skeleton: {error:#}"),
        )]),
    }
}

/// Runs every rig rule. Pure: the file is already read.
pub fn check(
    file: &str,
    skeleton: &Skeleton,
    profile: &Profile,
    height_meters: f64,
    symmetry: Symmetry,
    attempt: u32,
) -> Vec<Finding> {
    let rig = Measured {
        file,
        skeleton,
        profile,
        height_meters,
        symmetry,
        attempt,
    };
    [
        rig.names_standard(),
        rig.bone_set(),
        rig.single_root(),
        rig.parents(),
        rig.child_axis(),
        rig.mirror_length(),
        rig.mirror_direction(),
        rig.humerus_angle(),
        rig.elbow_bend(),
        rig.facing(),
        rig.up_axis(),
        rig.bind_deviation(),
        rig.world_height(),
        rig.object_transform(),
    ]
    .concat()
}

/// One rig, one profile, one pass.
struct Measured<'a> {
    file: &'a str,
    skeleton: &'a Skeleton,
    profile: &'a Profile,
    height_meters: f64,
    symmetry: Symmetry,
    attempt: u32,
}

impl<'a> Measured<'a> {
    fn measured(&self, rule: &Rule, subject: &str, measured: f64, message: String) -> Finding {
        rule.measured(self.profile, subject, measured, self.attempt, message)
    }

    fn undefined(&self, rule: &Rule, subject: &str, message: String) -> Finding {
        rule.undefined(subject, self.attempt, message)
    }

    /// Every name the rig carries, against the names the profile declares.
    fn names_standard(&self) -> Vec<Finding> {
        self.skeleton
            .joints()
            .iter()
            .map(|joint| {
                let known = self.profile.declares(&joint.name);
                self.measured(
                    &NAMES_STANDARD,
                    &joint.name,
                    if known { 0.0 } else { 1.0 },
                    if known {
                        format!("{} is a name the profile declares", joint.name)
                    } else {
                        format!(
                            "no convention names a bone {}, so no motion can be mapped to it",
                            joint.name
                        )
                    },
                )
            })
            .collect()
    }

    /// The other direction: each declared bone must be there, exactly once.
    fn bone_set(&self) -> Vec<Finding> {
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for joint in self.skeleton.joints() {
            *counts.entry(joint.name.as_str()).or_default() += 1;
        }
        self.profile
            .bones
            .iter()
            .map(|bone| {
                let count = counts.get(bone.as_str()).copied().unwrap_or_default();
                self.measured(
                    &BONE_SET,
                    bone,
                    count as f64,
                    match count {
                        0 => format!("the rig has no {bone}"),
                        1 => format!("{bone} is present once"),
                        // A duplicate leaves every rule below it ambiguous
                        // about which bone it measured.
                        _ => format!("the rig names {bone} {count} times"),
                    },
                )
            })
            .collect()
    }

    fn single_root(&self) -> Vec<Finding> {
        let root = self.profile.single_root.as_str();
        self.skeleton
            .joints()
            .iter()
            .map(|joint| {
                let top = self.top_of(&joint.name);
                let inside = top == root;
                self.measured(
                    &SINGLE_ROOT,
                    &joint.name,
                    if inside { 0.0 } else { 1.0 },
                    if joint.name == root {
                        format!("{root} is the root of the skeleton")
                    } else if inside {
                        format!("{} descends from {root}", joint.name)
                    } else {
                        format!("{} descends from {top}, not from {root}", joint.name)
                    },
                )
            })
            .collect()
    }

    /// The highest joint above this one, which is the root of its own tree.
    fn top_of(&self, bone: &'a str) -> &'a str {
        let mut at = bone;
        // The node graph is a tree, so this ends. Bounded anyway: a hang here
        // would read as a slow gate rather than as a broken one.
        for _ in 0..=self.skeleton.joints().len() {
            match self
                .skeleton
                .get(at)
                .and_then(|joint| joint.parent.as_deref())
            {
                Some(parent) => at = parent,
                None => break,
            }
        }
        at
    }

    fn parents(&self) -> Vec<Finding> {
        self.profile
            .parents
            .iter()
            // A bone the rig does not have at all is `rig.bone_set`'s
            // business. This rule is about where the bones it has hang.
            .filter_map(|(bone, declared)| Some((self.skeleton.get(bone)?, bone, declared)))
            .map(|(joint, bone, declared)| {
                let actual = joint.parent.as_deref().unwrap_or("nothing");
                let right = actual == declared;
                self.measured(
                    &PARENTS,
                    bone,
                    if right { 0.0 } else { 1.0 },
                    if right {
                        format!("{bone} hangs from {declared}")
                    } else {
                        format!("{bone} hangs from {actual}, the profile says {declared}")
                    },
                )
            })
            .collect()
    }

    /// Does each bone's own axis point at the child the profile names?
    fn child_axis(&self) -> Vec<Finding> {
        let axis = self.profile.child_axis;
        self.profile
            .tails
            .iter()
            .filter(|(bone, tail)| self.has(bone) && self.has(tail))
            .map(|(bone, tail)| {
                let Some(joint_axis) = self.skeleton.get(bone).and_then(|joint| joint.axis(axis))
                else {
                    return self.undefined(
                        &CHILD_AXIS,
                        bone,
                        format!("{bone} has no {axis} axis, its scale is zero"),
                    );
                };
                let Some(direction) = self.skeleton.direction(bone, tail) else {
                    return self.undefined(
                        &CHILD_AXIS,
                        bone,
                        format!("{bone} and {tail} sit at the same place"),
                    );
                };
                let off = degrees_between(joint_axis, direction);
                self.measured(
                    &CHILD_AXIS,
                    bone,
                    off,
                    format!(
                        "the {axis} axis of {bone} is {off:.3} degrees off the direction to {tail}"
                    ),
                )
            })
            .collect()
    }

    fn mirror_length(&self) -> Vec<Finding> {
        self.mirrored(&MIRROR_LENGTH, |left, right| {
            let (left, right) = (left.length(), right.length());
            (
                (left - right).abs() / ((left + right) / 2.0) * 100.0,
                format!("left {left:.5} m against right {right:.5} m"),
            )
        })
    }

    fn mirror_direction(&self) -> Vec<Finding> {
        self.mirrored(&MIRROR_DIRECTION, |left, right| {
            let apart = degrees_between(left.normalize(), reflected(right).normalize());
            (apart, format!("{apart:.3} degrees apart once reflected"))
        })
    }

    /// One finding per mirrored segment, measured by whatever the caller
    /// makes of the left and right vectors.
    ///
    /// Both segments are length checked here, once, for every mirror rule: a
    /// difference of two nearly equal joint positions is cancellation noise,
    /// and dividing by it or normalizing it turns that noise into a precise
    /// number. See [`SHORTEST_SEGMENT_METERS`].
    fn mirrored(
        &self,
        rule: &Rule,
        measure: impl Fn(DVec3, DVec3) -> (f64, String),
    ) -> Vec<Finding> {
        let pairs = self.profile.mirror_pairs();
        if self.symmetry == Symmetry::Declined {
            return pairs
                .into_iter()
                .map(|pair| {
                    rule.skipped(
                        self.profile,
                        &pair.segment,
                        self.attempt,
                        NOT_MIRRORED.to_owned(),
                    )
                })
                .collect();
        }
        pairs
            .into_iter()
            .filter_map(|pair| {
                let sides = [&pair.left, &pair.right].map(|(bone, tail)| {
                    let (bone, tail) = (self.skeleton.get(bone)?, self.skeleton.get(tail)?);
                    Some(tail.position() - bone.position())
                });
                // Absent on one side or both: `rig.bone_set` reports the
                // missing bone, and there is no pair left to compare.
                let [Some(left), Some(right)] = sides else {
                    return None;
                };
                if left.length().min(right.length()) <= SHORTEST_SEGMENT_METERS {
                    return Some(self.undefined(
                        rule,
                        &pair.segment,
                        format!("a {} segment has no length", pair.segment),
                    ));
                }
                let (measured, message) = measure(left, right);
                Some(self.measured(
                    rule,
                    &pair.segment,
                    measured,
                    format!("the {} segments are {message}", pair.segment),
                ))
            })
            .collect()
    }

    /// The upper arm against the angle the concept prompt asks for.
    fn humerus_angle(&self) -> Vec<Finding> {
        let band = self.profile.humerus_below_horizontal;
        self.sided(HUMERUS)
            .map(|(bone, tail)| {
                let Some(direction) = self.skeleton.direction(&bone, &tail) else {
                    return self.undefined(
                        &HUMERUS_ANGLE,
                        &bone,
                        format!("{bone} and {tail} sit at the same place"),
                    );
                };
                let below = -gltf_to_blender(direction)
                    .dot(self.profile.up_axis.vector())
                    .clamp(-1.0, 1.0)
                    .asin()
                    .to_degrees();
                self.measured(
                    &HUMERUS_ANGLE,
                    &bone,
                    (below - band.target).abs(),
                    format!(
                        "{bone} sits {below:.1} degrees below horizontal, against a target \
                         of {}",
                        band.target
                    ),
                )
            })
            .collect()
    }

    /// How far each forearm sits out of line with its own upper arm.
    ///
    /// Three joints, so the profile's tails cannot name the pair: the upper
    /// arm's own direction is the one from the shoulder joint to the elbow.
    fn elbow_bend(&self) -> Vec<Finding> {
        [LEFT, RIGHT]
            .into_iter()
            .map(|side| {
                (
                    format!("{side}{HUMERUS}"),
                    format!("{side}{FOREARM}"),
                    format!("{side}{HAND}"),
                )
            })
            .filter(|(arm, forearm, hand)| self.has(arm) && self.has(forearm) && self.has(hand))
            .map(|(arm, forearm, hand)| {
                let (Some(upper), Some(lower)) = (
                    self.skeleton.direction(&arm, &forearm),
                    self.skeleton.direction(&forearm, &hand),
                ) else {
                    return self.undefined(
                        &ELBOW_BEND,
                        &forearm,
                        format!("{arm}, {forearm} and {hand} do not make two segments"),
                    );
                };
                let bend = degrees_between(upper, lower);
                self.measured(
                    &ELBOW_BEND,
                    &forearm,
                    bend,
                    format!("{forearm} sits {bend:.1} degrees out of line with {arm}"),
                )
            })
            .collect()
    }

    /// Which way the feet point, which is which way the character faces.
    fn facing(&self) -> Vec<Finding> {
        let declared = self.profile.facing_axis_gltf;
        let up = blender_to_gltf(self.profile.up_axis.vector());
        self.sided(FOOT)
            .map(|(bone, tail)| {
                let Some(direction) = self.skeleton.direction(&bone, &tail) else {
                    return self.undefined(
                        &FACING,
                        &bone,
                        format!("{bone} and {tail} sit at the same place"),
                    );
                };
                // The same reason as a mirrored segment: what is left after
                // the vertical part is removed can be nothing but noise.
                let flat = direction - up * direction.dot(up);
                if flat.length() <= SHORTEST_SEGMENT_METERS {
                    return self.undefined(
                        &FACING,
                        &bone,
                        format!("{bone} points straight along the up axis, so it faces nowhere"),
                    );
                }
                self.axis_finding(&FACING, &bone, flat.normalize(), declared, "glTF")
            })
            .collect()
    }

    /// Which way the body stands up.
    fn up_axis(&self) -> Vec<Finding> {
        let chain = self.profile.root_chain();
        let [bottom, .., top] = chain.as_slice() else {
            return Vec::new();
        };
        if !(self.has(bottom) && self.has(top)) {
            return Vec::new(); // `rig.bone_set` owns the missing bones.
        }
        let subject = format!("{bottom} to {top}");
        match self.skeleton.direction(bottom, top) {
            Some(direction) => vec![self.axis_finding(
                &UP_AXIS,
                &subject,
                gltf_to_blender(direction),
                self.profile.up_axis,
                "Blender",
            )],
            None => vec![self.undefined(
                &UP_AXIS,
                &subject,
                format!("{bottom} and {top} sit at the same place"),
            )],
        }
    }

    /// One direction against one declared axis. The number says whether the
    /// closest of the six axes is the declared one, and the message carries
    /// the angle, because an axis is a choice of six and not a tolerance.
    fn axis_finding(
        &self,
        rule: &Rule,
        subject: &str,
        direction: DVec3,
        declared: Axis,
        space: &str,
    ) -> Finding {
        let closest = Axis::closest_to(direction);
        let apart = degrees_between(direction, declared.vector());
        self.measured(
            rule,
            subject,
            if closest == declared { 0.0 } else { 1.0 },
            format!(
                "{subject} points {closest} in {space} space, {apart:.1} degrees from the \
                 declared {declared}"
            ),
        )
    }

    /// How far the rest pose leans, step by step up the root chain. The band
    /// is wide on purpose: it catches a rig lying down, not an A-pose.
    fn bind_deviation(&self) -> Vec<Finding> {
        let up = self.profile.up_axis.vector();
        self.profile
            .root_chain()
            .windows(2)
            .filter(|step| self.has(step[0]) && self.has(step[1]))
            .map(|step| {
                let (bone, tail) = (step[0], step[1]);
                let subject = format!("{bone} to {tail}");
                match self.skeleton.direction(bone, tail) {
                    Some(direction) => {
                        let leans = degrees_between(gltf_to_blender(direction), up);
                        self.measured(
                            &BIND_DEVIATION,
                            &subject,
                            leans,
                            format!("{subject} leans {leans:.1} degrees from the up axis"),
                        )
                    }
                    None => self.undefined(
                        &BIND_DEVIATION,
                        &subject,
                        format!("{bone} and {tail} sit at the same place"),
                    ),
                }
            })
            .collect()
    }

    /// The rig against the height the spec asks for. One height rule, and
    /// this is it.
    fn world_height(&self) -> Vec<Finding> {
        let up = self.profile.up_axis.vector();
        let along = self
            .skeleton
            .joints()
            .iter()
            .map(|joint| gltf_to_blender(joint.position()).dot(up));
        // A skeleton with no joints cannot be read at all, so the span of an
        // empty set never arises. Zero rather than a branch: it would report
        // the whole height as missing, which is what it would be.
        let low = along.clone().reduce(f64::min).unwrap_or_default();
        let span = along.reduce(f64::max).unwrap_or_default() - low;
        vec![self.measured(
            &WORLD_HEIGHT,
            self.file,
            (span - self.height_meters).abs() / self.height_meters * 100.0,
            format!(
                "the joints span {span:.4} m against the spec's {:.4} m",
                self.height_meters
            ),
        )]
    }

    /// An action on the armature object, which is what makes applying an
    /// object transform silently change the meaning of every location key.
    fn object_transform(&self) -> Vec<Finding> {
        let channels = self.skeleton.object_channels();
        let total: usize = channels.values().sum();
        vec![self.measured(
            &OBJECT_TRANSFORM,
            self.file,
            total as f64,
            if total == 0 {
                "no animation drives a node above the skeleton".to_owned()
            } else {
                let driven: Vec<&str> = channels.keys().map(String::as_str).collect();
                format!("{total} channels drive {}", driven.join(", "))
            },
        )]
    }

    fn has(&self, bone: &str) -> bool {
        self.skeleton.get(bone).is_some()
    }

    /// The same bone on both sides, with the tail the profile names for it.
    /// A side whose bone or tail is missing yields nothing, because
    /// `rig.bone_set` already reports it.
    fn sided(&self, stem: &'static str) -> impl Iterator<Item = (String, String)> {
        [LEFT, RIGHT].into_iter().filter_map(move |side| {
            let bone = format!("{side}{stem}");
            let tail = self.profile.tail(&bone)?.to_owned();
            (self.has(&bone) && self.has(&tail)).then_some((bone, tail))
        })
    }
}
