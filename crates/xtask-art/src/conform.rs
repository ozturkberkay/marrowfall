//! Two edits that make a bought rig conformant, on the bytes and never on
//! the geometry.
//!
//! **Rename.** A vendor names its bones its own way, and Meshy numbers its
//! spine from the top so its `Spine` is our `Spine2`. Every joint is rewritten
//! by role, through the convention the file's own `[fingerprints]` names and
//! the canonical one. glTF addresses a joint by node index, so a name is one
//! string in the JSON chunk and the buffer is untouched.
//!
//! **Conform.** An auto-rigger points a joint wherever it likes and places
//! limb joints a few percent asymmetric even on a symmetrized mesh, which T12
//! measured on three fresh rigs. Every joint whose own axis misses the child
//! `[profile.tails]` names is turned onto it, and every mirrored pair is
//! averaged onto X = 0 first.
//!
//! **The mesh does not move, and that is arithmetic rather than care.** glTF
//! skins a vertex through `world(joint) @ inverseBind(joint)`, so recomputing
//! each inverse bind matrix to hold that product is holding the vertex, and
//! no vertex, weight, UV or texel is read at all. What is left is the `f32` a
//! GLB stores: the joints of the committed rig move millimeters and its
//! furthest rest vertex moves 1.57e-7 m.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context as _, Result, bail, ensure};
use glam::{DMat3, DMat4, DQuat, DVec3};
use gltf::animation::Property;
use serde_json::json;

use crate::check::Symmetry;
use crate::check::aim::{AimTable, bare_bone_name};
use crate::check::gltf_world::{
    SHORTEST_SEGMENT_METERS, degrees_between, local, node_name, world_nodes,
};
use crate::check::profile::{Profile, reflected};
use crate::glb::Glb;

/// How far a bind-pose key may sit from the rest transform it repeats, in the
/// file's own units. Both rigs measured here read under 5.2e-5, which is the
/// `f32` a GLB stores, and this file family carries a 0.01 armature scale, so
/// this is 1e-5 m on the body.
const REST_KEY_TOLERANCE: f64 = 1e-3;

/// Rewrites every joint name by role, from whatever convention the file is
/// named in to the canonical one.
///
/// A file already in the canonical convention is returned byte for byte, so
/// running this twice is running it once.
pub fn rename(bytes: &[u8], table: &AimTable) -> Result<Vec<u8>> {
    let gltf = gltf::Gltf::from_slice(bytes).context("parsing the glTF")?;
    let joints: BTreeMap<usize, String> = gltf
        .document
        .skins()
        .flat_map(|skin| skin.joints())
        .map(|joint| (joint.index(), node_name(&joint)))
        .collect();
    ensure!(
        !joints.is_empty(),
        "the file declares no skin, so it holds no bone to rename"
    );

    let convention = table.convention_of(joints.values().map(String::as_str))?;
    if convention == table.canonical() {
        return Ok(bytes.to_vec());
    }
    // By bare name, so a namespaced vendor file maps too, and the name a bone
    // is given is the canonical one without that namespace.
    let canonical = table.bones(table.canonical())?;
    let wanted: BTreeMap<String, &String> = table
        .bones(convention)?
        .iter()
        .filter_map(|(role, bone)| Some((bare_bone_name(bone), canonical.get(role)?)))
        .collect();

    let mut glb = Glb::parse(bytes)?;
    // Every name is decided before any is written: Meshy's `Spine02` becomes
    // `Spine` while another joint is still called `Spine`, so renaming one at
    // a time would collide with a name that is on its way out.
    let mut seen = BTreeSet::new();
    for (index, name) in &joints {
        let to = wanted.get(&bare_bone_name(name)).map_or(name, |to| *to);
        ensure!(
            seen.insert(to.clone()),
            "the rename would give two joints the name {to:?}, so no rule \
             could say which one it measured"
        );
        glb.document["nodes"][*index]["name"] = json!(to);
    }
    glb.to_bytes()
}

