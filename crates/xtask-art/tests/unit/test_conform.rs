//! The rename and the conform steps, on the real committed art.
//!
//! Both are byte edits, so every test here reads a file this repository
//! ships, runs the step on it in memory, and measures the result with the
//! same `rig.*` rules `cargo art check` prints. The negative is the art that
//! shipped: `humanoid_before_rename.glb` is the rig under Meshy's own names
//! and with the joints Meshy's rigger placed, so renaming it is what the
//! conform is measured on.

use std::collections::{BTreeMap, BTreeSet};

use xtask_art::blender::Build;
use xtask_art::check::aim::{self, AimTable};
use xtask_art::check::gltf_world::{SHORTEST_SEGMENT_METERS, Skeleton};
use xtask_art::check::profile::Profile;
use xtask_art::check::{Artifacts, Finding, Severity, Symmetry, rig};
use xtask_art::conform::{conform, rename};
use xtask_art::glb::Glb;
use xtask_art::library::HUMANOID;

use crate::support::{committed_glb, repo_root};

/// The rig as Meshy named it, kept as the only negative the three name rules
/// have.
const PRE_RENAME: &str = "crates/xtask-art/tests/fixtures/humanoid_before_rename.glb";

/// The rigged, skinned survivor, which the conform has already been run on:
/// what a rest-frame edit must leave alone.
const COMMITTED: &str = "art/characters/survivor/model.glb";

/// The pre-rename fixture with its bones renamed, which is a vendor rig as
/// the conform receives one: twelve defects across four geometry rules.
fn an_unconformed_rig() -> Vec<u8> {
    rename(&read(PRE_RENAME), &table()).expect("the rename")
}

/// The height `spec.ron` declares for him, which `rig.world_height` reads
/// against.
const HEIGHT_METERS: f64 = 1.7;

/// What the rename closes, and what nothing but a fresh generation can.
const NAME_RULES: [&str; 3] = ["rig.names_standard", "rig.bone_set", "rig.parents"];

fn profile() -> Profile {
    Profile::of(&repo_root(), HUMANOID).expect("the committed profile")
}

/// The committed profile with another axis named as the one that must point
/// at a bone's child, so a conformed rig has every joint left to turn.
fn sideways_profile() -> Profile {
    let path = repo_root().join("art/skeletons").join("humanoid.toml");
    let text = std::fs::read_to_string(path).expect("the committed profile");
    let row = "child_axis = \"y\"";
    assert_eq!(text.matches(row).count(), 1, "one child_axis row to turn");
    Profile::parse(&text.replace(row, "child_axis = \"x\"")).expect("a valid profile")
}

fn table() -> AimTable {
    AimTable::of(&repo_root(), HUMANOID).expect("the committed aim table")
}

fn read(file: &str) -> Vec<u8> {
    std::fs::read(committed_glb(file)).expect("committed art")
}

/// Every `rig.*` finding on a rig in memory, read in the convention its own
/// fingerprint names, which is how the pipeline reads one.
fn findings(bytes: &[u8]) -> Vec<Finding> {
    let (profile, table) = (profile(), table());
    let skeleton = Skeleton::from_slice(bytes).expect("a skeleton");
    let names: Vec<String> = skeleton
        .joints()
        .iter()
        .map(|joint| joint.name.clone())
        .collect();
    let convention = table
        .convention_of(names.iter().map(String::as_str))
        .expect("a fingerprinted rig");
    [
        rig::check(
            "the rig",
            &skeleton,
            &profile,
            HEIGHT_METERS,
            Symmetry::Enforced,
            1,
        ),
        aim::check(&skeleton, &profile, &table, convention, 1).expect("a declared convention"),
    ]
    .concat()
}

/// Which rules a rig breaks, and how many findings each one leaves.
fn defects(bytes: &[u8]) -> BTreeMap<String, usize> {
    let mut counted: BTreeMap<String, usize> = BTreeMap::new();
    for finding in findings(bytes) {
        if finding.severity == Severity::Error {
            *counted.entry(finding.rule).or_default() += 1;
        }
    }
    counted
}

/// The buffer chunk, which every vertex, weight and UV lives in.
fn buffer(bytes: &[u8]) -> Vec<u8> {
    Glb::parse(bytes).expect("a GLB").binary().to_vec()
}

/// Where every joint sits, in world space.
fn positions(bytes: &[u8]) -> BTreeMap<String, glam::DVec3> {
    Skeleton::from_slice(bytes)
        .expect("a skeleton")
        .joints()
        .iter()
        .map(|joint| (joint.name.clone(), joint.position()))
        .collect()
}

