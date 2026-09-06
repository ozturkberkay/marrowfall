//! The mesh gates: fifteen rules, every limit read from `[profile]`.
//!
//! These rules exist because the last mesh audit was **confidently wrong**.
//! It read 13,368 boundary edges on a mesh that has 171, and a second tool
//! agreed to the integer because it shared the representation. So every rule
//! here names the representation it measured, and the representation is
//! built once, in [`gltf_mesh`]: world space first, then welded.
//!
//! **Every rule reports on every subject it can resolve**, whether or not
//! the measurement holds. A rule that stays quiet on good art cannot be told
//! from a rule that never ran, and "reports success having done nothing" is
//! a failure this pipeline has already shipped.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context as _, Result};
use glam::DVec3;
use parry3d::bounding_volume::{Aabb, BoundingVolume as _};
use parry3d::math::{Pose, Vector};
use parry3d::partitioning::{Bvh, BvhBuildStrategy};
use parry3d::query::intersection_test;
use parry3d::shape::Triangle;
use serde::Deserialize;

use super::gltf_mesh::{self, Surface};
use super::gltf_world::{SHORTEST_SEGMENT_METERS, blender_to_gltf};
use super::profile::{Axis, Profile};
use super::{Comparison, Finding, NOT_MIRRORED, Rule, Severity, Symmetry, relative_to};

/// The stage these findings belong to, which names their report file.
///
/// Three of them, because each carries one fixed rule set: [`FILE_RULES`] on
/// the mesh as it arrived, [`CLEANED_RULES`] on the file the fixer wrote, and
/// [`CLEANUP_RULES`] on the pair. One report for both files would carry
/// `mesh.texture` twice on a subject called `char1`, which is in both.
pub const STAGE: &str = "mesh";

/// [`CLEANED_RULES`], on the file the fixer wrote.
pub const CLEANED_STAGE: &str = "cleaned";

/// [`CLEANUP_RULES`], which read the two files against each other.
///
/// Not a `Stage`: the lock keeps its six, and this runs inside the rig stage,
/// between the mesh arriving and the credits being spent on it.
pub const CLEANUP_STAGE: &str = "cleanup";

/// How much of the height, measured from the lowest vertex, is the feet.
///
/// The mesh gates run before rigging, so there is no ankle joint to measure
/// from. On the survivor this slice holds 1,670 vertices whose center sits
/// 4.4 cm forward of the whole body's, against 0.6 mm sideways, so the
/// direction it names is not noise.
const FOOT_SLICE: f64 = 0.05;

/// Where the topology rules measured. The weld distance is written out
/// rather than formatted, because [`Rule::space`] is one static string; a
/// unit test pins it against [`gltf_mesh::WELD_METERS`].
const WELDED: &str = "world space through the glTF node chain, the skin's bind pose where the \
                      mesh is skinned, welded at 1e-5 m";
const CROSSING: &str = "the welded world-space triangles, pairs that intersect and share no \
                        vertex, via parry3d's Bvh";
const REFLECTED: &str =
    "the welded world-space vertices, each against its nearest neighbor reflected across X = 0";
const FEET: &str =
    "glTF Y-up world space, from the whole body's center to the center of its lowest 5 percent";
const GRAPH: &str = "the mesh node names in the glTF node graph";
const DELIVERED: &str = "the texture coordinate accessors, as delivered, before any weld";
const MATERIALS: &str = "the base color texture of each primitive's material";
const MODES: &str = "the primitive modes in the glTF node graph";
const REMOTE: &str = "Meshy print/analyze, which counts boundary and true non-manifold edges \
                      together, on the model task";
/// The two post-cleanup rules read the file the fixer wrote, in the same
/// representation as everything else. The second names the pair, because a
/// number that is only meaningful beside another one has to say so.
const CLEANED: &str = "the surface the fixer wrote, in world space and welded at 1e-5 m";
const AGAINST_THE_BARE_MESH: &str = "the surface the fixer wrote against the surface it was \
                                     given, both in world space and welded at 1e-5 m";

pub const HOLES: Rule = Rule {
    id: "mesh.holes",
    comparison: Comparison::Le,
    unit: "boundary edges",
    space: WELDED,
    limit: |profile| profile.mesh.holes,
};