/// Turns every joint's rest axis onto the child the profile names for it, and
/// mirrors the pairs the mirror rules compare when the character is declared
/// symmetric.
///
/// Only the node transforms, the inverse bind matrices and the vendor's
/// bind-pose keys are written, so every vertex accessor comes out byte
/// identical.
pub fn conform(bytes: &[u8], profile: &Profile, symmetry: Symmetry) -> Result<Vec<u8>> {
    let gltf = gltf::Gltf::from_slice(bytes).context("parsing the glTF")?;
    let document = &gltf.document;
    let blob = gltf.blob.as_deref().unwrap_or_default();
    let scene = world_nodes(document)?;
    let joints: BTreeSet<usize> = document
        .skins()
        .flat_map(|skin| skin.joints())
        .map(|joint| joint.index())
        .collect();
    ensure!(
        !joints.is_empty(),
        "the file declares no skin, so it holds no rig to conform"
    );

    let mut world: BTreeMap<usize, DMat4> = BTreeMap::new();
    let mut index_of: BTreeMap<String, usize> = BTreeMap::new();
    for entry in &scene {
        let index = entry.node.index();
        world.insert(index, entry.world);
        if joints.contains(&index) {
            let name = node_name(&entry.node);
            ensure!(
                index_of.insert(name.clone(), index).is_none(),
                "two joints are named {name:?}, so nothing can say which one a \
                 profile row is about"
            );
        }
    }

    let placed = place(&index_of, &world, profile, symmetry);
    let aimed = aim(&index_of, &world, &placed, profile);

    // Parents first, so a child is re-expressed against the frame its parent
    // was actually given. The local transform is rounded to `f32` before the
    // world is taken from it, which is what keeps the recomputed inverse bind
    // matrices exact against the file a reader will open.
    let mut moved: BTreeMap<usize, DMat4> = BTreeMap::new();
    let mut locals: BTreeMap<usize, Trs> = BTreeMap::new();
    for entry in &scene {
        let index = entry.node.index();
        let above = entry
            .parent
            .map_or(DMat4::IDENTITY, |parent| moved[&parent]);
        let own = match aimed.get(&index) {
            Some(target) => Trs::of(above.inverse() * *target),
            None => Trs::of(local(&entry.node)),
        };
        moved.insert(index, above * own.matrix());
        if joints.contains(&index) {
            locals.insert(index, own);
        }
    }

    let mut glb = Glb::parse(bytes)?;
    for (index, own) in &locals {
        let node = glb.document["nodes"][*index]
            .as_object_mut()
            .with_context(|| format!("node {index} is not an object"))?;
        node.insert("translation".to_owned(), json!(own.translation));
        node.insert("rotation".to_owned(), json!(own.rotation));
        node.insert("scale".to_owned(), json!(own.scale));
        // The two spellings are exclusive, and this one is now stale.
        node.remove("matrix");
    }

    let mut writes = bind_matrices(blob, document, &world, &moved)?;
    writes.extend(bind_pose_keys(blob, document, &locals)?);
    apply(&mut glb, document, &writes)?;
    glb.to_bytes()
}

/// Where every joint sits after the mirror, by node index.
///
/// The pairs are the ones `rig.mirror_length` compares, read from
/// `[profile.tails]` rather than from a table of their own, so a skeleton with
/// no limbs mirrors nothing. Left is averaged with the reflected right and the
/// right is then that average reflected, which makes both mirror rules read
/// exactly zero rather than nearly it.
fn place(
    index_of: &BTreeMap<String, usize>,
    world: &BTreeMap<usize, DMat4>,
    profile: &Profile,
    symmetry: Symmetry,
) -> BTreeMap<usize, DVec3> {
    let mut placed: BTreeMap<usize, DVec3> = index_of
        .values()
        .map(|joint| (*joint, world[joint].w_axis.truncate()))
        .collect();
    if symmetry == Symmetry::Declined {
        return placed;
    }
    for pair in profile.mirror_pairs() {
        for (left, right) in [(&pair.left.0, &pair.right.0), (&pair.left.1, &pair.right.1)] {
            // A side the rig does not have is `rig.bone_set`'s to report, and
            // there is no pair here to average.
            let (Some(left), Some(right)) = (index_of.get(left), index_of.get(right)) else {
                continue;
            };
            let averaged = (placed[left] + reflected(placed[right])) / 2.0;
            placed.insert(*left, averaged);
            placed.insert(*right, reflected(averaged));
        }
    }
    placed
}