/// Every joint name a file carries.
fn joints(bytes: &[u8]) -> BTreeSet<String> {
    Skeleton::from_slice(bytes)
        .expect("a skeleton")
        .joints()
        .iter()
        .map(|joint| joint.name.clone())
        .collect()
}

#[test]
fn the_two_committed_rigs_are_told_apart_by_their_fingerprints() {
    let table = table();
    for (file, convention) in [
        (PRE_RENAME, "meshy"),
        ("art/skeletons/humanoid.glb", "standard"),
    ] {
        let names = joints(&read(file));
        assert_eq!(
            table
                .convention_of(names.iter().map(String::as_str))
                .expect("a fingerprinted rig"),
            convention,
            "{file}"
        );
    }
}

#[test]
fn a_rig_no_convention_fingerprints_is_refused_rather_than_guessed() {
    let error = table()
        .convention_of(["Hips", "Spine"])
        .expect_err("nothing fingerprints two bones")
        .to_string();
    assert!(error.contains("no convention fingerprints"), "{error}");
}

#[test]
fn the_three_name_rules_reject_the_vendor_names_and_hold_after_the_rename() {
    let before = read(PRE_RENAME);
    let broken = defects(&before);
    for rule in NAME_RULES {
        assert!(broken.contains_key(rule), "{rule} holds before the rename");
    }

    let after = rename(&before, &table()).expect("the rename");
    let left = defects(&after);
    for rule in NAME_RULES {
        assert!(!left.contains_key(rule), "{rule} still breaks: {left:?}");
    }
}

#[test]
fn the_rename_puts_every_vendor_bone_under_its_canonical_name() {
    let after = rename(&read(PRE_RENAME), &table()).expect("the rename");
    assert_eq!(joints(&after), joints(&read("art/skeletons/humanoid.glb")));
}

#[test]
fn the_rename_leaves_the_buffer_chunk_byte_identical() {
    let before = read(PRE_RENAME);
    let after = rename(&before, &table()).expect("the rename");
    assert_eq!(
        buffer(&after),
        buffer(&before),
        "a rename is a JSON chunk edit, so no vertex may move"
    );
}

#[test]
fn renaming_a_rig_already_in_the_canonical_names_changes_nothing() {
    let before = read("art/skeletons/humanoid.glb");
    assert_eq!(rename(&before, &table()).expect("the rename"), before);
}

#[test]
fn the_conform_closes_every_geometry_defect_a_vendor_rig_arrives_with() {
    let before = an_unconformed_rig();
    assert_eq!(
        defects(&before),
        BTreeMap::from([
            ("rig.aim_table".to_owned(), 1),
            ("rig.child_axis".to_owned(), 3),
            ("rig.mirror_direction".to_owned(), 3),
            ("rig.mirror_length".to_owned(), 5),
        ]),
        "the rig Meshy returned is the negative, and it reads 12 defects"
    );

    let after = conform(&before, &profile(), Symmetry::Enforced).expect("the conform");
    assert_eq!(defects(&after), BTreeMap::new(), "every one of them closed");
}

/// A reflected average is exact in `f64` and the file stores `f32`, so what
/// is left is storage: 2.51e-5 percent on the committed rig, which is 39,000x
/// under the 1.0 percent the profile publishes.
#[test]
fn the_conform_leaves_the_mirror_rules_at_their_storage_floor() {
    let after =
        conform(&an_unconformed_rig(), &profile(), Symmetry::Enforced).expect("the conform");
    let worst = findings(&after)
        .into_iter()
        .filter(|finding| finding.rule.starts_with("rig.mirror_"))
        .map(|finding| finding.measured)
        .fold(0.0_f64, f64::max);
    assert!(worst < 1e-3, "worst mirror reading {worst}");
}

#[test]
fn the_conform_leaves_every_vertex_accessor_byte_identical() {
    let before = read(COMMITTED);
    let after = conform(&before, &profile(), Symmetry::Enforced).expect("the conform");
    assert_ne!(
        buffer(&after),
        buffer(&before),
        "the joint frames and the bind matrices moved, or nothing happened"
    );

    let (was, is) = (vertex_accessors(&before), vertex_accessors(&after));
    let names: Vec<&str> = was.iter().map(|(name, _)| name.as_str()).collect();
    // The five a skinned mesh carries, so a reader that stopped finding one
    // leaves a shorter list rather than a quiet pass.
    for semantic in ["Positions", "Normals", "TexCoords", "Joints", "Weights"] {
        assert!(
            names.iter().any(|name| name.contains(semantic)),
            "no {semantic} accessor among {names:?}"
        );
    }
    assert_eq!(
        is.iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<&str>>(),
        names,
        "the same accessors both times"
    );
    for ((name, was), (_, is)) in was.iter().zip(&is) {
        let moved = was.iter().zip(is).filter(|(was, is)| was != is).count();
        assert_eq!(moved, 0, "{name} moved {moved} of {} bytes", was.len());
    }
}

