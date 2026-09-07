//! The mesh representation: world space first, then welded.
//!
//! This is where the last audit went wrong, so these tests pin the
//! representation itself before any rule reads it.

use xtask_art::check::gltf_mesh::{self, HISTOGRAM_METERS, Surface, WELD_METERS};

use crate::meshes::{self, ALLOWED, HEIGHT_METERS, Skin, SyntheticMesh};
use crate::support::{committed_glb, repo_root};

/// The rigged survivor. `bare.glb`, the mesh before rigging, is not on this
/// machine, so it is the provisional calibration asset.
const CALIBRATION: &str = "art/characters/survivor/model.glb";

fn calibration_bytes() -> Vec<u8> {
    std::fs::read(committed_glb(CALIBRATION)).expect("the committed survivor")
}

fn read(fixture: &SyntheticMesh) -> Surface {
    Surface::from_slice(&fixture.to_glb()).expect("a readable fixture")
}

fn boundary_edges(surface: &Surface) -> usize {
    gltf_mesh::edge_use(surface)
        .values()
        .filter(|used| **used == 1)
        .count()
}

fn non_manifold_edges(surface: &Surface) -> usize {
    gltf_mesh::edge_use(surface)
        .values()
        .filter(|used| **used >= 3)
        .count()
}

// --- the weld, and why the distance is not assumed ------------------------

/// The measured histogram, which is what chose [`WELD_METERS`]. It is flat
/// from 1e-9 m to 1e-4 m and only moves at 1e-3 m, so 1e-5 sits four orders
/// inside the plateau at each end.
#[test]
fn the_weld_distance_sits_on_the_plateau_of_the_merge_histogram() {
    let bytes = calibration_bytes();

    let histogram = gltf_mesh::merge_histogram(&bytes).expect("the survivor");

    assert_eq!(
        histogram,
        [
            (1e-9, 27_761),
            (1e-6, 27_761),
            (1e-5, 27_761),
            (1e-4, 27_761),
            (1e-3, 27_740),
        ]
    );
    assert!(
        HISTOGRAM_METERS.contains(&WELD_METERS),
        "the published distance is one the histogram was read at"
    );
    // And it is not the edge of it: the plateau runs four orders each way.
    let (below, above) = (HISTOGRAM_METERS[1], HISTOGRAM_METERS[3]);
    assert_eq!((below, above), (1e-6, 1e-4));
    assert!(below < WELD_METERS && WELD_METERS < above);
}

/// The whole table the design publishes, cell by cell. A number in a
/// document that no test pins is a number nobody checked.
#[test]
fn the_published_histogram_table_is_measured_cell_by_cell() {
    let bytes = calibration_bytes();

    // (weld distance, vertices, boundary edges, non-manifold edges, islands)
    for (distance, vertices, boundary, non_manifold, islands) in [
        // Unwelded, every seam splits, so no edge is shared by three faces
        // and the surface falls into 444 pieces instead of 1.
        (None, 35_285, 14_285, 0, 444),
        (Some(1e-9), 27_761, 18, 17, 1),
        (Some(1e-6), 27_761, 18, 17, 1),
        (Some(1e-5), 27_761, 18, 17, 1),
        (Some(1e-4), 27_761, 18, 17, 1),
        (Some(1e-3), 27_740, 17, 57, 1),
        (Some(1e-2), 10_590, 1, 14_740, 1),
    ] {
        let surface = match distance {
            Some(distance) => Surface::from_slice_welded_at(&bytes, distance),
            None => Surface::unwelded(&bytes),
        }
        .expect("the survivor");
        let row = (
            surface.positions().len(),
            boundary_edges(&surface),
            non_manifold_edges(&surface),
            gltf_mesh::islands(&surface),
        );
        assert_eq!(
            row,
            (vertices, boundary, non_manifold, islands),
            "welded at {distance:?}"
        );
    }
}

/// The other end of the plateau. A weld ten times too wide hides 17 of the
/// 18 holes and invents 14,000 non-manifold edges out of merged geometry,
/// which is why the distance is chosen and not guessed.
#[test]
fn a_weld_two_orders_too_wide_hides_the_holes_it_should_report() {
    let bytes = calibration_bytes();
    let wide = Surface::from_slice_welded_at(&bytes, 1e-2).expect("the survivor");

    assert_eq!(wide.positions().len(), 10_590);
    assert_eq!(boundary_edges(&wide), 1);
    assert_eq!(non_manifold_edges(&wide), 14_740);
}

