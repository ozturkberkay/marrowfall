//! The aim table: one absolute world direction per role, plus the rule that
//! reads a rig's rest pose against it.
//!
//! The table is the single source of every bone's constant offset in the
//! retarget, so a sign error on one row produces a confidently wrong clip.
//! It is validated on the way in rather than only parsed: every role has a
//! row, no row names a role nothing maps, and a left row is the exact
//! reflection of its right one.
//!
//! `rig.aim_table` is the validation that needs a rig. Each aim must sit
//! inside `max_bind_deviation_degrees` of the rest aim the rig itself
//! carries, so a table that describes neither rig cannot pass. One rule, run
//! once per rig: ours at the rig stage, a source rig when its motion
//! arrives, because the offset is only as good as the reference pose both of
//! them reach.
//!
//! Directions are Blender Z-up world space, which is the space the table
//! states, and they come from each joint's own axis through the whole glTF
//! node chain. Nothing here reads a bone tail.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context as _, Result, bail, ensure};
use glam::DVec3;
use serde::Deserialize;

use super::gltf_world::{Skeleton, degrees_between, gltf_to_blender};
use super::profile::Profile;
use super::{Comparison, Finding, Rule, relative_to};

/// The two sides of a mirrored skeleton, as a role names them. The bone
/// names use [`super::profile::LEFT`] and [`super::profile::RIGHT`] instead.
const LEFT: &str = "left_";
const RIGHT: &str = "right_";

/// The same role on the other side, for a role that has one.
pub fn mirrored_role(role: &str) -> Option<String> {
    match role.strip_prefix(LEFT) {
        Some(stem) => Some(format!("{RIGHT}{stem}")),
        None => role.strip_prefix(RIGHT).map(|stem| format!("{LEFT}{stem}")),
    }
}

const REST: &str = "Blender Z-up world space, each bone's own child axis at rest against the aim \
                    the table asks for";

pub const AIM_TABLE: Rule = Rule {
    id: "rig.aim_table",
    comparison: Comparison::Le,
    unit: "degrees",
    space: REST,
    limit: |profile| profile.max_bind_deviation_degrees,
};

/// Every rule this module owns, in the order `--list-rules` prints them.
pub const RULES: [&Rule; 1] = [&AIM_TABLE];

/// The role tables the retarget reads: which bone fills each role, where each
/// role's bone must point, which roles stand on the floor, and which two size
/// a step.
#[derive(Debug, Clone)]
pub struct AimTable {
    canonical: String,
    conventions: BTreeMap<String, BTreeMap<String, String>>,
    /// Convention to the bones a file must have for it to match.
    fingerprints: BTreeMap<String, Vec<String>>,
    /// Role to its aim, as a unit direction in Blender Z-up world space.
    aims: BTreeMap<String, DVec3>,
    ground: Vec<String>,
    stride: [String; 2],
}

/// Only what this reader owns. `[profile]` belongs to the rig gates and the
/// retargeting chain to the transfer, so neither is named here.
#[derive(Deserialize)]
struct SkeletonFile {
    canonical: String,
    conventions: BTreeMap<String, BTreeMap<String, String>>,
    fingerprints: BTreeMap<String, Vec<String>>,
    aim_table: BTreeMap<String, Vec<f64>>,
    ground_roles: Vec<String>,
    stride_segment: [String; 2],
}

impl AimTable {
    /// The table of one skeleton, named the way a spec names it. One file per
    /// skeleton, which is why the path is the profile's.
    pub fn of(repo_root: &Path, skeleton: &str) -> Result<Self> {
        Self::read(&Profile::path(repo_root, skeleton))
            .with_context(|| format!("the {skeleton} aim table"))
    }

