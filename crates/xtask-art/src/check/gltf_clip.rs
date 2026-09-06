//! The delivered clip, read out of the glTF file the bake will load.
//!
//! `check/gltf_world.rs` owns the rest pose, which is the node graph with no
//! animation applied. This module owns the other half: the same graph
//! evaluated at every key time the file carries.
//!
//! Reading the file rather than trusting what the retarget says it wrote is
//! the whole point. Between `transfer.py` and this GLB sit `write_keys`, the
//! travel scale, the interpolation pass and the exporter, and none of them is
//! measured anywhere else.
//!
//! Two things are deliberate.
//!
//! - **Whole transforms are composed, not rotations.** A parent's non-uniform
//!   scale shears its children, so a chain of quaternion products is not the
//!   world rotation. The matrices are composed and the rotation is taken at
//!   the end, with the scale divided out.
//! - **The frame set is the file's own key times**, the union over every
//!   sampler that drives a joint. A dropped key is then a frame that is
//!   missing rather than a value quietly interpolated over.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context as _, Result, bail, ensure};
use glam::{DMat4, DQuat, DVec3};

use super::gltf_world::{gltf_to_blender_rotation, node_name, rotation_of, world_nodes};
use super::motion::{Frame, Motion};

/// How close two key times must be to count as the same frame, in seconds.
///
/// Every channel of one action shares its input accessor in practice, so the
/// values are bit identical and this only guards against an exporter that
/// writes one accessor per channel. It is four orders under the shortest real
/// frame, which is 1/120 s.
const SAME_FRAME_SECONDS: f64 = 1e-6;

/// One clip's world rotations per role, in Blender Z-up world space.
///
/// `bones` maps each role to the bone that fills it on this rig. A role whose
/// bone the file does not have is absent from the result, and the rule that
/// asked for it reports the gap rather than this reader hiding it.
pub fn read(bytes: &[u8], bones: &BTreeMap<String, String>) -> Result<Motion> {
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
    // Node index rather than name, so a mesh node sharing a bone's name
    // cannot answer for it.
    let role_nodes: BTreeMap<String, usize> = bones
        .iter()
        .filter_map(|(role, bone)| {
            let found = scene.iter().find(|entry| {
                joints.contains(&entry.node.index()) && node_name(&entry.node) == *bone
            })?;
            Some((role.clone(), found.node.index()))
        })
        .collect();
    let rest: BTreeMap<usize, DMat4> = scene
        .iter()
        .map(|entry| (entry.node.index(), entry.world))
        .collect();

    let channels = Channels::read(document, gltf.blob.as_deref())?;
    let times = channels.key_times(&joints);
    ensure!(
        !times.is_empty(),
        "the file carries no animation, so it holds no clip"
    );

    let tree = Tree::of(&scene)?;
    let by_role = |world: &BTreeMap<usize, DMat4>| {
        role_nodes
            .iter()
            .filter_map(|(role, node)| {
                let rotation = rotation_of(*world.get(node)?)?;
                Some((role.clone(), gltf_to_blender_rotation(rotation)))
            })
            .collect::<BTreeMap<String, DQuat>>()
    };
    let frames = times
        .iter()
        .map(|seconds| Frame {
            seconds: *seconds,
            rotations: by_role(&tree.world_at(&channels, *seconds)),
        })
        .collect();
    Motion::new(by_role(&rest), frames)
}

/// One clip's own key grid, and the pose at each end of it.
///
/// [`read`] answers where every bone pointed, which needs the role map and a
/// composed world matrix. These two rules need neither: `clip.fps_grid` asks
/// whether the file's key times land on whole frames of the rate the library
/// declares, and `clip.loop` asks whether the last pose is the first one
/// again.
pub struct Keys {
    /// Seconds from the file's own first key, in order.
    ///
    /// Counted from the first key rather than from zero, because a Mixamo
    /// clip runs frames 1 to 21 and a Meshy one starts at 0. Where a clip
    /// starts is not a defect; how far apart its keys sit is.
    pub seconds: Vec<f64>,
    /// Joint name to its own local rotation at the first key and at the last.
    ///
    /// Local, so one wrong hips reports once rather than dragging every bone
    /// below it into the count.
    pub ends: BTreeMap<String, (DQuat, DQuat)>,
}