/// The world matrix every joint is given: its new position, and its own axis
/// turned onto the direction to the child `[profile.tails]` names.
fn aim(
    index_of: &BTreeMap<String, usize>,
    world: &BTreeMap<usize, DMat4>,
    placed: &BTreeMap<usize, DVec3>,
    profile: &Profile,
) -> BTreeMap<usize, DMat4> {
    index_of
        .iter()
        .map(|(name, joint)| {
            let rest = world[joint];
            let mut basis = DMat3::from_cols(
                rest.x_axis.truncate(),
                rest.y_axis.truncate(),
                rest.z_axis.truncate(),
            );
            if let Some(turn) = correction(name, *joint, index_of, placed, rest, profile) {
                basis = DMat3::from_quat(turn) * basis;
            }
            let matrix = DMat4::from_cols(
                basis.x_axis.extend(0.0),
                basis.y_axis.extend(0.0),
                basis.z_axis.extend(0.0),
                placed[joint].extend(1.0),
            );
            (*joint, matrix)
        })
        .collect()
}

/// The smallest rotation putting one joint's own axis on the direction to its
/// tail, about `own x wanted`, or `None` when there is nothing to correct.
///
/// A joint already inside `child_axis_tolerance_degrees` is left alone, so a
/// conformant rig comes back unchanged. A leaf has no tail to point at.
fn correction(
    name: &str,
    joint: usize,
    index_of: &BTreeMap<String, usize>,
    placed: &BTreeMap<usize, DVec3>,
    rest: DMat4,
    profile: &Profile,
) -> Option<DQuat> {
    let tail = index_of.get(profile.tail(name)?)?;
    let own = (rest * profile.child_axis.vector().extend(0.0))
        .truncate()
        .try_normalize()?;
    let step = placed[tail] - placed[&joint];
    // Two joints on top of each other give a direction that is cancellation
    // noise, and turning a bone onto noise is worse than leaving it.
    let wanted = (step.length() > SHORTEST_SEGMENT_METERS).then(|| step.normalize())?;
    (degrees_between(own, wanted) > profile.child_axis_tolerance_degrees)
        .then(|| DQuat::from_rotation_arc(own, wanted))
}

/// Every inverse bind matrix, recomputed so that each joint's skinning
/// matrix is the one the file already carried.
///
/// glTF skins a vertex through `world(joint) @ inverseBind(joint)`, so
/// holding that product fixed is what "no vertex moved" means, exactly. It is
/// `inverse(world)` on a file whose bind pose is its rest pose, which is
/// every file this pipeline rigs, and it stays right on one whose is not.
fn bind_matrices(
    blob: &[u8],
    document: &gltf::Document,
    world: &BTreeMap<usize, DMat4>,
    moved: &BTreeMap<usize, DMat4>,
) -> Result<Vec<(usize, Vec<f32>)>> {
    document
        .skins()
        .map(|skin| {
            let accessor = skin.inverse_bind_matrices().with_context(|| {
                format!("skin {} declares no inverse bind matrices", skin.index())
            })?;
            let bind = floats(blob, &accessor)?;
            let wanted = skin.joints().count() * 16;
            ensure!(
                bind.len() == wanted,
                "skin {} has {} joints and {} inverse bind numbers, and a \
                 matrix is sixteen of them",
                skin.index(),
                skin.joints().count(),
                bind.len()
            );
            let values = skin
                .joints()
                .enumerate()
                .flat_map(|(at, joint)| {
                    let was = DMat4::from_cols_array(&std::array::from_fn(|cell| {
                        f64::from(bind[at * 16 + cell])
                    }));
                    let index = joint.index();
                    narrow(&(moved[&index].inverse() * world[&index] * was).to_cols_array())
                })
                .collect();
            Ok((accessor.index(), values))
        })
        .collect()
}

