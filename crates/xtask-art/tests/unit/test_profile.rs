//! The skeleton profile, and everything it refuses.
//!
//! A profile is the only place a rig limit is published, so a nonsense one
//! must fail to load rather than make thirteen rules report precise, wrong
//! numbers.

use glam::DVec3;
use xtask_art::check::profile::{Axis, Profile, mirrored};
use xtask_art::library::HUMANOID;

use crate::support::repo_root;

/// A tiny valid profile, for one broken line at a time. Two sided bones, so
/// the mirror rules have something to pair.
const SMALL: &str = r#"
[profile]
bones = ["Root", "Tip", "LeftFin", "LeftFinTip", "RightFin", "RightFinTip"]
single_root = "Root"
meshes = ["body"]
up_axis = "z"
facing_axis_gltf = "+z"
child_axis = "y"
child_axis_tolerance_degrees = 2.0
height_tolerance_percent = 5.0
mirror_tolerance_percent = 1.0
mirror_tolerance_degrees = 1.0
max_bind_deviation_degrees = 75.0
humerus_below_horizontal = { target = 40.0, tolerance = 15.0 }

[profile.mesh]
holes = 200
non_manifold_edges = 10
islands = 8
self_intersections = 1000
mirror_percent = 3.5
triangles = 300000
printability_edges = 200

[profile.clip]
swing_degrees = 0.01
twist_degrees = 15.0
fps_grid_frames = 1e-4
root_travel_meters = 0.02
root_bob_meters = 0.15
floor_snap_meters = 0.005
stride_percent = 2.0
loop_degrees = 2.0
foot_plants = 1
foot_skate_meters = 0.025
foot_penetration_meters = 0.005

[profile.source]
travel_meters = 0.02

[profile.parents]
Tip = "Root"
LeftFin = "Root"
LeftFinTip = "LeftFin"
RightFin = "Root"
RightFinTip = "RightFin"

[profile.tails]
Root = "Tip"
LeftFin = "LeftFinTip"
RightFin = "RightFinTip"
"#;

/// The small profile with lines rewritten, which is how each refusal is
/// tested one at a time.
fn edited(edits: &[(&str, &str)]) -> anyhow::Result<Profile> {
    let mut text = SMALL.to_owned();
    for (from, to) in edits {
        assert!(text.contains(from), "{from:?} is not in the fixture");
        text = text.replace(from, to);
    }
    Profile::parse(&text)
}

fn refused(edits: &[(&str, &str)]) -> String {
    format!("{:#}", edited(edits).unwrap_err())
}

// --- the committed profile ------------------------------------------------

#[test]
fn the_committed_humanoid_profile_loads() {
    let profile = Profile::of(&repo_root(), HUMANOID).unwrap();

    assert_eq!(profile.bones.len(), 24, "24 bones, no fingers");
    assert_eq!(profile.single_root, "Hips");
    assert_eq!(profile.meshes, ["char1"]);
    assert_eq!(
        profile.up_axis.to_string(),
        "+z",
        "Blender, after the import"
    );
    assert_eq!(profile.facing_axis_gltf.to_string(), "+z", "glTF Y-up");
    assert_eq!(profile.child_axis.to_string(), "+y");
    assert_eq!(profile.child_axis_tolerance_degrees, 2.0);
    assert_eq!(profile.height_tolerance_percent, 5.0);
    assert_eq!(profile.mirror_tolerance_percent, 1.0);
    assert_eq!(profile.mirror_tolerance_degrees, 1.0);
    assert_eq!(profile.max_bind_deviation_degrees, 75.0);
    assert_eq!(profile.humerus_below_horizontal.target, 40.0);
    assert_eq!(profile.humerus_below_horizontal.tolerance, 15.0);
    // Provisional, calibrated on the rigged `model.glb`, because `bare.glb`
    // has never been downloaded. The measurement beside each is in the
    // design's limits table and in `test_mesh.rs`.
    assert_eq!(profile.mesh.holes, 200.0);
    assert_eq!(profile.mesh.non_manifold_edges, 10.0);
    assert_eq!(profile.mesh.islands, 8.0);
    assert_eq!(profile.mesh.self_intersections, 1000.0);
    assert_eq!(profile.mesh.mirror_percent, 3.5);
    assert_eq!(profile.mesh.triangles, 300_000.0);
    assert_eq!(profile.mesh.printability_edges, 200.0);
    assert_eq!(profile.clip.swing_degrees, 0.01);
    assert_eq!(profile.clip.twist_degrees, 15.0);
    assert_eq!(profile.clip.fps_grid_frames, 1e-4);
    assert_eq!(profile.clip.root_travel_meters, 0.02);
    assert_eq!(profile.clip.root_bob_meters, 0.15);
    assert_eq!(profile.clip.floor_snap_meters, 0.005);
    assert_eq!(profile.clip.stride_percent, 2.0);
    assert_eq!(profile.clip.loop_degrees, 2.0);
    assert_eq!(profile.clip.foot_plants, 1.0);
    assert_eq!(profile.clip.foot_skate_meters, 0.025);
    assert_eq!(profile.clip.foot_penetration_meters, 0.005);
    assert_eq!(profile.source.travel_meters, 0.02);
}