/// The key grid and the two end poses of one clip.
pub fn keys(bytes: &[u8]) -> Result<Keys> {
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
    let channels = Channels::read(document, gltf.blob.as_deref())?;
    let times = channels.key_times(&joints);
    ensure!(
        !times.is_empty(),
        "the file carries no animation, so it holds no clip"
    );
    let (first, last) = (times[0], times[times.len() - 1]);
    let ends = world_nodes(document)?
        .iter()
        .filter(|entry| joints.contains(&entry.node.index()))
        .map(|entry| {
            let declared = Trs::of(&entry.node)?;
            let at = |seconds| {
                channels
                    .sampled(entry.node.index(), declared, seconds)
                    .rotation
            };
            Ok((node_name(&entry.node), (at(first), at(last))))
        })
        .collect::<Result<_>>()?;
    Ok(Keys {
        seconds: times.iter().map(|time| time - first).collect(),
        ends,
    })
}

/// The node graph, parents first, with each node's own declared transform.
struct Tree {
    /// Node indices in the order `world_nodes` returns them, which is parents
    /// first, so a node's parent is always composed before the node asks.
    order: Vec<usize>,
    parents: BTreeMap<usize, usize>,
    declared: BTreeMap<usize, Trs>,
}

impl Tree {
    fn of(scene: &[super::gltf_world::WorldNode<'_>]) -> Result<Self> {
        Ok(Self {
            order: scene.iter().map(|entry| entry.node.index()).collect(),
            parents: scene
                .iter()
                .filter_map(|entry| Some((entry.node.index(), entry.parent?)))
                .collect(),
            declared: scene
                .iter()
                .map(|entry| Ok((entry.node.index(), Trs::of(&entry.node)?)))
                .collect::<Result<_>>()?,
        })
    }

    /// Every node's world transform at one instant.
    fn world_at(&self, channels: &Channels, seconds: f64) -> BTreeMap<usize, DMat4> {
        let mut world: BTreeMap<usize, DMat4> = BTreeMap::new();
        for index in &self.order {
            let local = channels
                .sampled(*index, self.declared[index], seconds)
                .matrix();
            let above = self
                .parents
                .get(index)
                .and_then(|parent| world.get(parent))
                .copied()
                .unwrap_or(DMat4::IDENTITY);
            world.insert(*index, above * local);
        }
        world
    }
}

/// One node's own transform, as the three parts an animation drives.
#[derive(Debug, Clone, Copy)]
struct Trs {
    translation: DVec3,
    rotation: DQuat,
    scale: DVec3,
}

impl Trs {
    /// A node's declared transform. glTF allows a matrix instead of the three
    /// parts, and an animation still drives the parts, so a matrix is
    /// decomposed here rather than at every sample.
    fn of(node: &gltf::Node<'_>) -> Result<Self> {
        let (translation, rotation, scale) = node.transform().decomposed();
        Ok(Self {
            translation: DVec3::from(translation.map(f64::from)),
            rotation: unit(rotation)
                .with_context(|| format!("the transform of node {}", node.index()))?,
            scale: DVec3::from(scale.map(f64::from)),
        })
    }

    fn matrix(self) -> DMat4 {
        DMat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }
}

/// One sampler's values, by the property it drives.
enum Samples {
    Vectors(Vec<DVec3>),
    Rotations(Vec<DQuat>),
}

impl Samples {
    fn len(&self) -> usize {
        match self {
            Self::Vectors(values) => values.len(),
            Self::Rotations(values) => values.len(),
        }
    }
}

struct Sampler {
    times: Vec<f64>,
    values: Samples,
    /// glTF's `STEP` holds the previous key rather than interpolating. The
    /// exporter writes it for a channel that never moves.
    step: bool,
}

impl Sampler {
    /// Which two keys surround `seconds`, and how far between them it sits.
    /// Outside the clip the nearest key is held, which is what the glTF
    /// specification says a sampler does.
    fn span(&self, seconds: f64) -> (usize, usize, f64) {
        let last = self.times.len() - 1;
        if seconds <= self.times[0] {
            return (0, 0, 0.0);
        }
        let Some(after) = self.times.iter().position(|time| *time > seconds) else {
            return (last, last, 0.0);
        };
        let before = after - 1;
        let width = self.times[after] - self.times[before];
        let fraction = if self.step || width <= 0.0 {
            0.0
        } else {
            (seconds - self.times[before]) / width
        };
        (before, after, fraction)
    }

    fn rotation(&self, seconds: f64) -> Option<DQuat> {
        let Samples::Rotations(values) = &self.values else {
            return None;
        };
        let (before, after, fraction) = self.span(seconds);
        Some(values[before].slerp(values[after], fraction))
    }

