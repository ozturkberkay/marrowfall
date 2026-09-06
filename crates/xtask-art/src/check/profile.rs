//! The skeleton profile: every published rig limit, read from
//! `art/skeletons/<skeleton>.toml`.
//!
//! Data, not code, so a second skeleton is a second file that the same rules
//! read. That file also carries the role tables, which Blender reads, so
//! anything outside `[profile]` is another reader's business and is ignored
//! here.
//!
//! The profile is validated on the way in, not only parsed. A rule whose
//! limit or hierarchy is nonsense reports precise, wrong numbers, and precise
//! wrong numbers are the failure this design exists to stop.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail, ensure};
use glam::DVec3;
use serde::Deserialize;

use super::gltf_world::gltf_to_blender;

/// The two sides of a mirrored skeleton. The mirror rules pair bones by these
/// prefixes.
pub const LEFT: &str = "Left";
pub const RIGHT: &str = "Right";

/// The same bone on the other side, for a bone that has one.
pub fn mirrored(bone: &str) -> Option<String> {
    match bone.strip_prefix(LEFT) {
        Some(stem) => Some(format!("{RIGHT}{stem}")),
        None => bone.strip_prefix(RIGHT).map(|stem| format!("{LEFT}{stem}")),
    }
}

/// One signed axis, in whatever space the field that holds it names.
///
/// `up_axis = "z"` and `facing_axis_gltf = "+z"` both parse here: an unsigned
/// letter means the positive direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Axis {
    letter: char,
    negative: bool,
}

impl Axis {
    /// The six signed axes, so a direction can be named by the one it points
    /// closest to.
    pub const ALL: [Axis; 6] = [
        Axis {
            letter: 'x',
            negative: false,
        },
        Axis {
            letter: 'x',
            negative: true,
        },
        Axis {
            letter: 'y',
            negative: false,
        },
        Axis {
            letter: 'y',
            negative: true,
        },
        Axis {
            letter: 'z',
            negative: false,
        },
        Axis {
            letter: 'z',
            negative: true,
        },
    ];

    pub fn parse(text: &str) -> Result<Self> {
        let (negative, letter) = match text.as_bytes() {
            [b'+', letter] => (false, *letter),
            [b'-', letter] => (true, *letter),
            [letter] => (false, *letter),
            _ => bail!("an axis is x, y or z with an optional sign, got {text:?}"),
        };
        ensure!(
            matches!(letter, b'x' | b'y' | b'z'),
            "an axis is x, y or z with an optional sign, got {text:?}"
        );
        Ok(Self {
            letter: char::from(letter),
            negative,
        })
    }

    /// The unit vector, in the axis's own space.
    pub fn vector(self) -> DVec3 {
        let unit = match self.letter {
            'x' => DVec3::X,
            'y' => DVec3::Y,
            _ => DVec3::Z,
        };
        if self.negative { -unit } else { unit }
    }

    /// Whichever of the six signed axes a direction points closest to. A
    /// finding names this one when it is not the declared axis.
    pub fn closest_to(direction: DVec3) -> Self {
        Self::ALL
            .into_iter()
            .max_by(|a, b| {
                a.vector()
                    .dot(direction)
                    .total_cmp(&b.vector().dot(direction))
            })
            .expect("six axes")
    }
}

impl std::fmt::Display for Axis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}{}",
            if self.negative { '-' } else { '+' },
            self.letter
        )
    }
}

impl<'de> Deserialize<'de> for Axis {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::parse(&String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// The `[profile.mesh]` table: one ceiling per mesh rule.
///
/// Every value is a count or a percent, so they are stored as `f64` and read
/// straight through [`super::Rule`], which compares in `f64`. An integer
/// field would have to be widened at every use.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeshLimits {
    /// Boundary edges: an edge used by exactly one face.
    pub holes: f64,
    /// Edges shared by three or more faces.
    pub non_manifold_edges: f64,
    /// Connected pieces of the welded surface.
    pub islands: f64,
    /// Faces that cross another face of the same mesh.
    pub self_intersections: f64,
    /// How far a vertex may sit from its own reflection, as a percent of the
    /// mesh's width.
    pub mirror_percent: f64,
    /// The triangle budget the rigger states.
    pub triangles: f64,
    /// Meshy's own edge count, which adds boundary and true non-manifold
    /// edges together.
    pub printability_edges: f64,
}

/// The `[profile.clip]` table: what a fitted clip may differ from the file it
/// was fitted from.
///
/// Both are angles in degrees, read against a per bone worst over the whole
/// clip. Neither is a guess: the Test Plan carries what each was measured on.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClipLimits {
    /// How far the output bone's own axis may point from the source bone's.
    /// The transfer drives this to zero by construction, so the limit is a
    /// noise floor rather than a tolerance.
    pub swing_degrees: f64,
    /// How far the roll the clip carries may sit from the roll the two bind
    /// poses call for.
    pub twist_degrees: f64,
}

/// A target angle and how far from it is still acceptable.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Band {
    pub target: f64,
    pub tolerance: f64,
}