/// The vendor's bind-pose action, re-keyed from the new rest transforms.
///
/// Meshy ships one animation of one key per channel, every key repeating the
/// node's own rest transform. Left alone through a rest-frame edit it would
/// become an action that puts the old rest pose back, so it is rewritten. A
/// channel that is anything else is motion, and motion belongs in
/// `art/animations/`.
fn bind_pose_keys(
    blob: &[u8],
    document: &gltf::Document,
    locals: &BTreeMap<usize, Trs>,
) -> Result<Vec<(usize, Vec<f32>)>> {
    let mut writes = Vec::new();
    for animation in document.animations() {
        for channel in animation.channels() {
            let node = channel.target().node();
            let name = node_name(&node);
            let sampler = channel.sampler();
            let keys = sampler.input().count();
            ensure!(
                keys == 1,
                "{:?} drives {name} over {keys} keys, so it is motion rather \
                 than a bind pose, and motion belongs in art/animations/",
                animation.name().unwrap_or("an animation")
            );
            // A node the conform did not move keeps the transform it had, so
            // its key is rewritten to the same numbers.
            let was = Trs::from(node.transform().decomposed());
            let becomes = locals.get(&node.index()).copied().unwrap_or(was);
            let (rest, keyed) = match channel.target().property() {
                Property::Translation => (was.translation.to_vec(), becomes.translation.to_vec()),
                Property::Rotation => (was.rotation.to_vec(), becomes.rotation.to_vec()),
                Property::Scale => (was.scale.to_vec(), becomes.scale.to_vec()),
                property => {
                    bail!("a channel drives {name}'s {property:?}, which is no rest transform")
                }
            };
            keyed_at_rest(blob, &sampler.output(), &rest, &name)?;
            writes.push((sampler.output().index(), keyed));
        }
    }
    Ok(writes)
}

/// Refuses a channel whose one key is not the node's own rest transform.
///
/// A quaternion and its negation are the same rotation, hence the second
/// comparison: without it half of them would read as a full turn away.
fn keyed_at_rest(
    blob: &[u8],
    accessor: &gltf::Accessor<'_>,
    rest: &[f32],
    name: &str,
) -> Result<()> {
    let key = floats(blob, accessor)?;
    ensure!(
        key.len() == rest.len(),
        "a channel keys {} numbers for {name} where its rest transform holds {}",
        key.len(),
        rest.len()
    );
    let apart = |sign: f64| {
        key.iter()
            .zip(rest)
            .map(|(key, rest)| f64::from(*key).mul_add(1.0, -sign * f64::from(*rest)).abs())
            .fold(0.0_f64, f64::max)
    };
    let off = apart(1.0).min(apart(-1.0));
    ensure!(
        off <= REST_KEY_TOLERANCE,
        "a channel keys {name} {off:.6} away from its own rest transform, so \
         it is a pose rather than a bind pose"
    );
    Ok(())
}

/// One accessor's floats, read out of the buffer chunk.
fn floats(blob: &[u8], accessor: &gltf::Accessor<'_>) -> Result<Vec<f32>> {
    let (offset, length) = f32_extent(accessor)?;
    let bytes = blob
        .get(offset..offset + length)
        .with_context(|| format!("accessor {} runs past the buffer", accessor.index()))?;
    Ok(bytes
        .chunks_exact(4)
        .map(|four| f32::from_le_bytes(four.try_into().expect("four bytes")))
        .collect())
}