    fn vector(&self, seconds: f64) -> Option<DVec3> {
        let Samples::Vectors(values) = &self.values else {
            return None;
        };
        let (before, after, fraction) = self.span(seconds);
        Some(values[before].lerp(values[after], fraction))
    }
}

/// Every channel of the file, by the node and the property it drives.
#[derive(Default)]
struct Channels(BTreeMap<(usize, &'static str), Sampler>);

const TRANSLATION: &str = "translation";
const ROTATION: &str = "rotation";
const SCALE: &str = "scale";

impl Channels {
    fn read(document: &gltf::Document, blob: Option<&[u8]>) -> Result<Self> {
        let mut found = Self::default();
        for channel in document
            .animations()
            .flat_map(|animation| animation.channels())
        {
            let node = channel.target().node().index();
            let reader = channel.reader(|buffer| match buffer.source() {
                gltf::buffer::Source::Bin => blob,
                gltf::buffer::Source::Uri(_) => None,
            });
            let times: Vec<f64> = reader
                .read_inputs()
                .with_context(|| format!("node {node} has a sampler with no key times"))?
                .map(f64::from)
                .collect();
            let outputs = reader
                .read_outputs()
                .with_context(|| format!("node {node} has a sampler with no values"))?;
            // `CUBICSPLINE` stores two tangents beside every value, so its
            // output is three times as long and none of the maths below
            // applies. The retarget writes `LINEAR`, and `clip.interpolation`
            // is the rule that says so, hence a refusal rather than a guess.
            let step = match channel.sampler().interpolation() {
                gltf::animation::Interpolation::Linear => false,
                gltf::animation::Interpolation::Step => true,
                gltf::animation::Interpolation::CubicSpline => bail!(
                    "node {node} is driven by a CUBICSPLINE sampler, which this reader \
                     does not evaluate"
                ),
            };
            let (property, values) = match outputs {
                gltf::animation::util::ReadOutputs::Translations(values) => {
                    (TRANSLATION, Samples::Vectors(values.map(widen).collect()))
                }
                gltf::animation::util::ReadOutputs::Scales(values) => {
                    (SCALE, Samples::Vectors(values.map(widen).collect()))
                }
                gltf::animation::util::ReadOutputs::Rotations(values) => (
                    ROTATION,
                    Samples::Rotations(
                        values
                            .into_f32()
                            .map(|value| {
                                unit(value)
                                    .with_context(|| format!("a rotation key on node {node}"))
                            })
                            .collect::<Result<Vec<DQuat>>>()?,
                    ),
                ),
                // A morph target drives no joint, so no rule here reads one.
                gltf::animation::util::ReadOutputs::MorphTargetWeights(_) => continue,
            };
            ensure!(
                !times.is_empty() && times.len() == values.len(),
                "node {node} has {} key times against {} values",
                times.len(),
                values.len()
            );
            found.0.insert(
                (node, property),
                Sampler {
                    times,
                    values,
                    step,
                },
            );
        }
        Ok(found)
    }

    /// Every distinct key time that drives a joint, in order.
    fn key_times(&self, joints: &BTreeSet<usize>) -> Vec<f64> {
        let mut times: Vec<f64> = self
            .0
            .iter()
            .filter(|((node, _), _)| joints.contains(node))
            .flat_map(|(_, sampler)| sampler.times.iter().copied())
            .collect();
        times.sort_by(f64::total_cmp);
        times.dedup_by(|a, b| (*a - *b).abs() <= SAME_FRAME_SECONDS);
        times
    }

    /// One node's transform at one instant, falling back to its own declared
    /// value for a property no channel drives.
    fn sampled(&self, node: usize, declared: Trs, seconds: f64) -> Trs {
        let at = |property| self.0.get(&(node, property));
        Trs {
            translation: at(TRANSLATION)
                .and_then(|sampler| sampler.vector(seconds))
                .unwrap_or(declared.translation),
            rotation: at(ROTATION)
                .and_then(|sampler| sampler.rotation(seconds))
                .unwrap_or(declared.rotation),
            scale: at(SCALE)
                .and_then(|sampler| sampler.vector(seconds))
                .unwrap_or(declared.scale),
        }
    }
}

fn widen(value: [f32; 3]) -> DVec3 {
    DVec3::from(value.map(f64::from))
}

/// glTF stores a rotation as `x, y, z, w`. A row that is not a rotation is
/// refused here rather than normalized into a NaN three functions away.
fn unit(value: [f32; 4]) -> Result<DQuat> {
    let rotation = DQuat::from_xyzw(
        f64::from(value[0]),
        f64::from(value[1]),
        f64::from(value[2]),
        f64::from(value[3]),
    );
    ensure!(
        rotation.is_finite() && rotation.length() > 0.0,
        "holds {value:?}, which is no rotation at all"
    );
    Ok(rotation.normalize())
}