/// The whole `[profile]` table.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// Every bone this skeleton has, under its standard name. An allowlist
    /// for `rig.names_standard`, and a required set for `rig.bone_set`.
    pub bones: Vec<String>,
    /// The bone every other bone descends from.
    pub single_root: String,
    /// Mesh objects a character file may hold. Read by the mesh gates, which
    /// run on the character and not on the skeleton.
    pub meshes: Vec<String>,
    /// Which way is up, in Blender, after the glTF Y-up import.
    pub up_axis: Axis,
    /// Which way the character faces, in glTF Y-up.
    pub facing_axis_gltf: Axis,
    /// Which bone axis points from a joint to its child.
    pub child_axis: Axis,
    pub child_axis_tolerance_degrees: f64,
    pub height_tolerance_percent: f64,
    pub mirror_tolerance_percent: f64,
    pub mirror_tolerance_degrees: f64,
    pub max_bind_deviation_degrees: f64,
    pub humerus_below_horizontal: Band,
    /// Every published mesh limit. The mesh gates run on the character, not
    /// on the skeleton, and they read these.
    pub mesh: MeshLimits,
    /// Every published clip limit, read by the retarget's own gates.
    pub clip: ClipLimits,
    /// Bone to its parent. Every bone but the root has a row.
    pub parents: BTreeMap<String, String>,
    /// Bone to the child its `child_axis` must point at.
    pub tails: BTreeMap<String, String>,
}

/// Only the `[profile]` table. The rest of the file belongs to other readers.
#[derive(Deserialize)]
struct SkeletonFile {
    profile: Profile,
}

impl Profile {
    /// Where every skeleton is declared.
    pub fn dir(repo_root: &Path) -> PathBuf {
        repo_root.join("art/skeletons")
    }

    /// Where one skeleton's profile lives.
    pub fn path(repo_root: &Path, skeleton: &str) -> PathBuf {
        Self::dir(repo_root).join(format!("{skeleton}.toml"))
    }