/// The names are Mixamo's and HumanIK's, spine numbered from the bottom, so
/// `Spine` is the lowest of the three rather than the highest.
#[test]
fn the_committed_profile_names_the_standard_bones() {
    let profile = Profile::of(&repo_root(), HUMANOID).unwrap();

    for bone in [
        "Hips",
        "Spine",
        "Spine1",
        "Spine2",
        "Neck",
        "Head",
        "LeftShoulder",
        "LeftArm",
        "LeftForeArm",
        "LeftHand",
        "LeftUpLeg",
        "LeftLeg",
        "LeftFoot",
        "LeftToeBase",
    ] {
        assert!(profile.declares(bone), "{bone} is missing");
    }
    // The two bones that fill no role are still part of this rig.
    assert!(profile.declares("head_end") && profile.declares("headfront"));
    for old in ["Spine01", "Spine02", "neck"] {
        assert!(
            !profile.declares(old),
            "{old} is the auto-rigger's own name"
        );
    }
}

#[test]
fn the_committed_profile_holds_the_body_upright_from_the_root() {
    let profile = Profile::of(&repo_root(), HUMANOID).unwrap();

    assert_eq!(
        profile.root_chain(),
        [
            "Hips", "Spine", "Spine1", "Spine2", "Neck", "Head", "head_end"
        ],
    );
    assert_eq!(profile.tail("LeftFoot"), Some("LeftToeBase"));
    assert_eq!(profile.tail("LeftToeBase"), None, "a leaf has no tail");
}

#[test]
fn the_committed_profile_pairs_every_limb_segment() {
    let profile = Profile::of(&repo_root(), HUMANOID).unwrap();
    let mut segments: Vec<String> = profile
        .mirror_pairs()
        .into_iter()
        .map(|pair| pair.segment)
        .collect();
    segments.sort();

    assert_eq!(
        segments,
        ["Arm", "Foot", "ForeArm", "Leg", "Shoulder", "UpLeg"],
        "six segments a side, each with a left and a right"
    );
    let arm = profile
        .mirror_pairs()
        .into_iter()
        .find(|pair| pair.segment == "Arm")
        .unwrap();
    assert_eq!(arm.left, ("LeftArm".to_owned(), "LeftForeArm".to_owned()));
    assert_eq!(
        arm.right,
        ("RightArm".to_owned(), "RightForeArm".to_owned())
    );
}

/// The Blender side reads the role tables out of the same file, so a table
/// this reader ignores must not stop it loading.
#[test]
fn the_role_tables_beside_the_profile_are_another_readers_business() {
    let with_roles = format!(
        "canonical = \"meshy\"\n[conventions.meshy]\nhips = \"Root\"\n\
         [aim_table]\nhips = [90, 0, 0]\n{SMALL}"
    );

    assert!(Profile::parse(&with_roles).is_ok());
}