/// Each vertex accessor's own bytes, named by primitive and semantic. The
/// range the accessor addresses, not the whole buffer: that is what "no
/// vertex moved" is a claim about.
fn vertex_accessors(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let gltf = gltf::Gltf::from_slice(bytes).expect("a GLB");
    let blob = gltf.blob.as_deref().expect("a buffer chunk");
    let mut read = Vec::new();
    for (at, primitive) in gltf
        .document
        .meshes()
        .flat_map(|mesh| mesh.primitives())
        .enumerate()
    {
        for (semantic, accessor) in primitive.attributes() {
            let view = accessor.view().expect("a buffer view");
            let start = view.offset() + accessor.offset();
            let length = accessor.count() * accessor.size();
            read.push((
                format!("{at}.{semantic:?}"),
                blob[start..start + length].to_vec(),
            ));
        }
    }
    read.sort_by(|one, other| one.0.cmp(&other.0));
    read
}

/// Running it again moves nothing a rule can see. Not byte identical: the
/// second pass averages positions composed from `f32` locals, so it shifts
/// each joint by the storage noise and rounds it back. The worst reads 1.3e-8
/// m, which is `f32` on a 1.7 m body and 74x under the shortest segment
/// anything here calls a direction.
#[test]
fn conforming_a_conformed_rig_moves_nothing_a_rule_can_see() {
    let once = conform(&read(COMMITTED), &profile(), Symmetry::Enforced).expect("the conform");
    let twice = conform(&once, &profile(), Symmetry::Enforced).expect("the conform");
    assert_eq!(defects(&twice), defects(&once));
    let moved = positions(&twice);
    for (name, was) in positions(&once) {
        let step = (moved[&name] - was).length();
        assert!(
            step < SHORTEST_SEGMENT_METERS,
            "{name} moved {step} m on a second pass"
        );
    }
}

#[test]
fn a_character_that_declines_symmetry_keeps_its_own_joint_positions() {
    let before = an_unconformed_rig();
    let after = conform(&before, &profile(), Symmetry::Declined).expect("the conform");
    // And the mirror rules still read what they read, because nothing
    // averaged them.
    assert!(defects(&after).contains_key("rig.mirror_length"));
    let (before, after) = (positions(&before), positions(&after));
    for (name, was) in &before {
        assert!(
            (after[name] - *was).length() < 1e-6,
            "{name} moved with symmetry declined"
        );
    }
}

#[test]
fn a_conform_refuses_an_animation_that_carries_motion() {
    let mut glb = Glb::parse(&read(COMMITTED)).expect("a GLB");
    // One channel keyed twice, which is a pose over time and not a rest pose.
    let sampler = glb.document["animations"][0]["channels"][0]["sampler"]
        .as_u64()
        .expect("a sampler index") as usize;
    let input = glb.document["animations"][0]["samplers"][sampler]["input"]
        .as_u64()
        .expect("an input accessor") as usize;
    glb.document["accessors"][input]["count"] = serde_json::json!(2);
    let bytes = glb.to_bytes().expect("a GLB");

    let error = conform(&bytes, &profile(), Symmetry::Enforced)
        .expect_err("two keys are motion")
        .to_string();
    assert!(
        error.contains("motion belongs in art/animations/"),
        "{error}"
    );
}

/// A quaternion may be stored as normalized integers, which is legal glTF and
/// not the storage this rewrites, so it is refused by its own component type
/// rather than by a byte count that happens not to add up.
#[test]
fn a_key_accessor_that_is_not_f32_is_refused_by_its_component_type() {
    let mut glb = Glb::parse(&read(COMMITTED)).expect("a GLB");
    let sampler = rotation_sampler(&glb);
    let output = glb.document["animations"][0]["samplers"][sampler]["output"]
        .as_u64()
        .expect("an output accessor") as usize;
    // 5122 is SHORT, which a rotation output may be when it is normalized.
    glb.document["accessors"][output]["componentType"] = serde_json::json!(5122);
    glb.document["accessors"][output]["normalized"] = serde_json::json!(true);
    let bytes = glb.to_bytes().expect("a GLB");

    let error = format!(
        "{:#}",
        conform(&bytes, &profile(), Symmetry::Enforced).expect_err("not f32")
    );
    assert!(error.contains("I16"), "{error}");
}