pub const NON_MANIFOLD: Rule = Rule {
    id: "mesh.non_manifold",
    comparison: Comparison::Le,
    unit: "edges",
    space: WELDED,
    limit: |profile| profile.mesh.non_manifold_edges,
};

pub const ISLANDS: Rule = Rule {
    id: "mesh.islands",
    comparison: Comparison::Le,
    unit: "islands",
    space: WELDED,
    limit: |profile| profile.mesh.islands,
};

pub const SELF_INTERSECT: Rule = Rule {
    id: "mesh.self_intersect",
    comparison: Comparison::Le,
    unit: "faces",
    space: CROSSING,
    limit: |profile| profile.mesh.self_intersections,
};

pub const MIRROR: Rule = Rule {
    id: "mesh.mirror",
    comparison: Comparison::Le,
    unit: "percent",
    space: REFLECTED,
    limit: |profile| profile.mesh.mirror_percent,
};

pub const WORLD_SIZE: Rule = Rule {
    id: "mesh.world_size",
    comparison: Comparison::Le,
    unit: "percent",
    space: "world space, the vertex span along the up axis against spec.subject.height_meters",
    limit: |profile| profile.height_tolerance_percent,
};

pub const FACING: Rule = Rule {
    id: "mesh.facing",
    comparison: Comparison::Eq,
    unit: "axes",
    space: FEET,
    limit: |_| 0.0,
};

pub const STRAY_OBJECT: Rule = Rule {
    id: "mesh.stray_object",
    comparison: Comparison::Eq,
    unit: "objects",
    space: GRAPH,
    limit: |_| 0.0,
};

pub const BUDGET: Rule = Rule {
    id: "mesh.budget",
    comparison: Comparison::Le,
    unit: "triangles",
    space: WELDED,
    limit: |profile| profile.mesh.triangles,
};

pub const UV: Rule = Rule {
    id: "mesh.uv",
    comparison: Comparison::Eq,
    unit: "coordinates",
    space: DELIVERED,
    limit: |_| 0.0,
};

pub const TEXTURE: Rule = Rule {
    id: "mesh.texture",
    comparison: Comparison::Eq,
    unit: "primitives",
    space: MATERIALS,
    limit: |_| 0.0,
};

/// glTF has no quad primitive mode, so the spec's `quads: true` cannot be
/// read back from a delivered GLB. What can be read is whether a primitive
/// is triangles at all, and that matters: a primitive of points or lines is
/// a primitive every rule above silently did not measure.
pub const QUADS: Rule = Rule {
    id: "mesh.quads",
    comparison: Comparison::Eq,
    unit: "primitives",
    space: MODES,
    limit: |_| 0.0,
};

pub const PRINTABILITY: Rule = Rule {
    id: "mesh.printability",
    comparison: Comparison::Le,
    unit: "edges",
    space: REMOTE,
    limit: |profile| profile.mesh.printability_edges,
};

/// What filling a hole leaves behind: closing it lays a face that can meet
/// two others, so the count after the fixer has a ceiling of its own rather
/// than the one [`NON_MANIFOLD`] is read against.
pub const NON_MANIFOLD_POST: Rule = Rule {
    id: "mesh.non_manifold_post",
    comparison: Comparison::Le,
    unit: "edges",
    space: CLEANED,
    limit: |profile| profile.mesh.non_manifold_post,
};

/// Whether the fixer did anything: the defects it left, over the defects it
/// was given. **Strictly less than**, because a stub that wrote its input
/// back reads the same number it was given and `le` would pass it.
///
/// Holes and pieces only, summed. The fixer is measured to make the other
/// two classes worse and each has a ceiling of its own, and one reading per
/// class would refuse a mesh that arrived clean in one. Corrections 1 and 2
/// of T10 in the design have the numbers.
///
/// The limit is the subject's own count, so this rule publishes none:
/// [`Rule::against`] builds its findings, and [`Rule::measured`] would file
/// the `NaN` below, which a report refuses.
pub const CLEANUP_EFFECTIVE: Rule = Rule {
    id: "mesh.cleanup_effective",
    comparison: Comparison::Lt,
    unit: "defects",
    space: AGAINST_THE_BARE_MESH,
    limit: |_| f64::NAN,
};

