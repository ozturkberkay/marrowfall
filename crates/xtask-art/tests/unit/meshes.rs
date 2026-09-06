//! Hand-built GLB meshes, so every mesh rule has a positive fixture and a
//! negative one.
//!
//! [`SyntheticMesh::figure`] is the positive. Each negative is that same
//! figure with one thing broken, which is what makes a failure name the rule
//! rather than the fixture.
//!
//! Every fixture carries the shape the real art carries: a 0.01 scale on the
//! node above the mesh, and vertex coordinates 100x larger to match. A reader
//! that skips the node chain measures the figure at 170 m, which is the whole
//! reason [`SyntheticMesh::without_the_node_scale`] exists.
//!
//! Every published limit is calibrated on a real 55,000 triangle mesh, so a
//! synthetic negative has to beat a limit in the hundreds. That is what
//! [`SyntheticMesh::repeated`] is for: the same defect, laid out n times.
//!
//! These are GLBs, not JSON glTF, because a GLB carries its vertex data in
//! the binary chunk. That is the path the real assets take, and the reader
//! refuses a data URI buffer on purpose.

use glam::{DMat4, DQuat, DVec2, DVec3};
use serde_json::{Value, json};

use super::gltf_bytes::{Buffers, FLOAT, UNSIGNED_INT, UNSIGNED_SHORT, columns, glb};

/// The scale the node above the mesh carries in every file of this family.
pub const OBJECT_SCALE: f64 = 0.01;

/// What the figure is built to, and what the survivor's spec asks for.
pub const HEIGHT_METERS: f64 = 1.70;

/// How far the figure's bottom face sits forward of its top, in meters. This
/// is what gives it a facing: the lowest slice of a real character is its
/// feet, and they point the way it looks.
const FOOT_REACH: f64 = 0.10;

/// What a skinned fixture's mesh node carries. Legal glTF, and ignored by
/// the specification, so it is a 100x scale: a reader that applies it on top
/// of the skin measures the figure at 170 m.
fn wrong_on_a_skinned_node() -> DMat4 {
    DMat4::from_scale(DVec3::splat(100.0))
}

/// The name the profile allows.
pub const ALLOWED: &str = "char1";

/// glTF primitive modes, of the two this fixture writes.
const TRIANGLES: u32 = 4;
const LINES: u32 = 1;

/// One mesh object of a fixture.
#[derive(Debug, Clone)]
struct Part {
    name: String,
    /// The node above the geometry, as written.
    node: DMat4,
    /// Vertex positions, in **world** meters. The writer divides by
    /// [`OBJECT_SCALE`] on the way out, the way an exporter would.
    vertices: Vec<DVec3>,
    uvs: Vec<DVec2>,
    triangles: Vec<[u32; 3]>,
    mode: u32,
    textured: bool,
    /// Writes one index short of a whole triangle.
    half_triangle: bool,
}

/// A mesh under construction.
#[derive(Debug, Clone)]
pub struct SyntheticMesh {
    parts: Vec<Part>,
    /// The skin over the whole mesh, when the fixture has one.
    skin: Option<Skin>,
    /// Writes the same vertex data under an identity node, which is what a
    /// reader that skips the node chain sees.
    node_scale_removed: bool,
}

impl SyntheticMesh {
    /// A closed box, 1.70 m tall, symmetric across X = 0, whose bottom face
    /// reaches forward so the mesh has a facing. It breaks no rule.
    pub fn figure() -> Self {
        Self {
            parts: vec![Part {
                name: ALLOWED.to_owned(),
                node: DMat4::from_scale(DVec3::splat(OBJECT_SCALE)),
                vertices: Vec::new(),
                uvs: Vec::new(),
                triangles: Vec::new(),
                mode: TRIANGLES,
                textured: true,
                half_triangle: false,
            }],
            skin: None,
            node_scale_removed: false,
        }
        .plus_box(
            DVec3::new(0.0, HEIGHT_METERS / 2.0, 0.0),
            DVec3::new(0.30, HEIGHT_METERS, 0.20),
            FOOT_REACH,
        )
    }