/// The representation the last audit measured, side by side with the right
/// one. 14,285 boundary edges against 18 is the whole reason the weld comes
/// first.
#[test]
fn skipping_the_weld_reads_fourteen_thousand_holes_where_there_are_eighteen() {
    let bytes = calibration_bytes();

    let welded = Surface::from_slice(&bytes).expect("the survivor");
    let raw = Surface::unwelded(&bytes).expect("the survivor");

    assert_eq!(welded.positions().len(), 27_761);
    assert_eq!(raw.positions().len(), 35_285);
    assert_eq!(boundary_edges(&welded), 18);
    assert_eq!(boundary_edges(&raw), 14_285);
    assert_eq!(welded.weld_meters(), Some(WELD_METERS));
    assert_eq!(raw.weld_meters(), None, "and it says it was not welded");
}

/// The weld is a merge and not a rounding. Two positions on opposite sides
/// of a grid boundary are still one vertex, which is what the 27-cell search
/// is for.
#[test]
fn two_positions_a_nanometer_apart_weld_into_one() {
    let closed = read(&SyntheticMesh::figure());
    let nudged = read(&SyntheticMesh::figure().lopsided(1e-9));

    assert_eq!(closed.positions().len(), 8, "a box has eight corners");
    assert_eq!(nudged.positions().len(), 8);
    assert_eq!(boundary_edges(&nudged), 0, "still a closed box");
}

/// And the other direction: a real feature is not welded away.
#[test]
fn two_positions_a_millimeter_apart_stay_two_vertices() {
    let apart = read(&SyntheticMesh::figure().lopsided(1e-3));

    assert_eq!(apart.positions().len(), 8);
    assert!(apart.positions().iter().any(|point| point.x > 0.15));
}

// --- world space ----------------------------------------------------------

/// The node chain is the transform. The same vertex data under a 0.01 scale
/// and under an identity node reads 1.70 m and 170 m.
#[test]
fn the_node_chain_is_the_transform_and_skipping_it_is_wrong_by_a_hundred() {
    let through = read(&SyntheticMesh::figure());
    let skipped = read(&SyntheticMesh::figure().without_the_node_scale());

    assert!((height_of(&through) - HEIGHT_METERS).abs() < 1e-6);
    assert!((height_of(&skipped) - HEIGHT_METERS * 100.0).abs() < 1e-4);
}

/// The glTF specification: the node transform of a **skinned** mesh must be
/// ignored, because the skin's own joints carry it. The survivor is skinned
/// under a 0.01 node, so a reader that composes the node chain anyway
/// measures a 1.70 m character at 0.017 m.
#[test]
fn a_skinned_mesh_is_placed_by_its_joints_and_not_by_its_node() {
    let survivor = Surface::read(&committed_glb(CALIBRATION)).expect("the survivor");

    let height = height_of(&survivor);
    assert!(
        (height - HEIGHT_METERS).abs() < 1e-5,
        "measured {height} m, and the spec asks for {HEIGHT_METERS} m"
    );
}

fn height_of(surface: &Surface) -> f64 {
    let along: Vec<f64> = surface.positions().iter().map(|point| point.y).collect();
    along.iter().copied().fold(f64::MIN, f64::max) - along.iter().copied().fold(f64::MAX, f64::min)
}

// --- topology -------------------------------------------------------------

#[test]
fn a_closed_box_has_no_boundary_edge_and_one_island() {
    let surface = read(&SyntheticMesh::figure());

    assert_eq!(surface.triangles().len(), 12);
    assert_eq!(boundary_edges(&surface), 0);
    assert_eq!(non_manifold_edges(&surface), 0);
    assert_eq!(gltf_mesh::islands(&surface), 1);
}

#[test]
fn a_box_missing_one_face_has_three_boundary_edges() {
    let surface = read(&SyntheticMesh::figure().without_a_face());

    assert_eq!(boundary_edges(&surface), 3);
    assert_eq!(gltf_mesh::islands(&surface), 1, "still one piece");
}