/// Every mesh rule, in the order `--list-rules` prints them.
pub const RULES: [&Rule; 15] = [
    &HOLES,
    &NON_MANIFOLD,
    &ISLANDS,
    &SELF_INTERSECT,
    &MIRROR,
    &WORLD_SIZE,
    &FACING,
    &STRAY_OBJECT,
    &BUDGET,
    &UV,
    &TEXTURE,
    &QUADS,
    &PRINTABILITY,
    &NON_MANIFOLD_POST,
    &CLEANUP_EFFECTIVE,
];

/// The rules [`check`] reads off one file, which is every one the fixer does
/// not leave behind. [`CLEANED_RULES`] is which of them a cleaned file is
/// read against.
pub const FILE_RULES: [&Rule; 13] = [
    &HOLES,
    &NON_MANIFOLD,
    &ISLANDS,
    &SELF_INTERSECT,
    &MIRROR,
    &WORLD_SIZE,
    &FACING,
    &STRAY_OBJECT,
    &BUDGET,
    &UV,
    &TEXTURE,
    &QUADS,
    &PRINTABILITY,
];

/// The same rules on the file the fixer wrote, less the two a cleaned mesh
/// cannot honestly be read against.
///
/// [`NON_MANIFOLD`]'s ceiling is calibrated on the mesh as it arrived, and
/// filling a hole raises that count on purpose: after the fixer it belongs
/// to [`NON_MANIFOLD_POST`], and one count against two limits is one of them
/// wrong. [`PRINTABILITY`] asks about a model task, and a file the fixer
/// wrote has none, so it could only ever warn that nobody answered.
pub const CLEANED_RULES: [&Rule; 11] = [
    &HOLES,
    &ISLANDS,
    &SELF_INTERSECT,
    &MIRROR,
    &WORLD_SIZE,
    &FACING,
    &STRAY_OBJECT,
    &BUDGET,
    &UV,
    &TEXTURE,
    &QUADS,
];

/// And the two [`check_cleanup`] reads off the pair the fixer produced.
pub const CLEANUP_RULES: [&Rule; 2] = [&NON_MANIFOLD_POST, &CLEANUP_EFFECTIVE];

/// One file's reading, cut down to the rules a report owns.
///
/// [`check_file`] measures a whole file in one pass, so what does not apply
/// is dropped here rather than left out of the pass.
pub fn only(rules: &[&Rule], findings: Vec<Finding>) -> Vec<Finding> {
    findings
        .into_iter()
        .filter(|finding| rules.iter().any(|rule| rule.id == finding.rule))
        .collect()
}

/// Why `mesh.printability` has nothing to read, in the one case that is not
/// a network error: nobody has asked yet.
pub const NO_RESPONSE: &str = "no response is on record, and check makes no remote call";

/// Runs every mesh rule on one file.
///
/// `printability` is the recorded `print/analyze` response, and `None` means
/// the remote call was not made. Either way the rule reports, because an
/// unavailable check that says nothing cannot be told from one that passed.
///
/// A missing or unreadable file is one error finding, never a skip: a gate
/// that goes quiet on absent input proves nothing.
pub fn check_file(
    file: &Path,
    repo_root: &Path,
    profile: &Profile,
    height_meters: f64,
    symmetry: Symmetry,
    printability: Option<&str>,
    attempt: u32,
) -> Result<Vec<Finding>> {
    let subject = relative_to(file, repo_root);
    match Surface::read(file) {
        Ok(surface) => Ok(check(
            &subject,
            &surface,
            profile,
            height_meters,
            symmetry,
            printability,
            attempt,
        )),
        // Under `holes`, because welding before counting an edge is what
        // this whole representation exists for, and an unreadable file has
        // no edges at all. The remote rule still reports: it measures what
        // Meshy saw, not what this file system holds.
        Err(error) => Ok(vec![
            HOLES.undefined(
                &subject,
                attempt,
                format!("{subject} holds no readable surface: {error:#}"),
            ),
            remote(&subject, profile, printability, attempt),
        ]),
    }
}

