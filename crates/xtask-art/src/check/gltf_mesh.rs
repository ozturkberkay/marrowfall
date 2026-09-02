//! The mesh surface, read out of a glTF file: world space first, then
//! welded.
//!
//! Order is the whole point. Two traps live here and both have already
//! produced confident, wrong numbers.
//!
//! - **glTF splits one vertex at every UV seam.** A naive read of the
//!   survivor counts 13,368 boundary edges on a mesh that has 171, and a
//!   second tool agreed to the integer because it shared the representation.
//!   So the positions are welded before a single edge is counted.
//! - **The node chain is the transform.** This asset family carries a 100x
//!   node scale. Meshy's own repair extension reported success having
//!   deleted nothing, because it measured piece volume in local space while
//!   its checker measured in world space. So the transform comes first and
//!   every constant below is world-space meters.
//!
//! The weld distance is a parameter, not an assumption. [`merge_histogram`]
//! is what picks it: the count is read at several distances and the plateau
//! is the answer.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use anyhow::{Context as _, Result, anyhow, ensure};
use glam::{DMat4, DVec2, DVec3};

use super::gltf_world::{node_name, world_nodes};

/// The weld distance every mesh gate measures at, in world-space meters.
///
/// Chosen from [`merge_histogram`], and published in
/// `docs/design/2026_08_20_art_pipeline_foundations.md`: the welded vertex
/// count is identical from 1e-9 m to 1e-4 m and only moves at 1e-3 m, so
/// 1e-5 sits four orders inside the plateau at each end. It is four orders
/// of magnitude under the smallest real feature of a 1.7 m character and two
/// orders over the `f32` noise floor the file stores its positions at.
pub const WELD_METERS: f64 = 1e-5;

/// The distances the histogram is read at, smallest first.
///
/// The design named the middle four. 1e-9 is here because the lower edge of
/// the plateau is what proves 1e-5 is not sitting next to a cliff, and a
/// claim about it has to be measured rather than asserted in a comment.
pub const HISTOGRAM_METERS: [f64; 5] = [1e-9, 1e-6, 1e-5, 1e-4, 1e-3];

/// One mesh object, as the file names it.
///
/// The counts here are per object rather than per file, so a finding names
/// the object a reviewer has to open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Object {
    /// The node's name, which is what `[profile].meshes` allows.
    pub name: String,
    pub primitives: usize,
    pub triangles: usize,
    /// Primitives no rule below could read, because glTF's mode says they
    /// are points, lines or strips rather than triangles.
    pub unreadable_primitives: usize,
    /// Primitives whose material carries no base color image. Rigging
    /// refuses an untextured mesh, so this is its precondition.
    pub untextured_primitives: usize,
    /// Texture coordinates outside the `[0,1]` tile, counted as delivered.
    /// Welding cannot touch these: a seam is one position with two
    /// coordinates, which is the whole reason a naive read miscounts edges.
    pub uvs_outside_the_tile: usize,
}

/// One glTF file as every mesh rule reads it: world space, then welded.
#[derive(Debug, Clone)]
pub struct Surface {
    objects: Vec<Object>,
    positions: Vec<DVec3>,
    triangles: Vec<[u32; 3]>,
    raw_vertices: usize,
    /// The distance this surface was welded at, or `None` for the unwelded
    /// reading that only [`Surface::unwelded`] produces.
    weld_meters: Option<f64>,
}

impl Surface {
    /// Reads one file at the published weld distance.
    pub fn read(path: &Path) -> Result<Self> {
        Self::read_welded_at(path, WELD_METERS)
    }

    /// Reads one file at a stated weld distance. Only the histogram and the
    /// tests that prove the weld matters pass anything but [`WELD_METERS`].
    pub fn read_welded_at(path: &Path, weld_meters: f64) -> Result<Self> {
        let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        Self::from_slice_welded_at(&bytes, weld_meters)
            .with_context(|| format!("in {}", path.display()))
    }

    pub fn from_slice(bytes: &[u8]) -> Result<Self> {
        Self::from_slice_welded_at(bytes, WELD_METERS)
    }