/// The first sampler driving a rotation, which is the one channel a
/// quantized storage is legal on.
fn rotation_sampler(glb: &Glb) -> usize {
    glb.document["animations"][0]["channels"]
        .as_array()
        .expect("channels")
        .iter()
        .find(|channel| channel["target"]["path"] == "rotation")
        .and_then(|channel| channel["sampler"].as_u64())
        .expect("a rotation channel") as usize
}

/// What "the mesh does not move" is, as a number: the rest pose skinned
/// through the joints, before against after.
///
/// glTF skins a vertex through `world(joint) @ inverseBind(joint)`, and the
/// conform holds that product per joint, so the only thing left is the `f32`
/// a GLB stores. The joints themselves move millimeters on the same file, and
/// that ratio is what says the compensation is really there: a conform that
/// forgot to recompute an inverse bind matrix would drag the mesh the whole
/// way with them.
///
/// On the committed character, which is already conformed, driven by a
/// profile that asks for another bone axis: that gives every joint of a real
/// 27,761 vertex mesh a rest frame to turn, which is the work this measures.
#[test]
fn the_conform_moves_the_joints_and_leaves_every_vertex_where_it_was() {
    let before = read(COMMITTED);
    let after = conform(&before, &sideways_profile(), Symmetry::Enforced).expect("the conform");

    let turned = furthest_joint_turn(&before, &after);
    let vertices = furthest_rest_vertex(&before, &after);

    assert!(
        turned > 45.0,
        "the joints turned {turned} deg, so nothing happened"
    );
    assert!(
        vertices < SHORTEST_SEGMENT_METERS,
        "a rest vertex moved {vertices} m while its joints turned {turned} deg"
    );
}

/// How far the furthest joint's own rest frame turns between two rigs, in
/// degrees.
fn furthest_joint_turn(before: &[u8], after: &[u8]) -> f64 {
    let orientations = |bytes: &[u8]| -> BTreeMap<String, glam::DQuat> {
        Skeleton::from_slice(bytes)
            .expect("a skeleton")
            .joints()
            .iter()
            .map(|joint| {
                let (_, rotation, _) = joint.world.to_scale_rotation_translation();
                (joint.name.clone(), rotation)
            })
            .collect()
    };
    let (was, is) = (orientations(before), orientations(after));
    was.iter()
        .map(|(name, rotation)| is[name].angle_between(*rotation).to_degrees())
        .fold(0.0_f64, f64::max)
}

/// How far the furthest vertex of the rest pose moves between two rigs, in
/// meters. Both files carry the same vertex bytes, so this reads only what
/// the joints and their inverse bind matrices did to them.
fn furthest_rest_vertex(before: &[u8], after: &[u8]) -> f64 {
    let (was, is) = (rest_pose(before), rest_pose(after));
    assert_eq!(was.len(), is.len(), "the same mesh both times");
    was.iter()
        .zip(&is)
        .map(|(was, is)| (*is - *was).length())
        .fold(0.0_f64, f64::max)
}

/// Every vertex of one file, skinned at its own rest pose.
fn rest_pose(bytes: &[u8]) -> Vec<glam::DVec3> {
    let gltf = gltf::Gltf::from_slice(bytes).expect("a GLB");
    let document = &gltf.document;
    let blob = gltf.blob.as_deref().expect("a buffer chunk");
    let world: BTreeMap<usize, glam::DMat4> = xtask_art::check::gltf_world::world_nodes(document)
        .expect("a scene")
        .into_iter()
        .map(|entry| (entry.node.index(), entry.world))
        .collect();

    let skin = document.skins().next().expect("a skin");
    let bind = skin
        .reader(|_| Some(blob))
        .read_inverse_bind_matrices()
        .expect("the inverse bind matrices");
    let skinning: Vec<glam::DMat4> = skin
        .joints()
        .zip(bind)
        .map(|(joint, bind)| {
            let bind = glam::DMat4::from_cols_array(&std::array::from_fn(|cell| {
                f64::from(bind[cell / 4][cell % 4])
            }));
            world[&joint.index()] * bind
        })
        .collect();

    let mut placed = Vec::new();
    for primitive in document.meshes().flat_map(|mesh| mesh.primitives()) {
        let reader = primitive.reader(|_| Some(blob));
        let points = reader.read_positions().expect("positions");
        let joints = reader.read_joints(0).expect("joints").into_u16();
        let weights = reader.read_weights(0).expect("weights").into_f32();
        for ((point, joints), weights) in points.zip(joints).zip(weights) {
            let point = glam::DVec3::new(
                f64::from(point[0]),
                f64::from(point[1]),
                f64::from(point[2]),
            );
            placed.push(
                joints
                    .iter()
                    .zip(weights)
                    .map(|(joint, weight)| {
                        skinning[usize::from(*joint)].transform_point3(point) * f64::from(weight)
                    })
                    .sum(),
            );
        }
    }
    placed
}