/// Runs every mesh rule. Pure: the file is already read and welded.
pub fn check(
    file: &str,
    surface: &Surface,
    profile: &Profile,
    height_meters: f64,
    symmetry: Symmetry,
    printability: Option<&str>,
    attempt: u32,
) -> Vec<Finding> {
    let mesh = Measured {
        file,
        surface,
        profile,
        height_meters,
        symmetry,
        attempt,
    };
    [
        mesh.topology(),
        mesh.self_intersect(),
        mesh.mirror(),
        mesh.world_size(),
        mesh.facing(),
        mesh.budget(),
        mesh.per_object(),
        vec![remote(file, profile, printability, attempt)],
    ]
    .concat()
}

/// The one rule measured somewhere else. A response that cannot be parsed is
/// an answer we do not have, so it lands the same way an absent one does.
fn remote(file: &str, profile: &Profile, printability: Option<&str>, attempt: u32) -> Finding {
    match printability.map(|response| self::printability(response, file, profile, attempt)) {
        Some(Ok(finding)) => finding,
        Some(Err(error)) => printability_unavailable(file, attempt, &format!("{error:#}")),
        None => printability_unavailable(file, attempt, NO_RESPONSE),
    }
}

/// What the fixer left behind, or why there is nothing to read.
///
/// Named rather than a pair of arguments, because the whole point of
/// [`CLEANUP_EFFECTIVE`] is which of the two surfaces is read against which.
pub enum Fixed<'a> {
    /// The mesh the fixer was given, and the mesh it wrote.
    Cleaned {
        before: &'a Surface,
        after: &'a Surface,
    },
    /// `spec.subject.cleanup` is false, so no fixer ran.
    Declined,
    /// One of the two files holds no readable surface, and why.
    Unreadable(String),
}

/// The two counts [`CLEANUP_EFFECTIVE`] adds up, in the order [`defects`]
/// returns them. Named in the message, so a reader sees which class moved.
const CLASSES: [&str; 2] = ["holes", "islands"];

/// The two post-cleanup rules, read off the files themselves.
///
/// `after` is `None` when `spec.subject.cleanup` is false, which is the one
/// reason nothing was written. An unreadable file is an error finding and
/// never a skip, the way it is in [`check_file`].
pub fn check_cleanup_files(
    before: &Path,
    after: Option<&Path>,
    repo_root: &Path,
    profile: &Profile,
    attempt: u32,
) -> Vec<Finding> {
    let subject = relative_to(after.unwrap_or(before), repo_root);
    let Some(after) = after else {
        return check_cleanup(&subject, Fixed::Declined, profile, attempt);
    };
    let read = |file: &Path| {
        Surface::read(file).map_err(|error| {
            format!(
                "{} holds no readable surface: {error:#}",
                relative_to(file, repo_root)
            )
        })
    };
    match (read(before), read(after)) {
        (Ok(before), Ok(after)) => check_cleanup(
            &subject,
            Fixed::Cleaned {
                before: &before,
                after: &after,
            },
            profile,
            attempt,
        ),
        (Err(why), _) | (_, Err(why)) => {
            check_cleanup(&subject, Fixed::Unreadable(why), profile, attempt)
        }
    }
}

/// The two rules that read what the fixer wrote. Pure: both files are read
/// and welded already.
pub fn check_cleanup(
    subject: &str,
    fixed: Fixed<'_>,
    profile: &Profile,
    attempt: u32,
) -> Vec<Finding> {
    let (before, after) = match fixed {
        Fixed::Cleaned { before, after } => (before, after),
        Fixed::Declined => {
            return CLEANUP_RULES
                .into_iter()
                .map(|rule| rule.skipped(profile, subject, attempt, DECLINED.to_owned()))
                .collect();
        }
        Fixed::Unreadable(why) => return undefined_pair(subject, &why, attempt),
    };
    // Zero non-manifold edges on no triangles is the shape of a fixer that
    // emptied the file, so neither rule reports a number on one.
    for (surface, which) in [(after, "wrote"), (before, "was given")] {
        if surface.triangles().is_empty() {
            let why = format!(
                "the mesh the fixer {which} holds no triangle, so there is nothing \
                 to measure"
            );
            return undefined_pair(subject, &why, attempt);
        }
    }
    let non_manifold = |surface: &Surface| {
        gltf_mesh::edge_use(surface)
            .values()
            .filter(|used| **used >= 3)
            .count()
    };
    let (left, right) = (non_manifold(after), non_manifold(before));
    let mut findings = vec![NON_MANIFOLD_POST.measured(
        profile,
        subject,
        left as f64,
        attempt,
        format!(
            "{left} edges are shared by three or more faces once the fixer has run, \
             against {right} before it"
        ),
    )];
    let (left, right) = (defects(after), defects(before));
    let per_class: Vec<String> = CLASSES
        .into_iter()
        .zip(left.into_iter().zip(right))
        .map(|(class, (left, right))| format!("{class} {right} to {left}"))
        .collect();
    findings.push(CLEANUP_EFFECTIVE.against(
        subject,
        left.iter().sum::<usize>() as f64,
        right.iter().sum::<usize>() as f64,
        attempt,
        format!("the fixer took {}", per_class.join(", ")),
    ));
    findings
}

