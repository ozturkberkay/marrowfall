//! World transforms out of the glTF node graph.
//!
//! The expected numbers here were worked out by hand, once, and written as
//! numbers. Nothing in this file compares the reader against a second run of
//! the same maths, because that is how a wrong transform ships green.

use glam::{DQuat, DVec3};
use xtask_art::check::gltf_world::{
    SHORTEST_SEGMENT_METERS, Skeleton, blender_to_gltf, gltf_to_blender, gltf_to_blender_rotation,
};
use xtask_art::check::profile::Axis;

use crate::rigs::{OBJECT_SCALE, SyntheticRig};
use crate::support::{committed_glb, repo_root};

/// A nested rig with a 0.01 object scale, one node in the middle that is not
/// a joint, and one 90 degree turn about Z.
///
/// Worked out by hand, from `world = parent * translation * rotation`:
///
/// | joint | world position | world +Y |
/// | ----- | -------------- | -------- |
/// | `A`   | `(0, 1, 0)`      | `(0, 1, 0)` |
/// | `B`   | `(0, 1.5, 0)`    | `(-1, 0, 0)` |
/// | `C`   | `(-0.3, 1.5, 0)` | `(-1, 0, 0)` |
///
/// `C`'s own translation is `(0, 30, 0)`, so a reader that skips the chain
/// puts it 100x out and 30 m in the air.
const NESTED: &str = r#"{
  "asset": { "version": "2.0" },
  "scene": 0,
  "scenes": [{ "nodes": [0] }],
  "nodes": [
    { "name": "Armature", "scale": [0.01, 0.01, 0.01], "children": [1] },
    { "name": "A", "translation": [0, 100, 0], "children": [2] },
    { "name": "helper", "translation": [0, 50, 0], "children": [3] },
    { "name": "B", "rotation": [0, 0, 0.7071067811865476, 0.7071067811865476],
      "children": [4] },
    { "name": "C", "translation": [0, 30, 0] }
  ],
  "skins": [{ "joints": [1, 3, 4] }]
}"#;

fn nested() -> Skeleton {
    Skeleton::from_slice(NESTED.as_bytes()).unwrap()
}

/// glTF stores `f32`, so a hand-written quaternion arrives with about seven
/// digits. Everything below that is the file's own precision, not the
/// reader's.
const CLOSE_ENOUGH: f64 = 1e-6;

fn about(measured: DVec3, expected: [f64; 3]) {
    let expected = DVec3::from_array(expected);
    assert!(
        (measured - expected).length() < CLOSE_ENOUGH,
        "expected {expected}, measured {measured}"
    );
}

#[test]
fn a_joint_sits_where_the_whole_node_chain_puts_it() {
    let rig = nested();

    about(rig.get("A").unwrap().position(), [0.0, 1.0, 0.0]);
    about(rig.get("B").unwrap().position(), [0.0, 1.5, 0.0]);
    about(rig.get("C").unwrap().position(), [-0.3, 1.5, 0.0]);
}

/// The trap this module exists for: the same file read without its node
/// scale puts a 1.7 m character at 170 m.
#[test]
fn a_local_read_of_the_same_file_would_be_a_hundred_times_out() {
    let rig = nested();
    // What the file stores for `C`, which is what a local read returns.
    let stored = DVec3::new(0.0, 30.0, 0.0);
    let segment = (rig.get("C").unwrap().position() - rig.get("B").unwrap().position()).length();

    assert!(
        (segment - stored.length() * OBJECT_SCALE).abs() < CLOSE_ENOUGH,
        "the bone is 30 units in the file and 0.3 m in the world, measured {segment}"
    );
    assert!(segment < stored.length() / 50.0);
}

#[test]
fn a_joint_axis_is_the_turned_one_and_not_the_files_own() {
    let rig = nested();
    let y = Axis::parse("y").unwrap();

    about(rig.get("A").unwrap().axis(y).unwrap(), [0.0, 1.0, 0.0]);
    about(rig.get("B").unwrap().axis(y).unwrap(), [-1.0, 0.0, 0.0]);
    about(rig.get("C").unwrap().axis(y).unwrap(), [-1.0, 0.0, 0.0]);
    // The sign is part of the axis, so -y is the other way round.
    about(
        rig.get("B")
            .unwrap()
            .axis(Axis::parse("-y").unwrap())
            .unwrap(),
        [1.0, 0.0, 0.0],
    );
}