    pub fn read(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading the skeleton file {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("in {}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Self> {
        let file: SkeletonFile =
            toml::from_str(text).context("parsing [aim_table] and the conventions")?;
        let roles = file
            .conventions
            .get(&file.canonical)
            .with_context(|| {
                format!(
                    "the canonical convention {:?} is not declared",
                    file.canonical
                )
            })?
            .keys()
            .cloned()
            .collect();
        Ok(Self {
            aims: aims(&roles, &file.aim_table)?,
            ground: ground(&roles, file.ground_roles)?,
            stride: stride(&roles, file.stride_segment)?,
            fingerprints: fingerprints(&file.conventions, file.fingerprints)?,
            canonical: file.canonical,
            conventions: file.conventions,
        })
    }

    /// The convention the canonical rig is itself named in.
    pub fn canonical(&self) -> &str {
        &self.canonical
    }

    /// Every role the table aims, sorted.
    pub fn roles(&self) -> impl Iterator<Item = &str> {
        self.aims.keys().map(String::as_str)
    }

    /// Where one role's bone must point, as a unit direction.
    pub fn aim(&self, role: &str) -> Option<DVec3> {
        self.aims.get(role).copied()
    }

    /// The roles that stand on the floor, which `clip.floor_snap` reads.
    /// Skeleton data, not a name: a quadruped has four of them.
    pub fn ground_roles(&self) -> &[String] {
        &self.ground
    }

    /// The two roles a clip's travel is sized by, which `clip.stride_ratio`
    /// measures on each rig. A femur: total height carries the head and the
    /// feet, and neither one takes a step.
    pub fn stride_segment(&self) -> &[String; 2] {
        &self.stride
    }

    /// One convention's role to bone name map.
    pub fn bones(&self, convention: &str) -> Result<&BTreeMap<String, String>> {
        self.conventions.get(convention).with_context(|| {
            let known: Vec<&str> = self.conventions.keys().map(String::as_str).collect();
            format!(
                "unknown bone naming convention {convention:?}, known: {}",
                known.join(", ")
            )
        })
    }

    /// Which convention a rig in hand is named in, from `[fingerprints]`.
    ///
    /// Every bone a convention fingerprints must be there, so a row of two is
    /// an "and". Naming nothing and naming two are both refused: a rename
    /// that guessed would rewrite a bone into the wrong role, and there is no
    /// gate downstream that could tell.
    pub fn convention_of<'a>(
        &'a self,
        bones: impl IntoIterator<Item = &'a str>,
    ) -> Result<&'a str> {
        let held: BTreeSet<String> = bones.into_iter().map(bare_bone_name).collect();
        let matched: Vec<&str> = self
            .fingerprints
            .iter()
            .filter(|(_, wanted)| {
                wanted
                    .iter()
                    .all(|bone| held.contains(&bare_bone_name(bone)))
            })
            .map(|(name, _)| name.as_str())
            .collect();
        match matched[..] {
            [only] => Ok(only),
            [] => bail!(
                "no convention fingerprints this rig, so nothing says which \
                 bone fills which role. It carries {}",
                held.iter().cloned().collect::<Vec<String>>().join(", ")
            ),
            _ => bail!(
                "{} all fingerprint this rig, so nothing tells them apart",
                matched.join(", ")
            ),
        }
    }
}

/// `mixamorig:LeftArm` becomes `leftarm`: no namespace, no case. The form two
/// rigs' bone names are compared in, and `skeleton.py::bare_bone_name` is the
/// other half of it.
pub fn bare_bone_name(bone: &str) -> String {
    bone.rsplit(':').next().unwrap_or(bone).to_lowercase()
}

/// Every convention's fingerprint, refused unless each one names at least one
/// bone and no bone fingerprints two conventions.
///
/// A shared bone tells the two apart from nothing, which is the same refusal
/// `skeleton.py` makes on the other side of this file.
fn fingerprints(
    conventions: &BTreeMap<String, BTreeMap<String, String>>,
    rows: BTreeMap<String, Vec<String>>,
) -> Result<BTreeMap<String, Vec<String>>> {
    let declared: BTreeSet<&str> = conventions.keys().map(String::as_str).collect();
    let covered: BTreeSet<&str> = rows.keys().map(String::as_str).collect();
    ensure!(
        declared == covered,
        "fingerprints covers {covered:?}, and every convention in {declared:?} needs one"
    );
    let mut owner: BTreeMap<String, &str> = BTreeMap::new();
    for (convention, bones) in &rows {
        ensure!(
            !bones.is_empty(),
            "convention {convention:?} has no fingerprint bone"
        );
        for bone in bones {
            if let Some(other) = owner.insert(bare_bone_name(bone), convention) {
                bail!(
                    "{bone:?} fingerprints {convention:?} and {other:?}, so it \
                     tells them apart from nothing"
                );
            }
        }
    }
    Ok(rows)
}

/// The roles that stand on the floor, refused unless every one is a role this
/// skeleton has and no two are the same joint.
///
/// A skeleton with none has no floor to sit on.
fn ground(roles: &BTreeSet<String>, named: Vec<String>) -> Result<Vec<String>> {
    ensure!(
        !named.is_empty(),
        "a skeleton with no ground role has no floor to sit on"
    );
    each_a_role(roles, &named, "ground_roles")?;
    Ok(named)
}

/// The two roles a step is measured across, refused unless both are roles
/// this skeleton has and they are two different joints.
fn stride(roles: &BTreeSet<String>, named: [String; 2]) -> Result<[String; 2]> {
    each_a_role(roles, &named, "stride_segment")?;
    Ok(named)
}