/// Both post-cleanup rules, on every subject they own, with one reason.
fn undefined_pair(subject: &str, why: &str, attempt: u32) -> Vec<Finding> {
    CLEANUP_RULES
        .into_iter()
        .map(|rule| rule.undefined(subject, attempt, why.to_owned()))
        .collect()
}

/// What [`CLEANUP_EFFECTIVE`] counts on one surface, in [`CLASSES`] order.
///
/// The two the fixer exists to remove. The two it makes worse have their own
/// ceilings, which is [`CLEANUP_EFFECTIVE`]'s own documentation.
fn defects(surface: &Surface) -> [usize; 2] {
    let boundary = gltf_mesh::edge_use(surface)
        .values()
        .filter(|used| **used == 1)
        .count();
    [boundary, gltf_mesh::islands(surface)]
}

/// Why nothing was measured, when the spec asked for no fixer at all.
const DECLINED: &str =
    "spec.subject.cleanup is false, so no fixer ran and nothing was written to measure";

/// One surface, one profile, one pass.
struct Measured<'a> {
    file: &'a str,
    surface: &'a Surface,
    profile: &'a Profile,
    height_meters: f64,
    symmetry: Symmetry,
    attempt: u32,
}

impl Measured<'_> {
    fn measured(&self, rule: &Rule, subject: &str, measured: f64, message: String) -> Finding {
        rule.measured(self.profile, subject, measured, self.attempt, message)
    }

    fn undefined(&self, rule: &Rule, subject: &str, message: String) -> Finding {
        rule.undefined(subject, self.attempt, message)
    }

    /// A whole-surface rule with no surface to read. Every one of them
    /// reports the reason rather than a zero, because zero holes on no
    /// triangles is the shape of a gate that passed by doing nothing.
    fn no_triangles(&self, rule: &Rule) -> Finding {
        self.undefined(
            rule,
            self.file,
            format!(
                "{} holds no triangle, so there is nothing to measure. \
                 mesh.quads names the primitives that were not readable",
                self.file
            ),
        )
    }

    /// Holes, non-manifold edges and islands, all three off one edge count.
    fn topology(&self) -> Vec<Finding> {
        if self.surface.triangles().is_empty() {
            return [&HOLES, &NON_MANIFOLD, &ISLANDS]
                .map(|rule| self.no_triangles(rule))
                .to_vec();
        }
        let uses = gltf_mesh::edge_use(self.surface);
        let boundary = uses.values().filter(|used| **used == 1).count();
        let non_manifold = uses.values().filter(|used| **used >= 3).count();
        let islands = gltf_mesh::islands(self.surface);
        let welded = self.surface.positions().len();
        vec![
            self.measured(
                &HOLES,
                self.file,
                boundary as f64,
                format!(
                    "{boundary} of {} edges are used by one face, on {welded} welded \
                     vertices out of {} the file stores",
                    uses.len(),
                    self.surface.raw_vertices()
                ),
            ),
            self.measured(
                &NON_MANIFOLD,
                self.file,
                non_manifold as f64,
                format!("{non_manifold} edges are shared by three or more faces"),
            ),
            self.measured(
                &ISLANDS,
                self.file,
                islands as f64,
                format!("the surface falls into {islands} connected pieces"),
            ),
        ]
    }

    /// Faces that cross another face of the same mesh.
    fn self_intersect(&self) -> Vec<Finding> {
        if self.surface.triangles().is_empty() {
            return vec![self.no_triangles(&SELF_INTERSECT)];
        }
        let (faces, pairs) = crossing_faces(self.surface);
        vec![self.measured(
            &SELF_INTERSECT,
            self.file,
            faces as f64,
            format!("{faces} faces meet another face they share no vertex with, in {pairs} pairs"),
        )]
    }

    /// How far the mesh is from its own reflection.
    fn mirror(&self) -> Vec<Finding> {
        if self.symmetry == Symmetry::Declined {
            return vec![MIRROR.skipped(
                self.profile,
                self.file,
                self.attempt,
                NOT_MIRRORED.to_owned(),
            )];
        }
        let Some(spread) = mirror_spread(self.surface) else {
            return vec![self.undefined(
                &MIRROR,
                self.file,
                format!("{} has no width, so it has nothing to reflect", self.file),
            )];
        };
        vec![self.measured(
            &MIRROR,
            self.file,
            spread.worst,
            format!(
                "the worst vertex sits {:.3} percent of the width from its reflection, \
                 with a mean of {:.3} and a 99th percentile of {:.3}",
                spread.worst, spread.mean, spread.p99
            ),
        )]
    }

    /// The mesh against the height the spec asks for.
    fn world_size(&self) -> Vec<Finding> {
        let up = blender_to_gltf(self.profile.up_axis.vector());
        let Some((low, high)) = span_along(self.surface.positions(), up) else {
            return vec![self.undefined(
                &WORLD_SIZE,
                self.file,
                format!("{} holds no vertex, so it has no height", self.file),
            )];
        };
        let span = high - low;
        vec![self.measured(
            &WORLD_SIZE,
            self.file,
            (span - self.height_meters).abs() / self.height_meters * 100.0,
            format!(
                "the mesh spans {span:.4} m against the spec's {:.4} m",
                self.height_meters
            ),
        )]
    }

    /// Which way the feet point, which is which way the character faces.
    fn facing(&self) -> Vec<Finding> {
        let declared = self.profile.facing_axis_gltf;
        let up = blender_to_gltf(self.profile.up_axis.vector());
        let Some(step) = foot_step(self.surface, up) else {
            return vec![self.undefined(
                &FACING,
                self.file,
                format!(
                    "the lowest {:.0} percent of {} sits directly under the whole body, \
                     so it faces nowhere",
                    FOOT_SLICE * 100.0,
                    self.file
                ),
            )];
        };
        let closest = Axis::closest_to(step);
        let apart = step
            .dot(declared.vector())
            .clamp(-1.0, 1.0)
            .acos()
            .to_degrees();
        vec![self.measured(
            &FACING,
            self.file,
            f64::from(closest != declared),
            format!(
                "the feet sit {closest} of the body's center in glTF space, {apart:.1} \
                 degrees from the declared {declared}"
            ),
        )]
    }

    fn budget(&self) -> Vec<Finding> {
        let triangles = self.surface.triangles().len();
        vec![self.measured(
            &BUDGET,
            self.file,
            triangles as f64,
            format!("the file holds {triangles} triangles"),
        )]
    }

    /// The four rules that belong to one mesh object rather than to the
    /// whole surface, so a finding names the object a reviewer opens.
    ///
    /// Each is a defect count against a limit of zero, with what to say when
    /// there are none and what to say when there are some.
    fn per_object(&self) -> Vec<Finding> {
        let allowed: BTreeSet<&str> = self.profile.meshes.iter().map(String::as_str).collect();
        self.surface
            .objects()
            .iter()
            .flat_map(|object| {
                let name = &object.name;
                let (untextured, off_tile) =
                    (object.untextured_primitives, object.uvs_outside_the_tile);
                let unreadable = object.unreadable_primitives;
                [
                    (
                        &STRAY_OBJECT,
                        usize::from(!allowed.contains(name.as_str())),
                        format!("{name} is a mesh the profile allows"),
                        format!(
                            "{name} is not in the profile's meshes, so it is debris the \
                             rigger would be paid to skin"
                        ),
                    ),
                    (
                        &TEXTURE,
                        untextured,
                        format!("every primitive of {name} has a base color image"),
                        format!(
                            "{untextured} primitives of {name} have no base color image, \
                             and rigging refuses an untextured mesh"
                        ),
                    ),
                    (
                        &UV,
                        off_tile,
                        format!("every texture coordinate of {name} is in [0,1]"),
                        format!(
                            "{off_tile} texture coordinates of {name} leave the [0,1] \
                             tile, where there is no texture to sample"
                        ),
                    ),
                    (
                        &QUADS,
                        unreadable,
                        // glTF cannot express a quad, so `spec.remesh.quads` is
                        // unverifiable here and this is all the rule can say.
                        format!(
                            "every one of the {} primitives of {name} is triangles, which \
                             is the only surface glTF can express",
                            object.primitives
                        ),
                        format!(
                            "{unreadable} primitives of {name} are points, lines or \
                             strips, so every rule above measured less than the whole mesh"
                        ),
                    ),
                ]
                .map(|(rule, defects, clean, broken)| {
                    self.measured(
                        rule,
                        name,
                        defects as f64,
                        if defects == 0 { clean } else { broken },
                    )
                })
            })
            .collect()
    }
}