#[test]
fn debris_inside_the_mesh_is_its_own_island() {
    let surface = read(&SyntheticMesh::figure().plus_debris());

    assert_eq!(gltf_mesh::islands(&surface), 2);
    assert_eq!(boundary_edges(&surface), 0, "both boxes are closed");
    assert_eq!(surface.objects()[0].triangles, 24);
}

#[test]
fn a_third_face_on_one_edge_is_non_manifold() {
    let surface = read(&SyntheticMesh::figure().with_three_faces_on_one_edge());

    assert_eq!(non_manifold_edges(&surface), 1);
}

// --- what the reader refuses ---------------------------------------------

#[test]
fn a_file_with_no_mesh_is_refused_rather_than_measured_as_empty() {
    let error = Surface::from_slice(&meshes::a_scene_with_no_mesh())
        .expect_err("a scene with no mesh node");

    assert!(format!("{error:#}").contains("no mesh"), "{error:#}");
}

#[test]
fn a_file_that_is_not_gltf_at_all_is_refused() {
    let error = Surface::read(&repo_root().join("art/skeletons/humanoid.toml"))
        .expect_err("a TOML file is not a GLB");

    assert!(format!("{error:#}").contains("humanoid.toml"), "{error:#}");
}

#[test]
fn a_weld_distance_of_zero_or_less_is_refused() {
    for distance in [0.0, -1e-5] {
        let error = Surface::from_slice_welded_at(&SyntheticMesh::figure().to_glb(), distance)
            .expect_err("a weld distance is positive");

        assert!(
            format!("{error:#}").contains("must be positive"),
            "{error:#}"
        );
    }
}

/// A primitive of lines carries no triangle, so every topology rule would
/// measure nothing. The reader counts it rather than dropping it silently.
#[test]
fn a_primitive_that_is_not_triangles_is_counted_and_not_read() {
    let surface = read(&SyntheticMesh::figure().made_of_lines());

    assert_eq!(surface.triangles().len(), 0);
    assert_eq!(surface.objects()[0].unreadable_primitives, 1);
    assert_eq!(surface.objects()[0].primitives, 1);
}

// --- per object -----------------------------------------------------------

#[test]
fn every_mesh_object_is_named_the_way_the_file_names_it() {
    let surface = read(&SyntheticMesh::figure().plus_an_object("Icosphere"));

    let names: Vec<&str> = surface
        .objects()
        .iter()
        .map(|object| object.name.as_str())
        .collect();
    assert_eq!(names, [ALLOWED, "Icosphere"]);
}

#[test]
fn a_missing_base_color_image_is_counted_per_primitive() {
    assert_eq!(
        read(&SyntheticMesh::figure()).objects()[0].untextured_primitives,
        0
    );
    assert_eq!(
        read(&SyntheticMesh::figure().untextured()).objects()[0].untextured_primitives,
        1
    );
}

/// Texture coordinates are counted **as delivered**, before any weld. A seam
/// is one position carrying two coordinates, so welding them would lose one.
#[test]
fn texture_coordinates_outside_the_tile_are_counted_before_the_weld() {
    assert_eq!(
        read(&SyntheticMesh::figure()).objects()[0].uvs_outside_the_tile,
        0
    );
    for value in [1.4, -0.2, f64::NAN] {
        let surface = read(&SyntheticMesh::figure().with_a_uv_at(value));
        assert_eq!(
            surface.objects()[0].uvs_outside_the_tile,
            1,
            "a coordinate at {value} is off the tile"
        );
    }
}

/// The glTF specification allows a primitive with no texture coordinates, so
/// there is nothing to count there and nothing to refuse.
#[test]
fn a_primitive_with_no_texture_coordinates_counts_none() {
    let surface = read(&SyntheticMesh::figure().without_texture_coordinates());

    assert_eq!(surface.objects()[0].uvs_outside_the_tile, 0);
    assert_eq!(surface.triangles().len(), 12);
}

#[test]
fn an_index_count_that_is_not_a_whole_number_of_triangles_is_refused() {
    let error = Surface::from_slice(&SyntheticMesh::figure().with_a_half_triangle().to_glb())
        .expect_err("35 indices is not a whole number of triangles");

    assert!(
        format!("{error:#}").contains("whole number of triangles"),
        "{error:#}"
    );
}