    pub fn from_slice_welded_at(bytes: &[u8], weld_meters: f64) -> Result<Self> {
        ensure!(
            weld_meters > 0.0,
            "the weld distance must be positive, got {weld_meters}"
        );
        Self::build(bytes, Some(weld_meters))
    }

    /// The same file with **no weld at all**: one vertex per stored vertex,
    /// seam duplicates included.
    ///
    /// This is the representation the last mesh audit measured, and it reads
    /// 13,368 boundary edges on a mesh that has 171. No gate uses it. It is
    /// here so a test can show what skipping the first step costs, which is
    /// the only way to prove the step is doing anything.
    pub fn unwelded(bytes: &[u8]) -> Result<Self> {
        Self::build(bytes, None)
    }

    fn build(bytes: &[u8], weld_meters: Option<f64>) -> Result<Self> {
        let gltf = gltf::Gltf::from_slice(bytes).context("parsing the glTF")?;
        let blob = gltf.blob.as_deref();
        let scene = world_nodes(&gltf.document)?;
        let joints: BTreeMap<usize, DMat4> = scene
            .iter()
            .map(|entry| (entry.node.index(), entry.world))
            .collect();
        let mut raw = Raw::default();
        for entry in &scene {
            let Some(mesh) = entry.node.mesh() else {
                continue;
            };
            let name = node_name(&entry.node);
            let to_world = match entry.node.skin() {
                // The glTF specification: the node transform of a skinned
                // mesh MUST be ignored, because the skin's own joints carry
                // it. Reading the node chain instead measures the survivor's
                // 1.70 m mesh at 0.017 m.
                Some(skin) => ToWorld::Skin(Skinning::read(&skin, &joints, blob, &name)?),
                None => ToWorld::Node(entry.world),
            };
            raw.read_object(&name, &mesh, &to_world, blob)?;
        }
        ensure!(
            !raw.objects.is_empty(),
            "the scene holds no mesh, so there is no surface to measure"
        );
        let (positions, welded) = match weld_meters {
            Some(distance) => weld(&raw.positions, distance),
            None => (
                raw.positions.clone(),
                (0..raw.positions.len() as u32).collect(),
            ),
        };
        Ok(Self {
            objects: raw.objects,
            positions,
            triangles: raw
                .triangles
                .iter()
                .map(|corners| corners.map(|corner| welded[corner as usize]))
                .collect(),
            raw_vertices: raw.positions.len(),
            weld_meters,
        })
    }

    /// Every mesh object, in the order the scene holds them.
    pub fn objects(&self) -> &[Object] {
        &self.objects
    }

    /// Welded, world-space vertex positions. Every triangle indexes these.
    pub fn positions(&self) -> &[DVec3] {
        &self.positions
    }

    /// Every triangle, as three indices into [`Surface::positions`].
    pub fn triangles(&self) -> &[[u32; 3]] {
        &self.triangles
    }

    /// How many vertices the file stores before welding. The gap between
    /// this and `positions().len()` is the seam splitting.
    pub fn raw_vertices(&self) -> usize {
        self.raw_vertices
    }

    /// The distance this surface was welded at, and `None` when it was not
    /// welded at all.
    pub fn weld_meters(&self) -> Option<f64> {
        self.weld_meters
    }
}

/// How many vertices survive the weld at each of [`HISTOGRAM_METERS`].
///
/// The weld distance is itself a parameter: too large and it merges distinct
/// vertices, hiding holes. The plateau, where the count stops moving, is the
/// distance to use.
pub fn merge_histogram(bytes: &[u8]) -> Result<Vec<(f64, usize)>> {
    HISTOGRAM_METERS
        .into_iter()
        .map(|distance| {
            let surface = Surface::from_slice_welded_at(bytes, distance)?;
            Ok((distance, surface.positions().len()))
        })
        .collect()
}

/// What takes one primitive's vertices to world space.
enum ToWorld {
    /// The whole node chain, for a mesh that is not skinned.
    Node(DMat4),
    /// The skin's bind pose, for one that is.
    Skin(Skinning),
}