#[test]
fn a_node_that_is_no_joint_is_stepped_over_rather_than_read_as_a_bone() {
    let rig = nested();

    assert_eq!(
        rig.get("A").unwrap().parent,
        None,
        "the object is not a bone"
    );
    assert_eq!(rig.get("B").unwrap().parent.as_deref(), Some("A"));
    assert_eq!(rig.get("C").unwrap().parent.as_deref(), Some("B"));
    assert_eq!(rig.joints().len(), 3, "the helper is not a joint");
    assert!(rig.get("helper").is_none());
}

#[test]
fn the_direction_between_two_joints_is_a_unit_vector() {
    let rig = nested();

    about(rig.direction("A", "B").unwrap(), [0.0, 1.0, 0.0]);
    about(rig.direction("B", "C").unwrap(), [-1.0, 0.0, 0.0]);
    assert!(rig.direction("A", "A").is_none(), "no direction to itself");
    assert!(rig.direction("A", "nothing").is_none());
}

#[test]
fn the_two_space_conversions_are_each_others_inverse() {
    // A character faces +Z in glTF and minus Y in Blender, where the bake
    // camera sits, and stands up along +Y in glTF and +Z in Blender.
    about(gltf_to_blender(DVec3::Z), [0.0, -1.0, 0.0]);
    about(gltf_to_blender(DVec3::Y), [0.0, 0.0, 1.0]);
    about(blender_to_gltf(DVec3::Z), [0.0, 1.0, 0.0]);
    for v in [DVec3::X, DVec3::Y, DVec3::Z, DVec3::new(1.0, 2.0, 3.0)] {
        about(blender_to_gltf(gltf_to_blender(v)), v.to_array());
    }
}

/// The rotation conversion, pinned against the vector one above.
///
/// A bone keeps its own axis labels across the conversion, so wherever
/// `gltf_to_blender` sends a bone's +Y, the converted orientation must point
/// its +Y. Without this the clip gates could carry any conversion at all:
/// both sides of their fixture pass through this one function, and a wrong
/// one cancels there.
#[test]
fn the_rotation_conversion_moves_a_bone_s_own_axes_the_way_the_vector_one_does() {
    let turned = DQuat::from_rotation_z(0.7) * DQuat::from_rotation_x(-1.3);

    for axis in [DVec3::X, DVec3::Y, DVec3::Z] {
        about(
            gltf_to_blender_rotation(turned) * axis,
            gltf_to_blender(turned * axis).to_array(),
        );
    }
}

/// A quarter turn about +X, written out: `sin(45), 0, 0, cos(45)`. This is
/// the sign, and `Rx(-90)` would pass every clip test on its own.
#[test]
fn an_unturned_bone_converts_to_the_quarter_turn_itself() {
    let quarter = gltf_to_blender_rotation(DQuat::IDENTITY);

    let half = std::f64::consts::FRAC_1_SQRT_2;
    about(
        DVec3::new(quarter.x, quarter.y, quarter.z),
        [half, 0.0, 0.0],
    );
    assert!(
        (quarter.w - half).abs() < CLOSE_ENOUGH,
        "measured {quarter}"
    );
}

/// Two joints asked to sit at the same place land a fraction of a nanometer
/// apart, because the file stores `f32` and the exporter derives each local
/// transform through the inverse of a 0.01 scale. A direction taken from
/// that difference is cancellation noise, normalized into confidence.
#[test]
fn a_segment_of_numeric_noise_has_no_direction() {
    let rig = read(&SyntheticRig::conformant().coincident("LeftForeArm", "LeftArm"));
    let apart = (rig.get("LeftForeArm").unwrap().position()
        - rig.get("LeftArm").unwrap().position())
    .length();

    assert!(apart > 0.0, "the noise is real, and this test needs it");
    assert!(apart < SHORTEST_SEGMENT_METERS, "measured {apart} m apart");
    assert!(rig.direction("LeftArm", "LeftForeArm").is_none());
}

