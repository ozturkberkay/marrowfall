//! World-space joints, read out of the glTF node graph.
//!
//! Two traps live here, and both have already shipped broken art.
//!
//! - **The node chain is the transform.** This asset family carries a 0.01
//!   scale on the armature object, so a joint whose local translation reads
//!   96 units sits at 0.96 m. Reading a local value as a world value is wrong
//!   by 100x, which is how a hip at 2.316 m was once reported at 231.599 m.
//! - **A bone tail is not data.** glTF stores joints, not bones. Blender's
//!   importer invents a tail for every joint and gets its length 100x wrong,
//!   so every direction here comes from two joint positions.
//!
//! Everything is composed in `f64`. The file stores `f32`, and the angle
//! between two nearly parallel directions loses most of its digits in
//! `acos`, which is exactly where the mirror limits sit.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context as _, Result, bail, ensure};
use glam::{DMat4, DVec3};

use super::profile::Axis;

/// The shortest segment that has a direction worth reporting.
///
/// A difference of two nearly equal positions loses its digits to
/// cancellation, and the file stores `f32`, so the noise floor on a 1.7 m
/// character is around a tenth of a micron. Normalizing anything smaller
/// turns that noise into a confident direction, which is the shape of defect
/// this whole module exists to catch. A bone's own axis needs no such guard:
/// a rotation times a small scale keeps its direction exactly.
pub const SHORTEST_SEGMENT_METERS: f64 = 1e-6;

/// glTF is Y-up and Blender is Z-up, and the importer rotates the asset on
/// the way in. A profile field that names Blender space is measured through
/// this.
pub fn gltf_to_blender(v: DVec3) -> DVec3 {
    DVec3::new(v.x, -v.z, v.y)
}

/// The same conversion the other way, for a rule whose profile field is
/// stated in glTF space while the up axis is stated in Blender space.
pub fn blender_to_gltf(v: DVec3) -> DVec3 {
    DVec3::new(v.x, v.z, -v.y)
}

/// One joint, with its full world transform.
#[derive(Debug, Clone)]
pub struct Joint {
    pub name: String,
    /// The nearest joint above this one, or `None` for the root of the
    /// skeleton. Nodes in between that are not joints are stepped over, so an
    /// armature object never reads as a parent bone.
    pub parent: Option<String>,
    /// The whole node chain composed, from the scene root down.
    pub world: DMat4,
}

impl Joint {
    /// Where the joint sits, in world space.
    pub fn position(&self) -> DVec3 {
        self.world.w_axis.truncate()
    }

    /// One of the joint's own axes, in world space. `None` when the joint
    /// carries a zero scale, where the axis has no direction at all:
    /// reporting that beats normalizing it into a NaN.
    pub fn axis(&self, axis: Axis) -> Option<DVec3> {
        (self.world * axis.vector().extend(0.0))
            .truncate()
            .try_normalize()
    }
}

/// One node of the file's one real scene, with its whole chain composed.
#[derive(Debug, Clone)]
pub struct WorldNode<'a> {
    pub node: gltf::Node<'a>,
    /// The node above this one, or `None` for a scene root.
    pub parent: Option<usize>,
    /// The whole node chain composed, from the scene root down.
    pub world: DMat4,
}

/// Every node of one glTF file, in parents-first order, each with its whole
/// node chain composed.
///
/// One owner for the transform, because both the rig gates and the mesh
/// gates read it and reading a local value as a world value is the 100x
/// mistake this module exists to stop.
///
/// Only one scene is real: a node outside it has no world transform, so
/// nothing outside it is returned. `scene` is optional in glTF, and a viewer
/// picks when it is absent, so the first scene is the fallback.
pub fn world_nodes<'a>(document: &'a gltf::Document) -> Result<Vec<WorldNode<'a>>> {
    let scene = document
        .default_scene()
        .or_else(|| document.scenes().next())
        .context("the file declares no scene")?;
    let mut found = Vec::new();
    let mut reached = BTreeSet::new();
    let roots: Vec<gltf::Node<'a>> = scene.nodes().collect();
    let mut queue: Vec<(gltf::Node<'a>, Option<usize>, DMat4)> = roots
        .into_iter()
        .rev()
        .map(|root| (root, None, DMat4::IDENTITY))
        .collect();
    while let Some((node, parent, above)) = queue.pop() {
        ensure!(
            reached.insert(node.index()),
            "node {} appears twice in the scene", // Two parents, two world transforms.
            node.index()
        );
        let world = above * local(&node);
        let index = node.index();
        for child in node.children().collect::<Vec<_>>().into_iter().rev() {
            queue.push((child, Some(index), world));
        }
        found.push(WorldNode {
            node,
            parent,
            world,
        });
    }
    Ok(found)
}