/// One skin's bind pose: a world matrix per joint, in the skin's own order.
///
/// A skinned vertex sits where its joints put it, so the transform is per
/// vertex and the mesh node's own matrix means nothing.
struct Skinning {
    /// `joint world * inverse bind matrix`, already composed.
    joints: Vec<DMat4>,
}

impl Skinning {
    fn read(
        skin: &gltf::Skin<'_>,
        world: &BTreeMap<usize, DMat4>,
        blob: Option<&[u8]>,
        object: &str,
    ) -> Result<Self> {
        let reader = skin.reader(|buffer| buffer_data(&buffer, blob));
        // Absent means identity: the specification says the matrices were
        // pre-applied.
        let mut inverse_bind = reader
            .read_inverse_bind_matrices()
            .map(|matrices| matrices.map(widen).collect::<Vec<DMat4>>())
            .unwrap_or_default();
        let count = skin.joints().count();
        // A shortfall is malformed input, not the absent case. Part of this
        // asset family's 0.01 scale lives in the inverse bind matrix, so an
        // invented identity would put every vertex on that joint 100x out,
        // and say nothing.
        ensure!(
            inverse_bind.is_empty() || inverse_bind.len() >= count,
            "the skin of {object} has {} inverse bind matrices for {count} joints",
            inverse_bind.len()
        );
        inverse_bind.truncate(count);
        inverse_bind.resize(count, DMat4::IDENTITY);
        let joints = skin
            .joints()
            .zip(inverse_bind)
            .map(|(joint, inverse_bind)| {
                let world = world.get(&joint.index()).with_context(|| {
                    format!(
                        "the skin of {object} names node {} as a joint, which is not in \
                         the scene",
                        joint.index()
                    )
                })?;
                Ok(*world * inverse_bind)
            })
            .collect::<Result<Vec<DMat4>>>()?;
        ensure!(!joints.is_empty(), "the skin of {object} has no joint");
        Ok(Self { joints })
    }

    /// Where one vertex sits in the bind pose.
    ///
    /// Weights are renormalized by their own total, so a file whose weights
    /// drift from 1.0 is not silently shrunk toward the origin. There are
    /// exactly two ways a vertex has no place, and [`Unplaced`] is which.
    fn place(
        &self,
        position: DVec3,
        joints: [u16; 4],
        weights: [f32; 4],
    ) -> Result<DVec3, Unplaced> {
        let total: f64 = weights.iter().map(|weight| f64::from(*weight)).sum();
        if total <= 0.0 {
            return Err(Unplaced::NoWeight);
        }
        let mut placed = DVec3::ZERO;
        for (joint, weight) in joints.into_iter().zip(weights) {
            let weight = f64::from(weight) / total;
            if weight == 0.0 {
                continue;
            }
            let bind = self
                .joints
                .get(usize::from(joint))
                .ok_or(Unplaced::UnknownJoint(joint))?;
            placed += weight * bind.transform_point3(position);
        }
        Ok(placed)
    }
}

/// Why a skinned vertex has no place in the world. Two faults, two
/// sentences: one is a weight problem and the other is a hierarchy problem,
/// and reporting them as one would send a reader to the wrong file.
#[derive(Debug, Clone, Copy)]
enum Unplaced {
    /// Its weights total zero, so no joint moves it.
    NoWeight,
    /// It names a joint the skin does not list.
    UnknownJoint(u16),
}

impl std::fmt::Display for Unplaced {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoWeight => {
                write!(f, "is skinned to nothing, so it has no place in the world")
            }
            Self::UnknownJoint(joint) => {
                write!(f, "names joint {joint}, which the skin does not have")
            }
        }
    }
}

/// One matrix as glTF stores it, widened to `f64`.
fn widen(columns: [[f32; 4]; 4]) -> DMat4 {
    DMat4::from_cols_array_2d(&columns.map(|column| column.map(f64::from)))
}