/// The lowest and highest projection of a point set on one direction.
fn span_along(points: &[DVec3], direction: DVec3) -> Option<(f64, f64)> {
    let along = points.iter().map(|point| point.dot(direction));
    let low = along.clone().reduce(f64::min)?;
    Some((low, along.reduce(f64::max)?))
}

/// The horizontal direction from the whole body's center to the center of
/// its lowest [`FOOT_SLICE`], normalized. `None` when the two sit on top of
/// each other, where there is no direction to name.
fn foot_step(surface: &Surface, up: DVec3) -> Option<DVec3> {
    let points = surface.positions();
    let (low, high) = span_along(points, up)?;
    let cut = low + (high - low) * FOOT_SLICE;
    let feet: Vec<DVec3> = points
        .iter()
        .copied()
        .filter(|point| point.dot(up) <= cut)
        .collect();
    let step = center(&feet)? - center(points)?;
    let flat = step - up * step.dot(up);
    (flat.length() > SHORTEST_SEGMENT_METERS).then(|| flat.normalize())
}

fn center(points: &[DVec3]) -> Option<DVec3> {
    (!points.is_empty()).then(|| points.iter().sum::<DVec3>() / points.len() as f64)
}

/// How far every vertex sits from its own reflection, as a percent of width.
struct MirrorSpread {
    mean: f64,
    p99: f64,
    worst: f64,
}