/// Writes every value into the buffer chunk, in place.
///
/// In place rather than repacked, because that is what makes "the vertex
/// accessors are byte identical" a fact about the file rather than a hope.
/// Each range is checked against every accessor the file declares: one that
/// shared bytes with these would be rewritten without a word.
fn apply(glb: &mut Glb, document: &gltf::Document, writes: &[(usize, Vec<f32>)]) -> Result<()> {
    let mut ranges: BTreeMap<usize, (usize, usize)> = BTreeMap::new();
    for (index, values) in writes {
        let accessor = document
            .accessors()
            .nth(*index)
            .with_context(|| format!("the file declares no accessor {index}"))?;
        let (offset, length) = f32_extent(&accessor)?;
        ensure!(
            length == values.len() * 4,
            "accessor {index} holds {length} bytes and {} were computed for it",
            values.len() * 4
        );
        ensure!(
            ranges.insert(*index, (offset, length)).is_none(),
            "accessor {index} is written twice, so one of the two would win \
             without a word"
        );
    }
    for other in document.accessors() {
        let (start, length) = extent(&other)?;
        for (index, (offset, over)) in &ranges {
            ensure!(
                other.index() == *index || start >= offset + over || start + length <= *offset,
                "accessor {} shares bytes with accessor {index}, so rewriting \
                 one would rewrite the other",
                other.index()
            );
        }
    }
    for (index, values) in writes {
        let bytes: Vec<u8> = values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        glb.overwrite(ranges[index].0, &bytes)?;
    }
    Ok(())
}

/// The same, for the accessors this reads and writes: they hold the `f32`
/// glTF stores a matrix, a translation and a quaternion in.
///
/// A rotation may be stored as normalized integers instead, which is legal
/// glTF and another storage, so it is named rather than decoded four bytes at
/// a time.
fn f32_extent(accessor: &gltf::Accessor<'_>) -> Result<(usize, usize)> {
    ensure!(
        accessor.data_type() == gltf::accessor::DataType::F32,
        "accessor {} stores {:?}, and this reads and writes f32",
        accessor.index(),
        accessor.data_type()
    );
    extent(accessor)
}

/// Where one accessor's values start in the buffer chunk, and how many bytes
/// they take.
///
/// Refuses everything an in-place rewrite cannot do exactly: a strided view,
/// or a second buffer. Read on every accessor the file carries, vertex ones
/// included, so it says nothing about the component type.
fn extent(accessor: &gltf::Accessor<'_>) -> Result<(usize, usize)> {
    let view = accessor
        .view()
        .with_context(|| format!("accessor {} has no buffer view", accessor.index()))?;
    ensure!(
        view.stride().is_none(),
        "accessor {} is strided, so its values are not one run of bytes",
        accessor.index()
    );
    ensure!(
        view.buffer().index() == 0,
        "accessor {} reads buffer {}, and the GLB chunk is buffer 0",
        accessor.index(),
        view.buffer().index()
    );
    Ok((
        view.offset() + accessor.offset(),
        accessor.count() * accessor.size(),
    ))
}

/// `f64` values as the file stores them.
fn narrow<const N: usize>(values: &[f64; N]) -> [f32; N] {
    values.map(|value| value as f32)
}

/// One node transform as glTF writes it: three numbers, four, and three.
#[derive(Debug, Clone, Copy)]
struct Trs {
    translation: [f32; 3],
    /// x, y, z, w, which is glTF's order and not Blender's.
    rotation: [f32; 4],
    scale: [f32; 3],
}

impl Trs {
    fn of(matrix: DMat4) -> Self {
        let (scale, rotation, translation) = matrix.to_scale_rotation_translation();
        Self::from((
            narrow(&translation.to_array()),
            narrow(&rotation.to_array()),
            narrow(&scale.to_array()),
        ))
    }

    /// The same transform again, from the `f32` the file will hold, so the
    /// world matrices this composes into are the ones a reader will see.
    fn matrix(self) -> DMat4 {
        DMat4::from_scale_rotation_translation(
            DVec3::from(self.scale.map(f64::from)),
            DQuat::from_array(self.rotation.map(f64::from)).normalize(),
            DVec3::from(self.translation.map(f64::from)),
        )
    }
}

impl From<([f32; 3], [f32; 4], [f32; 3])> for Trs {
    fn from((translation, rotation, scale): ([f32; 3], [f32; 4], [f32; 3])) -> Self {
        Self {
            translation,
            rotation,
            scale,
        }
    }
}