/// A GLB carries its data inline. An external or data URI buffer would need
/// a fetch, and measuring the part that did load is how a gate reports a
/// precise number about half a mesh.
fn buffer_data<'a>(buffer: &gltf::Buffer<'_>, blob: Option<&'a [u8]>) -> Option<&'a [u8]> {
    match buffer.source() {
        gltf::buffer::Source::Bin => blob,
        gltf::buffer::Source::Uri(_) => None,
    }
}

/// Every primitive of the file, taken to world space, before welding.
#[derive(Default)]
struct Raw {
    objects: Vec<Object>,
    positions: Vec<DVec3>,
    triangles: Vec<[u32; 3]>,
}

impl Raw {
    fn read_object(
        &mut self,
        name: &str,
        mesh: &gltf::Mesh<'_>,
        to_world: &ToWorld,
        blob: Option<&[u8]>,
    ) -> Result<()> {
        let mut object = Object {
            name: name.to_owned(),
            primitives: mesh.primitives().len(),
            triangles: 0,
            unreadable_primitives: 0,
            untextured_primitives: 0,
            uvs_outside_the_tile: 0,
        };
        for primitive in mesh.primitives() {
            if primitive
                .material()
                .pbr_metallic_roughness()
                .base_color_texture()
                .is_none()
            {
                object.untextured_primitives += 1;
            }
            // Points, lines and strips carry no triangles, so every rule
            // below would measure nothing and say nothing. Counted here and
            // refused by `mesh.quads`.
            if primitive.mode() != gltf::mesh::Mode::Triangles {
                object.unreadable_primitives += 1;
                continue;
            }
            let reader = primitive.reader(|buffer| buffer_data(&buffer, blob));
            let positions = reader.read_positions().with_context(|| {
                format!("{name} has a primitive whose vertex positions are not in the file")
            })?;
            let indices = reader
                .read_indices()
                .with_context(|| format!("{name} has a primitive with no indices"))?
                .into_u32();
            if let Some(uvs) = reader.read_tex_coords(0) {
                object.uvs_outside_the_tile += uvs
                    .into_f32()
                    .map(|uv| DVec2::new(f64::from(uv[0]), f64::from(uv[1])))
                    .filter(|uv| outside_the_tile(*uv))
                    .count();
            }

            let base = u32::try_from(self.positions.len()).context("too many vertices to index")?;
            let local = positions.map(|p| DVec3::new(p[0].into(), p[1].into(), p[2].into()));
            match to_world {
                ToWorld::Node(world) => self
                    .positions
                    .extend(local.map(|p| world.transform_point3(p))),
                ToWorld::Skin(skinning) => {
                    let joints = reader
                        .read_joints(0)
                        .with_context(|| format!("{name} is skinned but names no joint"))?
                        .into_u16();
                    let weights = reader
                        .read_weights(0)
                        .with_context(|| format!("{name} is skinned but carries no weight"))?
                        .into_f32();
                    for (index, ((position, joints), weights)) in
                        local.zip(joints).zip(weights).enumerate()
                    {
                        self.positions.push(
                            skinning
                                .place(position, joints, weights)
                                .map_err(|why| anyhow!("vertex {index} of {name} {why}"))?,
                        );
                    }
                }
            }
            let corners: Vec<u32> = indices.map(|index| base + index).collect();
            ensure!(
                corners.len().is_multiple_of(3),
                "{name} has a triangle primitive with {} indices, which is not a whole \
                 number of triangles",
                corners.len()
            );
            for triangle in corners.chunks_exact(3) {
                self.triangles.push([triangle[0], triangle[1], triangle[2]]);
                object.triangles += 1;
            }
        }
        self.objects.push(object);
        Ok(())
    }
}

/// Whether a texture coordinate leaves the `[0,1]` tile. The bake samples one
/// tile, so anything outside it is a surface with no texture.
///
/// A NaN is outside too: it compares false against both ends, so `contains`
/// is false and the negation catches it.
fn outside_the_tile(uv: DVec2) -> bool {
    [uv.x, uv.y]
        .into_iter()
        .any(|part| !(0.0..=1.0).contains(&part))
}