/// Reflects every vertex across X = 0 and measures how far the nearest real
/// vertex is, as a percent of the mesh's width.
///
/// `None` when the mesh has no width, where a percent of it is a precise
/// number about nothing.
fn mirror_spread(surface: &Surface) -> Option<MirrorSpread> {
    let points = surface.positions();
    let (low, high) = span_along(points, DVec3::X)?;
    let width = high - low;
    if width <= SHORTEST_SEGMENT_METERS {
        return None;
    }
    let tree = Bvh::from_leaves(
        BvhBuildStrategy::Binned,
        &points
            .iter()
            .map(|point| {
                let at = as_vector(*point);
                Aabb::new(at, at)
            })
            .collect::<Vec<Aabb>>(),
    );
    // No vertex can be further from its reflection than the whole bounding
    // box is across, so this bound never excludes the real answer.
    let reach = tree.root_aabb().extents().length() * 2.0;
    let mut apart: Vec<f64> = points
        .iter()
        .map(|point| {
            let reflected = as_vector(DVec3::new(-point.x, point.y, point.z));
            let nearest = tree.find_best(
                reach,
                |node, _| {
                    let around = node.aabb();
                    (reflected.clamp(around.mins, around.maxs) - reflected).length()
                },
                |leaf, _| Some((as_vector(points[leaf as usize]) - reflected).length()),
            );
            // `find_best` returns nothing only for an empty tree, and the
            // width check above already refused that.
            f64::from(nearest.map_or(reach, |(_, apart)| apart)) / width * 100.0
        })
        .collect();
    apart.sort_by(f64::total_cmp);
    Some(MirrorSpread {
        mean: apart.iter().sum::<f64>() / apart.len() as f64,
        p99: apart[apart.len() * 99 / 100],
        worst: *apart.last()?,
    })
}