/// A GLB carries its vertex data inline. A buffer behind a URI would need a
/// fetch, and measuring the part that did load is how a gate reports a
/// precise number about half a mesh.
#[test]
fn a_buffer_the_file_does_not_carry_is_refused() {
    let error = Surface::from_slice(&meshes::a_mesh_whose_buffer_is_a_uri())
        .expect_err("the vertex data is not in the file");

    assert!(
        format!("{error:#}").contains("vertex positions are not in the file"),
        "{error:#}"
    );
}

// --- the skin, synthetically ---------------------------------------------

/// The same proof as on the survivor, on a fixture. The skin's joint and its
/// inverse bind matrix carry half the 0.01 scale each, and the mesh node
/// carries a 100x scale that the specification says is ignored. Read the
/// skin and the figure is 1.70 m. Apply the node on top and it is 170 m.
/// Read the node instead and it is 17,000 m.
#[test]
fn a_skinned_fixture_is_placed_by_its_skin_and_not_by_its_node() {
    let skinned = read(&SyntheticMesh::figure().skinned());
    let unskinned = read(&SyntheticMesh::figure());

    assert!(
        (height_of(&skinned) - HEIGHT_METERS).abs() < 1e-4,
        "measured {} m",
        height_of(&skinned)
    );
    assert!((height_of(&unskinned) - HEIGHT_METERS).abs() < 1e-4);
}

/// A vertex whose weights total zero is skinned to nothing, so it has no
/// place in the world. Refused, rather than collapsed onto the origin.
#[test]
fn a_vertex_skinned_to_nothing_is_refused() {
    let error = Surface::from_slice(
        &SyntheticMesh::figure()
            .skinned_but(Skin::UnweightedVertices(1))
            .to_glb(),
    )
    .expect_err("a vertex with no weight has no place");

    assert!(
        format!("{error:#}").contains("skinned to nothing"),
        "{error:#}"
    );
}

/// Fewer inverse bind matrices than joints is malformed input, not the
/// absent case the specification allows. Filling the gap with identity would
/// place every vertex on that joint 100x out, because half this family's
/// scale lives in the matrix, and it would say nothing.
#[test]
fn a_skin_one_inverse_bind_matrix_short_is_refused() {
    let error = Surface::from_slice(
        &SyntheticMesh::figure()
            .skinned_but(Skin::OneMatrixShort)
            .to_glb(),
    )
    .expect_err("two joints and one matrix");

    assert!(
        format!("{error:#}").contains("1 inverse bind matrices for 2 joints"),
        "{error:#}"
    );
}

/// A vertex naming a joint the skin does not have is a different fault from
/// a vertex with no weight, and it says so.
#[test]
fn a_vertex_naming_a_joint_the_skin_does_not_have_is_refused() {
    let error = Surface::from_slice(
        &SyntheticMesh::figure()
            .skinned_but(Skin::UnknownJoint)
            .to_glb(),
    )
    .expect_err("joint 1 of a one-joint skin");

    assert!(
        format!("{error:#}").contains("names joint 1, which the skin does not have"),
        "{error:#}"
    );
}

/// A joint outside the scene graph has no world transform, so the skin
/// cannot place anything through it.
#[test]
fn a_skin_whose_joint_is_not_in_the_scene_is_refused() {
    let error = Surface::from_slice(
        &SyntheticMesh::figure()
            .skinned_but(Skin::JointOutsideTheScene)
            .to_glb(),
    )
    .expect_err("the joint has no world transform");

    assert!(
        format!("{error:#}").contains("not in the scene"),
        "{error:#}"
    );
}

// --- determinism ----------------------------------------------------------

#[test]
fn the_same_bytes_read_the_same_surface_twice() {
    let bytes = calibration_bytes();
    let once = Surface::from_slice(&bytes).expect("the survivor");
    let twice = Surface::from_slice(&bytes).expect("the survivor");

    assert_eq!(once.positions(), twice.positions());
    assert_eq!(once.triangles(), twice.triangles());
    assert_eq!(once.objects(), twice.objects());
}