    /// Adds a closed box to the first object, centered at `at`, of `size`,
    /// with its bottom face pushed `reach` along +Z.
    fn plus_box(mut self, at: DVec3, size: DVec3, reach: f64) -> Self {
        let half = size / 2.0;
        let base = self.parts[0].vertices.len() as u32;
        for (x, y, z) in [
            (-1.0, -1.0, -1.0),
            (1.0, -1.0, -1.0),
            (1.0, -1.0, 1.0),
            (-1.0, -1.0, 1.0),
            (-1.0, 1.0, -1.0),
            (1.0, 1.0, -1.0),
            (1.0, 1.0, 1.0),
            (-1.0, 1.0, 1.0),
        ] {
            let forward = if y < 0.0 { reach } else { 0.0 };
            self.parts[0]
                .vertices
                .push(at + DVec3::new(x * half.x, y * half.y, z * half.z + forward));
            // One tile, so `mesh.uv` has something in bounds to measure.
            self.parts[0]
                .uvs
                .push(DVec2::new((x + 1.0) / 2.0, (z + 1.0) / 2.0));
        }
        // Six quads as twelve triangles, wound so no edge is used twice the
        // same way. Bottom 0-3, top 4-7.
        for face in [
            [0, 2, 1],
            [0, 3, 2], // bottom
            [4, 5, 6],
            [4, 6, 7], // top
            [0, 1, 5],
            [0, 5, 4], // -Z
            [2, 3, 7],
            [2, 7, 6], // +Z
            [1, 2, 6],
            [1, 6, 5], // +X
            [3, 0, 4],
            [3, 4, 7], // -X
        ] {
            self.parts[0]
                .triangles
                .push(face.map(|corner| base + corner));
        }
        self
    }

    /// Removes one face, which leaves three boundary edges: a hole.
    pub fn without_a_face(mut self) -> Self {
        self.parts[0].triangles.remove(0);
        self
    }

    /// Adds a 5 mm cube **inside** the figure, which is its own island and
    /// changes nothing else: it is debris the rigger would be paid to skin.
    pub fn plus_debris(self) -> Self {
        self.plus_box(
            DVec3::new(0.0, HEIGHT_METERS / 2.0, 0.05),
            DVec3::splat(0.005),
            0.0,
        )
    }

    /// Adds a second box that sits inside the first, so their faces cross.
    pub fn plus_an_overlapping_box(self) -> Self {
        self.plus_box(
            DVec3::new(0.0, HEIGHT_METERS / 2.0, 0.0),
            DVec3::new(0.40, 0.30, 0.30),
            0.0,
        )
    }

    /// Adds one more face on an edge that already has two, which is what
    /// makes weight painting unable to tell which bone a vertex belongs to.
    pub fn with_three_faces_on_one_edge(mut self) -> Self {
        let flap = self.parts[0].vertices.len() as u32;
        let along = self.parts[0].vertices[0];
        self.parts[0]
            .vertices
            .push(along + DVec3::new(0.0, 0.0, 0.5));
        self.parts[0].uvs.push(DVec2::new(0.5, 0.5));
        // Vertices 0 and 1 are two corners of the bottom face, so the edge
        // between them already carries the bottom and the -Z side.
        self.parts[0].triangles.push([0, 1, flap]);
        self
    }

    /// Pushes one side out, so the mesh stops being its own reflection.
    pub fn lopsided(mut self, by: f64) -> Self {
        for vertex in &mut self.parts[0].vertices {
            if vertex.x > 0.0 {
                vertex.x += by;
            }
        }
        self
    }

    /// Turns the whole mesh from the node above it. A yaw of 180 degrees
    /// faces it away.
    pub fn turned(mut self, rotation: DQuat) -> Self {
        self.parts[0].node = DMat4::from_quat(rotation) * self.parts[0].node;
        self
    }

    /// Writes the very same vertex data under an identity node.
    ///
    /// The vertex coordinates are 100x, as this asset family stores them, so
    /// the node's 0.01 scale is what brings them back to meters. Take the
    /// scale out of the file and the same geometry reads 170 m: one fixture,
    /// two node transforms, one accepted and one refused.
    pub fn without_the_node_scale(mut self) -> Self {
        self.node_scale_removed = true;
        self
    }