/// Every named role is a role of this skeleton, and none is named twice.
///
/// A repeat is refused because both readers take one number over the set: the
/// lowest of one joint and itself is that joint, and the distance from a
/// joint to itself is zero.
fn each_a_role(roles: &BTreeSet<String>, named: &[String], field: &str) -> Result<()> {
    let unknown: Vec<&String> = named.iter().filter(|role| !roles.contains(*role)).collect();
    ensure!(
        unknown.is_empty(),
        "{field} names {unknown:?}, which no convention maps"
    );
    for role in named {
        ensure!(
            named.iter().filter(|other| *other == role).count() == 1,
            "{field} names {role} twice"
        );
    }
    Ok(())
}

/// Every row read as a direction, refusing everything the table has to be
/// before a rig is ever measured against it.
///
/// A missing row is refused rather than filled from the source's own rest
/// pose, because a silent fallback is how the code this replaces left seven
/// bones uncorrected.
fn aims(
    roles: &BTreeSet<String>,
    rows: &BTreeMap<String, Vec<f64>>,
) -> Result<BTreeMap<String, DVec3>> {
    for role in roles {
        ensure!(
            rows.contains_key(role),
            "aim_table has no row for {role:?}, and every role needs one"
        );
    }
    let mut read = BTreeMap::new();
    for (role, row) in rows {
        ensure!(
            roles.contains(role),
            "aim_table names {role:?}, which no convention maps"
        );
        // A fourth number would otherwise be dropped without a word.
        let [x, y, z] = row[..] else {
            bail!(
                "aim_table.{role} holds {} numbers, and a direction is three",
                row.len()
            );
        };
        let direction = DVec3::new(x, y, z);
        ensure!(
            direction.is_finite() && direction.length() > 0.0,
            "aim_table.{role} is {row:?}, which is no direction at all"
        );
        read.insert(role.clone(), direction);
    }
    // A sign error on one row is invisible until a clip comes out wrong, so
    // the two sides are held to an exact reflection rather than a tolerance.
    for (role, aim) in &read {
        let Some(stem) = role.strip_prefix(LEFT) else {
            continue;
        };
        let other = format!("{RIGHT}{stem}");
        let Some(mirror) = read.get(&other) else {
            bail!("aim_table.{role} has no mirror row {other:?}");
        };
        ensure!(
            *aim == DVec3::new(-mirror.x, mirror.y, mirror.z),
            "aim_table.{role} is {aim} and {other} is {mirror}, which is not its \
             reflection across X = 0"
        );
    }
    // Normalized last, so the reflection above is compared on the rows as
    // written and the rows can stay whole numbers.
    Ok(read
        .into_iter()
        .map(|(role, aim)| (role, aim.normalize()))
        .collect())
}

/// Runs `rig.aim_table` on one file, in whichever convention it is named.
///
/// A missing or unreadable file is one error finding, never a skip: a gate
/// that goes quiet on absent input proves nothing.
pub fn check_file(
    file: &Path,
    repo_root: &Path,
    profile: &Profile,
    table: &AimTable,
    convention: &str,
    attempt: u32,
) -> Result<Vec<Finding>> {
    let subject = relative_to(file, repo_root);
    match Skeleton::read(file) {
        Ok(skeleton) => check(&skeleton, profile, table, convention, attempt),
        Err(error) => Ok(vec![AIM_TABLE.undefined(
            &subject,
            attempt,
            format!("{subject} holds no readable skeleton: {error:#}"),
        )]),
    }
}

/// One finding per role the rig fills. Pure: the file is already read.
///
/// A role the convention leaves out, or a bone the rig does not have, is
/// `rig.bone_set`'s business to report; there is no rest aim here to measure.
pub fn check(
    skeleton: &Skeleton,
    profile: &Profile,
    table: &AimTable,
    convention: &str,
    attempt: u32,
) -> Result<Vec<Finding>> {
    let axis = profile.child_axis;
    let bones = table.bones(convention)?;
    Ok(table
        .aims
        .iter()
        .filter_map(|(role, aim)| {
            let bone = bones.get(role)?;
            let joint = skeleton.get(bone)?;
            let Some(rest) = joint.axis(axis) else {
                return Some(AIM_TABLE.undefined(
                    role,
                    attempt,
                    format!("{bone} has no {axis} axis, its scale is zero"),
                ));
            };
            let off = degrees_between(gltf_to_blender(rest), *aim);
            Some(AIM_TABLE.measured(
                profile,
                role,
                off,
                attempt,
                format!(
                    "{bone} rests {off:.1} degrees from the {role} aim of \
                     [{:.3}, {:.3}, {:.3}]",
                    aim.x, aim.y, aim.z
                ),
            ))
        })
        .collect())
}