#[test]
fn a_file_with_no_profile_at_all_is_refused() {
    let error = format!(
        "{:#}",
        Profile::parse("canonical = \"meshy\"\n").unwrap_err()
    );
    assert!(error.contains("profile"), "got: {error}");
}

#[test]
fn an_unknown_field_inside_the_profile_is_refused() {
    let error = refused(&[("meshes = [\"body\"]", "meshes = [\"body\"]\nwaiver = true")]);
    assert!(error.contains("waiver"), "got: {error}");
}

#[test]
fn a_missing_profile_file_is_reported_by_path() {
    let error = format!("{:#}", Profile::of(&repo_root(), "quadruped").unwrap_err());
    assert!(
        error.contains("quadruped") && error.contains("art/skeletons"),
        "got: {error}"
    );
}

// --- the axis fields ------------------------------------------------------

#[test]
fn an_axis_reads_as_a_letter_with_an_optional_sign() {
    assert_eq!(Axis::parse("z").unwrap().vector(), DVec3::Z);
    assert_eq!(Axis::parse("+z").unwrap().vector(), DVec3::Z);
    assert_eq!(Axis::parse("-y").unwrap().vector(), -DVec3::Y);
    assert_eq!(Axis::parse("x").unwrap().to_string(), "+x");
    assert_eq!(Axis::parse("-x").unwrap().to_string(), "-x");
}

#[test]
fn anything_that_is_not_an_axis_is_refused() {
    for text in ["w", "", "zz", "+", "++z", "z+", " z"] {
        assert!(Axis::parse(text).is_err(), "{text:?} is not an axis");
    }
    let error = refused(&[("up_axis = \"z\"", "up_axis = \"w\"")]);
    assert!(error.contains("axis"), "got: {error}");
}

#[test]
fn a_direction_is_named_by_the_axis_it_points_closest_to() {
    for (direction, expected) in [
        (DVec3::new(0.0, 0.99, 0.1), "+y"),
        (DVec3::new(0.0, -0.99, 0.1), "-y"),
        (DVec3::new(0.9, 0.0, 0.4), "+x"),
        (DVec3::new(-0.9, 0.0, 0.4), "-x"),
        (DVec3::new(0.1, 0.2, 0.9), "+z"),
        (DVec3::new(0.1, 0.2, -0.9), "-z"),
    ] {
        assert_eq!(
            Axis::closest_to(direction.normalize()).to_string(),
            expected,
            "for {direction}"
        );
    }
}

/// The up axis is stated in Blender space and the facing in glTF space, so a
/// typo that makes the character face straight up leaves the facing rule no
/// horizontal direction to measure.
#[test]
fn a_facing_axis_that_is_the_up_axis_is_refused() {
    let error = refused(&[("facing_axis_gltf = \"+z\"", "facing_axis_gltf = \"+y\"")]);
    assert!(error.contains("horizontal"), "got: {error}");

    assert!(edited(&[("facing_axis_gltf = \"+z\"", "facing_axis_gltf = \"-x\"")]).is_ok());
}

#[test]
fn a_bone_has_at_most_one_mirror() {
    assert_eq!(mirrored("LeftArm").as_deref(), Some("RightArm"));
    assert_eq!(mirrored("RightToeBase").as_deref(), Some("LeftToeBase"));
    assert_eq!(mirrored("Hips"), None);
}

// --- what the loader refuses ----------------------------------------------