/// Every joint of one glTF file, in world space, plus what the object-level
/// rule needs.
#[derive(Debug, Clone)]
pub struct Skeleton {
    joints: Vec<Joint>,
    /// How many animation channels drive each node above the skeleton.
    object_channels: BTreeMap<String, usize>,
}

impl Skeleton {
    pub fn read(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        Self::from_slice(&bytes).with_context(|| format!("in {}", path.display()))
    }

    pub fn from_slice(bytes: &[u8]) -> Result<Self> {
        let gltf = gltf::Gltf::from_slice(bytes).context("parsing the glTF")?;
        let document = &gltf.document;
        let joints: BTreeSet<usize> = document
            .skins()
            .flat_map(|skin| skin.joints())
            .map(|joint| joint.index())
            .collect();
        ensure!(
            !joints.is_empty(),
            "the file declares no skin, so it holds no skeleton"
        );

        let scene = world_nodes(document)?;
        // The nearest joint above each node. Filled parents-first, so a
        // node's own answer is already there when its children ask.
        let mut nearest: BTreeMap<usize, String> = BTreeMap::new();
        let mut found = Vec::new();
        for entry in &scene {
            let index = entry.node.index();
            let above = entry
                .parent
                .and_then(|parent| nearest.get(&parent))
                .cloned();
            let name = node_name(&entry.node);
            if joints.contains(&index) {
                found.push(Joint {
                    name: name.clone(),
                    parent: above,
                    world: entry.world,
                });
                nearest.insert(index, name);
            } else if let Some(above) = above {
                nearest.insert(index, above);
            }
        }
        let reached: BTreeSet<usize> = scene.iter().map(|entry| entry.node.index()).collect();
        if let Some(missing) = joints.difference(&reached).next() {
            bail!("joint {missing} is not in the scene, so it has no world transform");
        }

        let mut object_channels = BTreeMap::new();
        for channel in document
            .animations()
            .flat_map(|animation| animation.channels())
        {
            let node = channel.target().node();
            if reached.contains(&node.index()) && !joints.contains(&node.index()) {
                *object_channels.entry(node_name(&node)).or_insert(0_usize) += 1;
            }
        }
        Ok(Self {
            joints: found,
            object_channels,
        })
    }

    pub fn joints(&self) -> &[Joint] {
        &self.joints
    }

    /// One joint by name, the first if a file names two the same. Which
    /// duplicate wins is `rig.bone_set`'s business to report, not this
    /// reader's to hide.
    pub fn get(&self, name: &str) -> Option<&Joint> {
        self.joints.iter().find(|joint| joint.name == name)
    }

    /// The world direction from one joint to another, and `None` when the
    /// two sit on top of each other, where there is no direction to report.
    /// See [`SHORTEST_SEGMENT_METERS`].
    pub fn direction(&self, from: &str, to: &str) -> Option<DVec3> {
        let (from, to) = (self.get(from)?, self.get(to)?);
        let step = to.position() - from.position();
        (step.length() > SHORTEST_SEGMENT_METERS).then(|| step.normalize())
    }

    /// How many animation channels drive each node above the skeleton. An
    /// object-level action is what makes `transform_apply` change the meaning
    /// of every location key, so the rig stage refuses one.
    pub fn object_channels(&self) -> &BTreeMap<String, usize> {
        &self.object_channels
    }
}

/// One node's own transform, widened to `f64`.
fn local(node: &gltf::Node<'_>) -> DMat4 {
    let columns = node.transform().matrix();
    DMat4::from_cols_array_2d(&columns.map(|column| column.map(f64::from)))
}

/// The angle between two unit directions, in degrees. Clamped, so numeric
/// drift past 1.0 cannot come back as a NaN.
pub fn degrees_between(a: DVec3, b: DVec3) -> f64 {
    debug_assert!(a.is_normalized() && b.is_normalized(), "unit directions");
    a.dot(b).clamp(-1.0, 1.0).acos().to_degrees()
}

/// A node with no name of its own is named by its index, so a finding can
/// still point at it.
pub fn node_name(node: &gltf::Node<'_>) -> String {
    node.name()
        .map(str::to_owned)
        .unwrap_or_else(|| format!("node {}", node.index()))
}