#[test]
fn a_joint_with_no_scale_has_no_axis_rather_than_a_nan() {
    let rig = read(&SyntheticRig::conformant().without_scale("LeftFoot"));

    assert!(
        rig.get("LeftFoot")
            .unwrap()
            .axis(Axis::parse("y").unwrap())
            .is_none(),
        "a zero axis must be reported, never normalized"
    );
}

#[test]
fn the_committed_rig_reads_at_the_size_the_spec_asks_for() {
    let rig = Skeleton::read(&committed_glb("art/skeletons/humanoid.glb")).unwrap();

    let up: Vec<f64> = rig
        .joints()
        .iter()
        .map(|joint| joint.position().y)
        .collect();
    let span =
        up.iter().copied().reduce(f64::max).unwrap() - up.iter().copied().reduce(f64::min).unwrap();

    assert_eq!(rig.joints().len(), 24, "24 bones, no fingers");
    assert!(
        (span - 1.665_165).abs() < 1e-4,
        "the joints span {span} m, and 1.665165 was measured by hand"
    );
    assert!(
        rig.object_channels().is_empty(),
        "the rig carries no action"
    );
}

// --- what the reader refuses ----------------------------------------------

fn refused(document: &str) -> String {
    format!(
        "{:#}",
        Skeleton::from_slice(document.as_bytes()).unwrap_err()
    )
}

#[test]
fn a_file_with_no_skin_holds_no_skeleton() {
    let error = refused(
        r#"{ "asset": { "version": "2.0" }, "scene": 0,
             "scenes": [{ "nodes": [0] }], "nodes": [{ "name": "A" }] }"#,
    );
    assert!(error.contains("no skin"), "got: {error}");
}

#[test]
fn a_file_with_no_scene_has_no_world_space_at_all() {
    let error = refused(
        r#"{ "asset": { "version": "2.0" }, "nodes": [{ "name": "A" }],
             "skins": [{ "joints": [0] }] }"#,
    );
    assert!(error.contains("no scene"), "got: {error}");
}

/// A joint outside the scene has no transform to compose, so measuring it
/// would mean inventing an identity for it.
#[test]
fn a_joint_that_is_not_in_the_scene_is_refused() {
    let error = refused(
        r#"{ "asset": { "version": "2.0" }, "scene": 0,
             "scenes": [{ "nodes": [0] }],
             "nodes": [{ "name": "A" }, { "name": "loose" }],
             "skins": [{ "joints": [0, 1] }] }"#,
    );
    assert!(error.contains("not in the scene"), "got: {error}");
}

#[test]
fn a_node_with_two_parents_is_refused_rather_than_measured_twice() {
    let error = refused(
        r#"{ "asset": { "version": "2.0" }, "scene": 0,
             "scenes": [{ "nodes": [0, 1] }],
             "nodes": [{ "name": "A", "children": [2] },
                       { "name": "B", "children": [2] }, { "name": "C" }],
             "skins": [{ "joints": [0, 1, 2] }] }"#,
    );
    assert!(error.contains("twice"), "got: {error}");
}

#[test]
fn an_unreadable_file_is_reported_by_path() {
    let error = format!(
        "{:#}",
        Skeleton::read(&repo_root().join("art/skeletons/humanoid.toml")).unwrap_err()
    );
    assert!(error.contains("humanoid.toml"), "got: {error}");
}

/// A node with no name still has to be nameable, or a finding cannot point
/// at it.
#[test]
fn a_nameless_node_is_named_by_its_index() {
    let rig = Skeleton::from_slice(
        br#"{ "asset": { "version": "2.0" }, "scene": 0,
             "scenes": [{ "nodes": [0] }], "nodes": [{}],
             "skins": [{ "joints": [0] }] }"#,
    )
    .unwrap();

    assert_eq!(rig.joints()[0].name, "node 0");
}

pub fn read(rig: &SyntheticRig) -> Skeleton {
    Skeleton::from_slice(rig.to_gltf().as_bytes()).expect("a fixture is valid glTF")
}