    /// Scales the whole figure to `meters` tall, the way a generator hands
    /// back a body of the size it chose rather than the size that was asked
    /// for: `height_meters` is a parameter of the rigging call.
    ///
    /// Uniform, so the mirror, the facing and every count read the same.
    pub fn at_height(mut self, meters: f64) -> Self {
        let factor = meters / HEIGHT_METERS;
        for vertex in &mut self.parts[0].vertices {
            *vertex *= factor;
        }
        self
    }

    /// Flattens the figure onto X = 0, so it has no width and a percent of
    /// that width is a precise number about nothing.
    pub fn flat_in_x(mut self) -> Self {
        for vertex in &mut self.parts[0].vertices {
            vertex.x = 0.0;
        }
        self
    }

    /// Lays `copies` of the whole object side by side along X, centered on
    /// X = 0, so the mesh stays its own reflection, keeps its height and
    /// keeps its facing while every count multiplies by `copies`.
    ///
    /// The published limits are calibrated on a real 55,000 triangle mesh,
    /// so this is how a synthetic negative reaches a count in the hundreds.
    pub fn repeated(mut self, copies: usize) -> Self {
        let part = &mut self.parts[0];
        let (vertices, uvs, triangles) = (
            part.vertices.clone(),
            part.uvs.clone(),
            part.triangles.clone(),
        );
        part.vertices.clear();
        part.uvs.clear();
        part.triangles.clear();
        // Wider than the widest fixture box, so no two copies touch and
        // weld into one island.
        let spacing = 0.50;
        for copy in 0..copies {
            let aside = (copy as f64 - (copies as f64 - 1.0) / 2.0) * spacing;
            let base = part.vertices.len() as u32;
            part.vertices
                .extend(vertices.iter().map(|at| *at + DVec3::new(aside, 0.0, 0.0)));
            part.uvs.extend(uvs.iter().copied());
            part.triangles.extend(
                triangles
                    .iter()
                    .map(|face| face.map(|corner| base + corner)),
            );
        }
        self
    }

    /// Removes the forward reach of the bottom face, so the lowest slice
    /// sits directly under the body and names no direction.
    pub fn without_feet(mut self) -> Self {
        for vertex in &mut self.parts[0].vertices {
            if vertex.y < HEIGHT_METERS / 2.0 {
                vertex.z -= FOOT_REACH;
            }
        }
        self
    }

    /// Adds a second mesh **object**, which the profile's `meshes` list does
    /// not name. Meshy's own debris arrives this way.
    pub fn plus_an_object(mut self, name: &str) -> Self {
        let mut stray = self.parts[0].clone();
        stray.name = name.to_owned();
        for vertex in &mut stray.vertices {
            *vertex = *vertex * 0.05 + DVec3::new(0.9, 0.9, 0.0);
        }
        self.parts.push(stray);
        self
    }

    /// Strips the material, which rigging refuses.
    pub fn untextured(mut self) -> Self {
        self.parts[0].textured = false;
        self
    }

    /// Moves one texture coordinate off the tile, where nothing samples.
    pub fn with_a_uv_at(mut self, value: f64) -> Self {
        self.parts[0].uvs[0].x = value;
        self
    }

    /// Declares the primitive as lines, so no rule below can read it.
    pub fn made_of_lines(mut self) -> Self {
        self.parts[0].mode = LINES;
        self
    }

    /// Replaces the geometry with a long strip of `triangles` faces, laid end
    /// to end so none of them touch. Only `mesh.budget` is about this.
    pub fn strip_of(mut self, triangles: usize) -> Self {
        let part = &mut self.parts[0];
        part.vertices.clear();
        part.uvs.clear();
        part.triangles.clear();
        let step = 0.001;
        for index in 0..=triangles + 1 {
            let along = index as f64 * step;
            part.vertices.push(DVec3::new(along, 0.0, 0.0));
            part.uvs.push(DVec2::new(0.5, 0.5));
        }
        for index in 0..triangles {
            let first = index as u32;
            part.triangles.push([first, first + 1, first + 2]);
        }
        self
    }