/// How far two renders of the same rest pose may sit apart, as a share of
/// their components and as levels of 255.
///
/// Not zero, and the test above says why: a GLB stores `f32`, so the
/// recomputed inverse bind matrices leave the furthest vertex 1.57e-7 m from
/// where it was, and a silhouette pixel that sat on a quantization boundary
/// moves one step. Measured on the five views of the committed rig: 103 of
/// 1,310,720 components differ, 0.00786 percent, and the worst by 3. These
/// sit 6.4x and 2.7x over those, and a joint moved without its inverse bind
/// matrix would move the whole silhouette by a pixel of the 6.6 mm one covers.
const RENDER_DRIFT_PERCENT: f64 = 0.05;
const RENDER_DRIFT_LEVELS: u8 = 8;

/// The proof that nothing moved, in pixels rather than in algebra: Blender's
/// own evaluator reads the file and draws the body.
///
/// Blender-marked: a machine without one prints why and passes, which is what
/// lets CI run this tier on an architecture Blender has no build for.
#[test]
fn the_conform_renders_the_rest_pose_to_the_same_pixels() {
    let root = repo_root();
    if Build::detected().read().is_err() {
        println!("no Blender here, so the render proof did not run");
        return;
    }
    let before = read(COMMITTED);
    let after = conform(&before, &profile(), Symmetry::Enforced).expect("the conform");

    // Nothing of this lands in the tree: the repo root is read for the
    // scripts and the virtualenv, and everything written goes here.
    let at = tempfile::tempdir().expect("a tempdir");
    let mut drawn = Vec::new();
    for (name, bytes) in [("before", &before), ("after", &after)] {
        let glb = at.path().join(format!("{name}.glb"));
        std::fs::write(&glb, bytes).expect("writing the rig");
        drawn.push(render(&root, at.path(), &glb, &at.path().join(name)));
    }

    let [before, after] = drawn.try_into().expect("two renders");
    assert!(!before.is_empty(), "the sheet script drew nothing");
    for (view, pixels) in &before {
        let other = after.get(view).expect("the same views both times");
        assert_eq!(pixels.len(), other.len(), "{view} changed size");
        // Counted, never printed: a mismatch of two 256 px buffers is
        // 262,144 numbers nobody can read.
        let apart: Vec<u8> = pixels
            .iter()
            .zip(other)
            .map(|(before, after)| before.abs_diff(*after))
            .filter(|step| *step > 0)
            .collect();
        let share = apart.len() as f64 / pixels.len() as f64 * 100.0;
        let worst = apart.iter().copied().max().unwrap_or_default();
        assert!(
            share <= RENDER_DRIFT_PERCENT && worst <= RENDER_DRIFT_LEVELS,
            "{view} moved {share:.5} percent of its components, worst {worst} \
             of 255, so the conform moved the body rather than the joints"
        );
    }
}

/// One rig through the contact-sheet script, and the PNGs it wrote. The
/// report, the log and the sentinel go under `at` rather than in the tree.
fn render(
    root: &std::path::Path,
    at: &std::path::Path,
    glb: &std::path::Path,
    out: &std::path::Path,
) -> Views {
    let script = root.join("tools/blender/src/mesh_sheet.py");
    let name = out
        .file_name()
        .and_then(|name| name.to_str())
        .expect("a name");
    let artifacts = Artifacts::new(at, "conform", name, 1).expect("an artifact name");
    let args = vec![
        std::ffi::OsString::from("--glb"),
        glb.into(),
        std::ffi::OsString::from("--out-dir"),
        out.into(),
        std::ffi::OsString::from("--size"),
        std::ffi::OsString::from("256"),
    ];
    let findings = xtask_art::blender::run(&script, &args, &artifacts, root).expect("the render");
    assert!(findings.is_none(), "the sheet script measures nothing");

    std::fs::read_dir(out)
        .expect("the views")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|kind| kind == "png"))
        .map(|path| {
            let view = path.file_name().expect("a file name").to_string_lossy();
            // The pixels, not the file: a PNG carries the date it was
            // written, so two identical renders are never identical bytes.
            let pixels = image::open(&path).expect("a PNG").to_rgba8().into_raw();
            (view.into_owned(), pixels)
        })
        .collect()
}

/// One render's decoded views, by name.
type Views = BTreeMap<String, Vec<u8>>;