/// Faces that meet another face of the same mesh, and how many pairs do.
///
/// A pair that shares a vertex or an edge does not count: after welding
/// every neighbor shares its indices, so the filter is exact rather than a
/// tolerance. What is left is two different parts of one surface in contact,
/// which is the fused geometry that makes weight painting unable to tell
/// which bone a vertex belongs to.
///
/// **The test is intersection and not penetration depth.** Two triangles
/// have no thickness, so they cross in a line of zero volume and EPA reports
/// a depth of 0. A depth threshold therefore drops real crossings: two
/// visibly interpenetrating boxes read zero faces that way.
fn crossing_faces(surface: &Surface) -> (usize, usize) {
    let points = surface.positions();
    let corners = |face: &[u32; 3]| face.map(|corner| as_vector(points[corner as usize]));
    let boxes: Vec<Aabb> = surface
        .triangles()
        .iter()
        .map(|face| Aabb::from_points(corners(face).iter().copied()))
        .collect();
    let tree = Bvh::from_leaves(BvhBuildStrategy::Binned, &boxes);
    let at_rest = Pose::IDENTITY;
    let mut faces = BTreeSet::new();
    let mut pairs = 0;
    for (index, face) in surface.triangles().iter().enumerate() {
        let [a, b, c] = corners(face);
        let mine = Triangle::new(a, b, c);
        for other in tree.leaves(|node| node.aabb().intersects(&boxes[index])) {
            let other = other as usize;
            if other <= index {
                continue; // Each pair once, and never a face against itself.
            }
            let theirs = &surface.triangles()[other];
            if face.iter().any(|corner| theirs.contains(corner)) {
                continue;
            }
            let [a, b, c] = corners(theirs);
            let meets = intersection_test(&at_rest, &mine, &at_rest, &Triangle::new(a, b, c))
                .unwrap_or(false);
            if meets {
                faces.insert(index);
                faces.insert(other);
                pairs += 1;
            }
        }
    }
    (faces.len(), pairs)
}

/// `parry3d` measures in `f32`. A 1.7 m character keeps about a tenth of a
/// micron of resolution there, which is two orders under the smallest weld
/// distance and four under the thinnest real feature.
fn as_vector(point: DVec3) -> Vector {
    point.as_vec3()
}

/// What Meshy's `print/analyze` returns, of the fields a gate reads.
///
/// Its `non_manifold_edges` counts boundary and true non-manifold edges
/// together, which is why it reads 179 on the survivor where our own pass
/// reads 171 plus 8. `is_watertight` is Meshy's own summary of that same
/// count, so it goes in the message rather than being measured twice.
#[derive(Debug, Deserialize)]
pub struct Printability {
    pub non_manifold_edges: u64,
    pub is_watertight: bool,
}

/// Maps a `print/analyze` response into a finding. Pure, so the mapping is
/// tested without a network.
pub fn printability(
    response: &str,
    subject: &str,
    profile: &Profile,
    attempt: u32,
) -> Result<Finding> {
    let report: Printability =
        serde_json::from_str(response).context("parsing the print/analyze response")?;
    Ok(PRINTABILITY.measured(
        profile,
        subject,
        report.non_manifold_edges as f64,
        attempt,
        format!(
            "Meshy counts {} boundary and non-manifold edges together, and calls the mesh {}",
            report.non_manifold_edges,
            if report.is_watertight {
                "watertight"
            } else {
                "not watertight"
            }
        ),
    ))
}

/// The remote call could not be made. A warning and never silence: an
/// unavailable check that reports nothing is indistinguishable from one that
/// passed.
///
/// This is also the design's one stated exemption to re-running a gate after
/// a fixer: `print/analyze` needs a task or a URL, and `clean.glb` is local.
pub fn printability_unavailable(subject: &str, attempt: u32, why: &str) -> Finding {
    Finding {
        rule: PRINTABILITY.id.to_owned(),
        severity: Severity::Warning,
        subject: subject.to_owned(),
        measured: 0.0,
        limit: 0.0,
        comparison: Comparison::Le,
        unit: "unavailable calls".to_owned(),
        attempt,
        measured_on: PRINTABILITY.space.to_owned(),
        message: format!("Meshy print/analyze did not answer for {subject}: {why}"),
    }
}