    /// Every skeleton this repository declares, by name, sorted. One file per
    /// skeleton, so this is the whole list.
    pub fn declared(repo_root: &Path) -> Result<Vec<String>> {
        let dir = Self::dir(repo_root);
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut found: Vec<String> = std::fs::read_dir(&dir)
            .with_context(|| format!("reading {}", dir.display()))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|kind| kind == "toml"))
            .filter_map(|path| path.file_stem()?.to_str().map(str::to_owned))
            .collect();
        found.sort();
        Ok(found)
    }

    /// The profile of one skeleton, named the way a spec names it.
    pub fn of(repo_root: &Path, skeleton: &str) -> Result<Self> {
        Self::read(&Self::path(repo_root, skeleton))
            .with_context(|| format!("the {skeleton} skeleton profile"))
    }

    pub fn read(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading the skeleton profile {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("in {}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Self> {
        let file: SkeletonFile = toml::from_str(text).context("parsing [profile]")?;
        file.profile.validate()?;
        Ok(file.profile)
    }

    /// Whether the profile declares this bone.
    pub fn declares(&self, bone: &str) -> bool {
        self.bones.iter().any(|known| known == bone)
    }

    /// The bone whose direction `child_axis` must point at, for a bone that
    /// has one. A leaf has none.
    pub fn tail(&self, bone: &str) -> Option<&str> {
        self.tails.get(bone).map(String::as_str)
    }

    /// The tail chain that holds the body upright, from the root to the end:
    /// `Hips`, `Spine`, ... `head_end`. `rig.bind_deviation` measures every
    /// step of it and `rig.up_axis` measures it end to end.
    pub fn root_chain(&self) -> Vec<&str> {
        let mut chain = vec![self.single_root.as_str()];
        // One step per bone at most. `validate` has already refused a cycle,
        // and this does not depend on that being true two functions away.
        for _ in 0..self.bones.len() {
            match self.tail(chain[chain.len() - 1]) {
                Some(tail) => chain.push(tail),
                None => break,
            }
        }
        chain
    }

    /// Every mirrored segment: the left tail edge beside the right one.
    /// Derived from the tails, so a skeleton with no limbs yields none.
    pub fn mirror_pairs(&self) -> Vec<MirrorPair> {
        self.tails
            .iter()
            .filter_map(|(bone, tail)| {
                let segment = bone.strip_prefix(LEFT)?;
                let right = mirrored(bone)?;
                let right_tail = mirrored(tail)?;
                Some(MirrorPair {
                    segment: segment.to_owned(),
                    left: (bone.clone(), tail.clone()),
                    right: (right, right_tail),
                })
            })
            .collect()
    }

    fn validate(&self) -> Result<()> {
        distinct("bones", &self.bones)?;
        distinct("meshes", &self.meshes)?;
        ensure!(
            self.declares(&self.single_root),
            "single_root {:?} is not in bones",
            self.single_root
        );
        for (table, rows) in [("parents", &self.parents), ("tails", &self.tails)] {
            for (bone, other) in rows {
                ensure!(
                    self.declares(bone),
                    "{table} names {bone:?}, which is not in bones"
                );
                ensure!(
                    self.declares(other),
                    "{table}.{bone} names {other:?}, which is not in bones"
                );
            }
        }
        // Every bone but the root needs a parent, or no rule can say where it
        // belongs, and the root must have none, or the hierarchy has no top.
        for bone in &self.bones {
            let parent = self.parents.get(bone);
            if bone == &self.single_root {
                ensure!(parent.is_none(), "the root {bone:?} must have no parent");
            } else {
                ensure!(parent.is_some(), "{bone:?} has no parent");
            }
        }
        for bone in self.parents.keys() {
            self.ends_at_the_root(bone)?;
        }
        // A tail must be a real child, or `rig.child_axis` would measure
        // toward a bone that sits somewhere else entirely.
        for (bone, tail) in &self.tails {
            ensure!(
                self.parents.get(tail) == Some(bone),
                "tails.{bone} names {tail:?}, which is not its child"
            );
        }
        // And every bone that has a child needs a tail, or deleting one row
        // would quietly drop that bone from `rig.child_axis` and from
        // nothing else. A leaf has no child and needs none.
        for parent in self.parents.values() {
            ensure!(
                self.tails.contains_key(parent),
                "{parent} has children but no tails row, so no rule would \
                 measure which way it points"
            );
        }
        for bone in &self.bones {
            if let Some(other) = mirrored(bone) {
                ensure!(
                    self.declares(&other),
                    "{bone} has no mirror {other:?}, so the mirror rules would \
                     skip it"
                );
            }
        }
        for (field, value) in [
            (
                "child_axis_tolerance_degrees",
                self.child_axis_tolerance_degrees,
            ),
            ("height_tolerance_percent", self.height_tolerance_percent),
            ("mirror_tolerance_percent", self.mirror_tolerance_percent),
            ("mirror_tolerance_degrees", self.mirror_tolerance_degrees),
            (
                "max_bind_deviation_degrees",
                self.max_bind_deviation_degrees,
            ),
            (
                "humerus_below_horizontal.target",
                self.humerus_below_horizontal.target,
            ),
            (
                "humerus_below_horizontal.tolerance",
                self.humerus_below_horizontal.tolerance,
            ),
            // Every mesh ceiling. Zero islands or zero triangles describes
            // no mesh that can exist, so none of these may be zero either.
            ("mesh.holes", self.mesh.holes),
            ("mesh.non_manifold_edges", self.mesh.non_manifold_edges),
            ("mesh.islands", self.mesh.islands),
            ("mesh.self_intersections", self.mesh.self_intersections),
            ("mesh.mirror_percent", self.mesh.mirror_percent),
            ("mesh.triangles", self.mesh.triangles),
            ("mesh.printability_edges", self.mesh.printability_edges),
            // A clip limit of zero would fail every correct clip: both are
            // calibrated noise floors and neither is ever exactly reached.
            ("clip.swing_degrees", self.clip.swing_degrees),
            ("clip.twist_degrees", self.clip.twist_degrees),
        ] {
            ensure!(
                value.is_finite() && value > 0.0,
                "{field} must be a positive number, got {value}"
            );
        }
        // The two axis fields are stated in different spaces, so they are
        // compared in one: a character that faces the way it stands up leaves
        // `rig.facing` nothing horizontal to measure.
        let facing = gltf_to_blender(self.facing_axis_gltf.vector());
        ensure!(
            facing.dot(self.up_axis.vector()).abs() < 0.5,
            "facing_axis_gltf {} is the up axis {} in Blender space, so the \
             facing has no horizontal direction",
            self.facing_axis_gltf,
            self.up_axis
        );
        Ok(())
    }

    /// Follows the parents up, refusing a cycle. Nothing else can go wrong
    /// here: every parent names a declared bone, and exactly one bone has no
    /// parent row, so a walk that ends, ends at the root.
    fn ends_at_the_root(&self, bone: &str) -> Result<()> {
        let mut at = bone;
        for _ in 0..self.bones.len() {
            match self.parents.get(at) {
                Some(parent) => at = parent,
                None => return Ok(()),
            }
        }
        bail!("the parents of {bone:?} form a cycle")
    }
}

/// A list of names that must hold something, and nothing twice. An empty
/// list would leave the rule that reads it with no subjects at all.
fn distinct(field: &str, names: &[String]) -> Result<()> {
    ensure!(!names.is_empty(), "a profile needs {field}");
    let mut seen = BTreeSet::new();
    for name in names {
        ensure!(!name.trim().is_empty(), "a {field} name must not be empty");
        ensure!(seen.insert(name), "{field} names {name:?} twice");
    }
    Ok(())
}

/// One segment beside its reflection: `LeftArm` to `LeftForeArm` against
/// `RightArm` to `RightForeArm`.
#[derive(Debug, Clone)]
pub struct MirrorPair {
    /// What both sides share, such as `Arm`.
    pub segment: String,
    /// The bone and the tail it points at.
    pub left: (String, String),
    pub right: (String, String),
}