#[test]
fn a_profile_with_no_bones_is_refused() {
    let error = refused(&[(
        r#"bones = ["Root", "Tip", "LeftFin", "LeftFinTip", "RightFin", "RightFinTip"]"#,
        "bones = []",
    )]);
    assert!(error.contains("needs bones"), "got: {error}");

    let error = refused(&[(r#""Root", "Tip""#, r#""Root", " ", "Tip""#)]);
    assert!(error.contains("must not be empty"), "got: {error}");
}

#[test]
fn a_bone_named_twice_is_refused() {
    let error = refused(&[(r#""Root", "Tip""#, r#""Root", "Root", "Tip""#)]);
    assert!(error.contains("twice"), "got: {error}");
}

#[test]
fn a_root_that_is_not_a_bone_is_refused() {
    let error = refused(&[(r#"single_root = "Root""#, r#"single_root = "Pelvis""#)]);
    assert!(error.contains("single_root"), "got: {error}");
}

#[test]
fn a_hierarchy_row_naming_an_unknown_bone_is_refused() {
    for (from, to, word) in [
        ("Tip = \"Root\"", "Wing = \"Root\"", "parents"),
        ("Tip = \"Root\"", "Tip = \"Trunk\"", "parents.Tip"),
        ("Root = \"Tip\"", "Wing = \"Tip\"", "tails"),
        ("Root = \"Tip\"", "Root = \"Trunk\"", "tails.Root"),
    ] {
        let error = refused(&[(from, to)]);
        assert!(error.contains(word), "{to}: {error}");
    }
}

#[test]
fn a_bone_with_no_parent_is_refused() {
    let error = refused(&[("LeftFinTip = \"LeftFin\"\n", "")]);
    assert!(error.contains("has no parent"), "got: {error}");
}

#[test]
fn a_root_with_a_parent_is_refused() {
    let error = refused(&[("Tip = \"Root\"", "Tip = \"Root\"\nRoot = \"Tip\"")]);
    assert!(error.contains("must have no parent"), "got: {error}");
}

/// The chain walk is bounded rather than trusting `validate` two functions
/// away. The fields are public, so a caller can build a profile the loader
/// would have refused, and a walk that does not end is not a wrong answer:
/// it is no answer at all.
#[test]
fn the_root_chain_ends_even_when_the_tails_form_a_cycle() {
    let mut profile = Profile::parse(SMALL).unwrap();
    profile.tails.insert("Tip".to_owned(), "Root".to_owned());

    let chain = profile.root_chain();

    assert!(
        chain.len() <= profile.bones.len() + 1,
        "one step per bone at most, got {chain:?}"
    );
    assert_eq!(&chain[..3], ["Root", "Tip", "Root"]);
}

#[test]
fn a_cycle_in_the_hierarchy_is_refused() {
    let error = refused(&[(
        "LeftFin = \"Root\"\nLeftFinTip = \"LeftFin\"",
        "LeftFin = \"LeftFinTip\"\nLeftFinTip = \"LeftFin\"",
    )]);
    assert!(error.contains("cycle"), "got: {error}");
}

/// `rig.child_axis` measures toward the tail, so a tail that is not really a
/// child would make it measure toward a bone somewhere else entirely.
#[test]
fn a_tail_that_is_not_a_child_is_refused() {
    let error = refused(&[("Root = \"Tip\"", "Root = \"LeftFinTip\"")]);
    assert!(error.contains("not its child"), "got: {error}");
}

/// `rig.child_axis` iterates the tails, so a missing row would drop one bone
/// from that rule and from nothing else, which is the quietest way a gate
/// can stop measuring.
#[test]
fn a_bone_with_children_and_no_tails_row_is_refused() {
    let error = refused(&[("Root = \"Tip\"\n", "")]);
    assert!(
        error.contains("Root has children but no tails row"),
        "got: {error}"
    );

    // A leaf needs none: nothing hangs below it to point at.
    assert!(edited(&[("LeftFin = \"LeftFinTip\"\n", "")]).is_err());
    assert!(
        Profile::of(&repo_root(), HUMANOID)
            .unwrap()
            .tail("LeftToeBase")
            .is_none()
    );
}

#[test]
fn a_mesh_list_that_names_nothing_or_names_it_twice_is_refused() {
    for (broken, word) in [
        ("meshes = []", "meshes"),
        (r#"meshes = ["body", "body"]"#, "twice"),
        (r#"meshes = [" "]"#, "must not be empty"),
    ] {
        let error = refused(&[(r#"meshes = ["body"]"#, broken)]);
        assert!(error.contains(word), "{broken}: {error}");
    }
}

#[test]
fn a_sided_bone_with_no_mirror_is_refused() {
    let error = refused(&[
        (r#""RightFinTip"]"#, r#""RightFinTip", "LeftWing"]"#),
        (r#"Tip = "Root""#, "Tip = \"Root\"\nLeftWing = \"Root\""),
    ]);
    assert!(error.contains("mirror"), "got: {error}");
}

#[test]
fn a_limit_that_is_not_a_positive_number_is_refused() {
    for (field, declared) in [
        ("child_axis_tolerance_degrees", "2.0"),
        ("height_tolerance_percent", "5.0"),
        ("mirror_tolerance_percent", "1.0"),
        ("mirror_tolerance_degrees", "1.0"),
        ("max_bind_deviation_degrees", "75.0"),
    ] {
        for value in ["0.0", "-1.0", "nan"] {
            let error = refused(&[(
                &format!("{field} = {declared}"),
                &format!("{field} = {value}"),
            )]);
            assert!(error.contains(field), "{field} = {value}: {error}");
        }
    }
    let band = "humerus_below_horizontal = { target = 40.0, tolerance = 15.0 }";
    for (broken, field) in [
        ("{ target = 0.0, tolerance = 15.0 }", "target"),
        ("{ target = 40.0, tolerance = 0.0 }", "tolerance"),
    ] {
        let error = refused(&[(band, &format!("humerus_below_horizontal = {broken}"))]);
        assert!(error.contains(field), "{field}: {error}");
    }
}

/// And the clip tolerances. Each one is a calibrated band no correct clip
/// ever reaches exactly, so a zero would reject every one of them.
#[test]
fn a_clip_limit_that_is_not_a_positive_number_is_refused() {
    for (field, declared) in [
        ("swing_degrees", "0.01"),
        ("twist_degrees", "15.0"),
        ("fps_grid_frames", "1e-4"),
        ("root_travel_meters", "0.02"),
        ("root_bob_meters", "0.15"),
        ("floor_snap_meters", "0.005"),
        ("stride_percent", "2.0"),
        ("loop_degrees", "2.0"),
        ("foot_plants", "1"),
        ("foot_skate_meters", "0.025"),
        ("foot_penetration_meters", "0.005"),
    ] {
        for value in ["0.0", "-1.0", "nan"] {
            let error = refused(&[(
                &format!("{field} = {declared}"),
                &format!("{field} = {value}"),
            )]);
            assert!(error.contains(field), "clip.{field} = {value}: {error}");
        }
    }
}

/// The mesh ceilings go the same way. Zero islands or zero triangles
/// describes no mesh that can exist, so a zero is refused rather than left
/// to make every mesh rule fail on every asset.
#[test]
fn a_mesh_ceiling_that_is_not_a_positive_number_is_refused() {
    for (field, declared) in [
        ("holes", "200"),
        ("non_manifold_edges", "10"),
        ("islands", "8"),
        ("self_intersections", "1000"),
        ("mirror_percent", "3.5"),
        ("triangles", "300000"),
        ("printability_edges", "200"),
    ] {
        for value in ["0.0", "-1.0", "nan"] {
            let error = refused(&[(
                &format!("{field} = {declared}"),
                &format!("{field} = {value}"),
            )]);
            assert!(error.contains(field), "mesh.{field} = {value}: {error}");
        }
    }
}

/// The mesh gates run on the character while the profile is per skeleton, so
/// a profile with no mesh table would leave thirteen rules with no limits.
#[test]
fn a_profile_with_no_mesh_table_is_refused() {
    let error = refused(&[("[profile.mesh]", "[profile.unread]")]);

    assert!(error.contains("mesh"), "got: {error}");
}