    /// Drops the texture coordinates, which a primitive is allowed not to
    /// have.
    pub fn without_texture_coordinates(mut self) -> Self {
        self.parts[0].uvs.clear();
        self
    }

    /// Leaves one index off the last triangle, so the primitive claims to be
    /// triangles and is not a whole number of them.
    pub fn with_a_half_triangle(mut self) -> Self {
        self.parts[0].half_triangle = true;
        self
    }

    /// Attaches the figure to a one-joint skin, and leaves the mesh node
    /// carrying an identity transform that the specification says must be
    /// ignored.
    ///
    /// The vertex data is 100x, as this asset family stores it. The skin
    /// brings it back: `joint world * inverse bind matrix` is the 0.01
    /// scale, split across both factors so neither can be dropped. Read the
    /// node chain instead and the figure measures 170 m.
    pub fn skinned(self) -> Self {
        self.skinned_but(Skin::Sound)
    }

    /// The same skin with one thing wrong with it.
    ///
    /// - [`Skin::UnweightedVertices`]: vertices with no place in the world.
    /// - [`Skin::JointOutsideTheScene`]: a joint with no world transform.
    /// - [`Skin::OneMatrixShort`]: two joints, one inverse bind matrix.
    ///   Filling the gap with identity would place every vertex on that
    ///   joint 100x out, because half this family's scale is in the matrix.
    /// - [`Skin::UnknownJoint`]: a joint index past the end of the list.
    pub fn skinned_but(mut self, wrong: Skin) -> Self {
        self.skin = Some(wrong);
        self
    }

    /// The mesh as a GLB: one JSON chunk, one binary chunk.
    pub fn to_glb(&self) -> Vec<u8> {
        let mut out = Written::default();
        // Half the 0.01 scale sits on the joint node and half in the inverse
        // bind matrix, so a reader that drops either factor reads the wrong
        // size.
        let half = DMat4::from_scale(DVec3::splat(OBJECT_SCALE.sqrt()));
        let mut skins = Vec::new();
        if let Some(skin) = self.skin {
            // The joints come first, so a mesh node's index is its part
            // index plus however many of them there are.
            for joint in 0..skin.joints() {
                out.nodes
                    .push(json!({ "name": format!("Joint{joint}"), "matrix": columns(half) }));
            }
            skins.push(json!({
                "name": "Armature",
                "joints": (0..skin.joints()).collect::<Vec<usize>>(),
                // One matrix, however many joints were declared.
                "inverseBindMatrices": push_mat4(&mut out.buffers, &[half]),
            }));
        }
        for part in &self.parts {
            let primitive = self.write_part(&mut out.buffers, part);
            out.nodes.push(json!({
                "name": part.name,
                "mesh": out.meshes.len(),
                "matrix": columns(match (&self.skin, self.node_scale_removed) {
                    // The specification says a skinned mesh's node transform
                    // is ignored, so this one is deliberately wrong: apply
                    // it on top of the skin and the figure reads 170 m.
                    (Some(_), _) => wrong_on_a_skinned_node(),
                    (None, true) => DMat4::IDENTITY,
                    (None, false) => part.node,
                }),
                "skin": self.skin.as_ref().map(|_| 0),
            }));
            out.meshes.push(json!({
                "name": part.name,
                "primitives": [primitive],
            }));
        }
        let document = json!({
            "asset": { "version": "2.0", "generator": "marrowfall test fixture" },
            "scene": 0,
            "scenes": [{ "nodes": self.scene_nodes() }],
            "nodes": out.nodes,
            "meshes": out.meshes,
            "skins": skins,
            // Two materials, so a fixture picks the textured one or the bare
            // one by index and the file always validates.
            "materials": [
                { "pbrMetallicRoughness": { "baseColorTexture": { "index": 0 } } },
                { "name": "no base color" },
            ],
            "textures": [{ "source": 0 }],
            "images": [{ "uri": "skin.png" }],
            "accessors": out.buffers.accessors(),
            "bufferViews": out.buffers.views(),
            "buffers": [{ "byteLength": out.buffers.len() }],
        });
        out.buffers.wrap(&document)
    }