/// Merges coincident positions, and says which welded vertex each raw one
/// became.
///
/// A grid of `distance`-sized cells, and each raw vertex joins the nearest
/// welded position within `distance` from the 27 cells around it. The 27
/// matter: rounding alone splits two positions a nanometer apart when they
/// straddle a cell boundary, which is a hole invented out of arithmetic.
fn weld(raw: &[DVec3], distance: f64) -> (Vec<DVec3>, Vec<u32>) {
    let mut welded: Vec<DVec3> = Vec::new();
    let mut of_raw: Vec<u32> = Vec::with_capacity(raw.len());
    let mut grid: HashMap<[i64; 3], Vec<u32>> = HashMap::new();
    for position in raw {
        let cell = cell_of(*position, distance);
        let nearest = neighborhood(cell)
            .flat_map(|around| grid.get(&around).map_or(&[][..], Vec::as_slice))
            .map(|&index| (welded[index as usize].distance_squared(*position), index))
            .filter(|(apart, _)| *apart <= distance * distance)
            .min_by(|a, b| a.0.total_cmp(&b.0));
        let index = match nearest {
            Some((_, index)) => index,
            None => {
                let index = welded.len() as u32;
                welded.push(*position);
                grid.entry(cell).or_default().push(index);
                index
            }
        };
        of_raw.push(index);
    }
    (welded, of_raw)
}

fn cell_of(position: DVec3, distance: f64) -> [i64; 3] {
    [position.x, position.y, position.z].map(|part| (part / distance).floor() as i64)
}

/// The cell and its 26 neighbors.
fn neighborhood(cell: [i64; 3]) -> impl Iterator<Item = [i64; 3]> {
    (-1..=1).flat_map(move |x| {
        (-1..=1).flat_map(move |y| (-1..=1).map(move |z| [cell[0] + x, cell[1] + y, cell[2] + z]))
    })
}

/// An edge of the welded surface, named the same way whichever triangle
/// walks it, so the two sides of one edge are one key.
pub fn edge(a: u32, b: u32) -> (u32, u32) {
    if a <= b { (a, b) } else { (b, a) }
}

/// How many triangles use each edge of the welded surface.
///
/// One is a boundary edge, which is a hole. Three or more is non-manifold,
/// which is what makes weight painting unable to tell which bone a vertex
/// belongs to.
pub fn edge_use(surface: &Surface) -> HashMap<(u32, u32), usize> {
    let mut counts: HashMap<(u32, u32), usize> = HashMap::new();
    for [a, b, c] in surface.triangles() {
        for (from, to) in [(*a, *b), (*b, *c), (*c, *a)] {
            *counts.entry(edge(from, to)).or_default() += 1;
        }
    }
    counts
}

/// How many connected components the welded surface has, counted over
/// triangles that share a vertex.
///
/// A vertex no triangle uses is not a component: it renders as nothing, and
/// counting it would report debris the fixer cannot see.
pub fn islands(surface: &Surface) -> usize {
    let mut groups = Groups::new(surface.positions().len());
    for [a, b, c] in surface.triangles() {
        groups.join(*a, *b);
        groups.join(*a, *c);
    }
    let mut roots: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
    for [a, ..] in surface.triangles() {
        roots.insert(groups.root(*a));
    }
    roots.len()
}

/// Union-find over vertex indices, for the island count.
struct Groups {
    parent: Vec<u32>,
}

impl Groups {
    fn new(vertices: usize) -> Self {
        Self {
            parent: (0..vertices as u32).collect(),
        }
    }

    fn root(&mut self, of: u32) -> u32 {
        let mut at = of;
        while self.parent[at as usize] != at {
            let grandparent = self.parent[self.parent[at as usize] as usize];
            self.parent[at as usize] = grandparent; // Path halving.
            at = grandparent;
        }
        at
    }

    fn join(&mut self, a: u32, b: u32) {
        let (a, b) = (self.root(a), self.root(b));
        if a != b {
            self.parent[a as usize] = b;
        }
    }
}