    /// One primitive, with its accessors appended to the binary chunk.
    fn write_part(&self, out: &mut Buffers, part: &Part) -> Value {
        // The file stores 100x coordinates, and the node or the skin above
        // brings them back to meters.
        let local: Vec<DVec3> = part
            .vertices
            .iter()
            .map(|vertex| *vertex / OBJECT_SCALE)
            .collect();
        let mut attributes = json!({ "POSITION": push_vec3(out, &local) });
        if !part.uvs.is_empty() {
            attributes["TEXCOORD_0"] = json!(push_vec2(out, &part.uvs));
        }
        if let Some(skin) = self.skin {
            attributes["JOINTS_0"] = json!(push_u16x4(out, local.len(), skin.joint_index()));
            let weights: Vec<[f32; 4]> = (0..local.len())
                .map(|vertex| [skin.weight_of(vertex), 0.0, 0.0, 0.0])
                .collect();
            attributes["WEIGHTS_0"] = json!(push_vec4(out, &weights));
        }
        let mut corners: Vec<u32> = part.triangles.iter().flatten().copied().collect();
        if part.half_triangle {
            corners.pop();
        }
        json!({
            "attributes": attributes,
            "indices": push_u32(out, &corners),
            "mode": part.mode,
            "material": usize::from(!part.textured),
        })
    }

    /// Which nodes the scene holds. A joint left out of it has no world
    /// transform, which the skin reader refuses.
    fn scene_nodes(&self) -> Vec<usize> {
        let above = self.skin.map_or(0, Skin::joints);
        let meshes: Vec<usize> = (above..above + self.parts.len()).collect();
        match self.skin {
            Some(skin) if skin.in_the_scene() => {
                [(0..above).collect::<Vec<usize>>(), meshes].concat()
            }
            _ => meshes,
        }
    }
}

/// A skin over the whole mesh, and what is wrong with it. Exactly one thing
/// at a time, so a failure names the rule rather than the fixture.
#[derive(Debug, Clone, Copy)]
pub enum Skin {
    /// Nothing. One joint, one matrix, every vertex weighted to it.
    Sound,
    /// This many leading vertices carry a total weight of zero.
    UnweightedVertices(usize),
    /// The joint node is not in the scene graph, so it has no world
    /// transform.
    JointOutsideTheScene,
    /// Two joints declared and one matrix written.
    OneMatrixShort,
    /// Every vertex names a joint the skin does not have.
    UnknownJoint,
}

impl Skin {
    pub fn joints(self) -> usize {
        1 + usize::from(matches!(self, Self::OneMatrixShort))
    }

    fn in_the_scene(self) -> bool {
        !matches!(self, Self::JointOutsideTheScene)
    }

    /// The joint every vertex names, which only [`Skin::UnknownJoint`]
    /// pushes past the end of the list.
    fn joint_index(self) -> u32 {
        u32::from(matches!(self, Self::UnknownJoint))
    }

    fn weight_of(self, vertex: usize) -> f32 {
        match self {
            Self::UnweightedVertices(count) => f32::from(vertex >= count),
            _ => 1.0,
        }
    }
}

/// The GLB under construction: the shared binary chunk, plus the two tables
/// only a mesh fixture fills.
#[derive(Default)]
struct Written {
    buffers: Buffers,
    nodes: Vec<Value>,
    meshes: Vec<Value>,
}

/// A scene with one node and no mesh at all, so the reader has something to
/// refuse. The committed `humanoid.glb` cannot serve: it carries a
/// one-triangle skin carrier.
pub fn a_scene_with_no_mesh() -> Vec<u8> {
    let document = json!({
        "asset": { "version": "2.0", "generator": "marrowfall test fixture" },
        "scene": 0,
        "scenes": [{ "nodes": [0] }],
        "nodes": [{ "name": "Empty" }],
    });
    glb(&document.to_string(), &[])
}

/// A mesh whose vertex data sits behind a URI rather than in the file, so
/// the reader has nothing to measure and says so.
pub fn a_mesh_whose_buffer_is_a_uri() -> Vec<u8> {
    let document = json!({
        "asset": { "version": "2.0", "generator": "marrowfall test fixture" },
        "scene": 0,
        "scenes": [{ "nodes": [0] }],
        "nodes": [{ "name": ALLOWED, "mesh": 0 }],
        "meshes": [{
            "name": ALLOWED,
            "primitives": [{ "attributes": { "POSITION": 0 }, "indices": 1 }],
        }],
        "accessors": [
            {
                "bufferView": 0, "componentType": FLOAT, "count": 3, "type": "VEC3",
                "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 1.0],
            },
            { "bufferView": 1, "componentType": UNSIGNED_INT, "count": 3, "type": "SCALAR" },
        ],
        "bufferViews": [
            { "buffer": 0, "byteOffset": 0, "byteLength": 36 },
            { "buffer": 0, "byteOffset": 36, "byteLength": 12 },
        ],
        "buffers": [{ "byteLength": 48, "uri": "somewhere_else.bin" }],
    });
    glb(&document.to_string(), &[])
}

/// Appends one accessor of `f32` triples and returns its index.
fn push_vec3(out: &mut Buffers, values: &[DVec3]) -> usize {
    let floats: Vec<f32> = values
        .iter()
        .flat_map(|value| [value.x as f32, value.y as f32, value.z as f32])
        .collect();
    // POSITION requires min and max, and the validator enforces it.
    let fold = |pick: fn(f32, f32) -> f32, part: usize| {
        floats
            .iter()
            .skip(part)
            .step_by(3)
            .copied()
            .fold(f32::NAN, pick)
    };
    let view = out.push_view(&floats);
    out.push_accessor(json!({
        "bufferView": view,
        "componentType": FLOAT,
        "count": values.len(),
        "type": "VEC3",
        "min": (0..3).map(|part| fold(f32::min, part)).collect::<Vec<f32>>(),
        "max": (0..3).map(|part| fold(f32::max, part)).collect::<Vec<f32>>(),
    }))
}

fn push_vec2(out: &mut Buffers, values: &[DVec2]) -> usize {
    let floats: Vec<f32> = values
        .iter()
        .flat_map(|value| [value.x as f32, value.y as f32])
        .collect();
    let view = out.push_view(&floats);
    out.push_accessor(json!({
        "bufferView": view,
        "componentType": FLOAT,
        "count": values.len(),
        "type": "VEC2",
    }))
}

/// The skin weights, four per vertex.
fn push_vec4(out: &mut Buffers, values: &[[f32; 4]]) -> usize {
    let floats: Vec<f32> = values.iter().flatten().copied().collect();
    let view = out.push_view(&floats);
    out.push_accessor(json!({
        "bufferView": view,
        "componentType": FLOAT,
        "count": values.len(),
        "type": "VEC4",
    }))
}

/// The skin's inverse bind matrices, sixteen floats each.
fn push_mat4(out: &mut Buffers, values: &[DMat4]) -> usize {
    let floats: Vec<f32> = values
        .iter()
        .flat_map(|matrix| matrix.to_cols_array().map(|part| part as f32))
        .collect();
    let view = out.push_view(&floats);
    out.push_accessor(json!({
        "bufferView": view,
        "componentType": FLOAT,
        "count": values.len(),
        "type": "MAT4",
    }))
}

/// One joint index per vertex, the same joint every time. `u16` pairs pack
/// four to eight bytes, so the chunk stays four-byte aligned, and the index
/// goes in the low half of the first pair.
fn push_u16x4(out: &mut Buffers, vertices: usize, joint: u32) -> usize {
    let view = out.push_view(&[joint, 0_u32].repeat(vertices));
    out.push_accessor(json!({
        "bufferView": view,
        "componentType": UNSIGNED_SHORT,
        "count": vertices,
        "type": "VEC4",
    }))
}

fn push_u32(out: &mut Buffers, values: &[u32]) -> usize {
    let view = out.push_view(values);
    out.push_accessor(json!({
        "bufferView": view,
        "componentType": UNSIGNED_INT,
        "count": values.len(),
        "type": "SCALAR",
    }))
}
