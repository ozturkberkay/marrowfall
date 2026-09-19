//! The aim table, and the one rule that reads a rig against it.
//!
//! The table is the single source of every bone's constant offset in the
//! retarget, so a wrong row produces a confidently wrong clip and nothing
//! downstream can tell. Half of this file is therefore negatives: a missing
//! row, an extra row, a broken mirror pair, a row that is no direction, and
//! a rig whose rest pose the table does not describe.

use std::collections::{BTreeSet, HashSet};

use glam::{DQuat, DVec3};
use xtask_art::check::aim::{self, AimTable, mirrored_role};
use xtask_art::check::gltf_world::Skeleton;
use xtask_art::check::profile::Profile;
use xtask_art::check::{Finding, Rule, Severity, Symmetry, rig};
use xtask_art::library::HUMANOID;

use crate::rigs::{HEIGHT_METERS, SyntheticRig};
use crate::support::{committed_glb, repo_root};

/// The shared skeleton, and the survivor rigged onto it. The same 24 bones,
/// so the same row fails on both.
const COMMITTED: [&str; 2] = [
    "art/skeletons/humanoid.glb",
    "art/characters/survivor/model.glb",
];

/// The file every skeleton table lives in, `[profile]` included.
const SKELETON_FILE: &str = "art/skeletons/humanoid.toml";

/// The same rig under Meshy's own names, which is what a fresh rig arrives
/// as. Its spine is numbered from the top and its neck is lowercase.
const PRE_RENAME: &str = "crates/xtask-art/tests/fixtures/humanoid_before_rename.glb";

/// How many roles the humanoid table aims, which is how many rows one rig
/// owes.
const ROLES: usize = 22;

/// A tiny valid skeleton file, for one broken line at a time. One sided pair,
/// so the mirror rows have something to reflect.
const SMALL: &str = r#"
canonical = "ours"
ground_roles = ["left_fin", "right_fin"]
stride_segment = ["hips", "left_fin"]

[conventions.ours]
hips = "Hips"
left_fin = "LeftFin"
right_fin = "RightFin"

[landmarks.ours]
skull_top = "head_end"

[fingerprints]
ours = ["LeftFin"]

[aim_table]
hips = [0.0, 0.0, 1.0]
left_fin = [1.0, 0.0, -1.0]
right_fin = [-1.0, 0.0, -1.0]
"#;

/// The small file with lines rewritten, which is how each refusal is tested
/// one at a time.
fn edited(edits: &[(&str, &str)]) -> anyhow::Result<AimTable> {
    let mut text = SMALL.to_owned();
    for (from, to) in edits {
        assert!(text.contains(from), "{from:?} is not in the fixture");
        text = text.replace(from, to);
    }
    AimTable::parse(&text)
}

fn refused(edits: &[(&str, &str)]) -> String {
    format!("{:#}", edited(edits).unwrap_err())
}

fn profile() -> Profile {
    Profile::of(&repo_root(), HUMANOID).expect("the committed humanoid profile")
}

fn table() -> AimTable {
    AimTable::of(&repo_root(), HUMANOID).expect("the committed humanoid aim table")
}

/// Every finding for a committed rig, read in the convention it is named in.
fn findings_for(file: &str) -> Vec<Finding> {
    let table = table();
    aim::check_file(
        &committed_glb(file),
        &repo_root(),
        &profile(),
        &table,
        table.canonical(),
        1,
    )
    .expect("the aim rule runs without a Blender")
}

/// One rig this repository ships, as the gates read it.
fn read_file(file: &str) -> Skeleton {
    Skeleton::read(&committed_glb(file)).expect("committed art")
}

/// One hand-built rig, as the gates read it.
fn read(fixture: &SyntheticRig) -> Skeleton {
    Skeleton::from_slice(fixture.to_gltf().as_bytes()).expect("valid glTF")
}

/// Every finding for a hand-built rig, named in one convention.
fn findings_of(fixture: &SyntheticRig, convention: &str) -> Vec<Finding> {
    aim::check(&read(fixture), &profile(), &table(), convention, 1).expect("a declared convention")
}

fn errors(findings: &[Finding]) -> Vec<&Finding> {
    findings
        .iter()
        .filter(|finding| finding.severity == Severity::Error)
        .collect()
}

/// The subjects the rule reported an error on.
fn rejected(findings: &[Finding]) -> Vec<String> {
    errors(findings)
        .iter()
        .map(|finding| finding.subject.clone())
        .collect()
}

/// Which rules reported an error, with no duplicates.
fn broken(findings: &[Finding]) -> Vec<String> {
    let rules: BTreeSet<String> = errors(findings)
        .iter()
        .map(|finding| finding.rule.clone())
        .collect();
    rules.into_iter().collect()
}

/// The rule's measurement of one subject.
fn measured(findings: &[Finding], subject: &str) -> f64 {
    findings
        .iter()
        .find(|finding| finding.subject == subject)
        .unwrap_or_else(|| panic!("nothing measured {subject}"))
        .measured
}

// --- the committed table --------------------------------------------------

#[test]
fn the_committed_aim_table_loads() {
    let table = table();

    assert_eq!(table.canonical(), "standard");
    assert_eq!(table.roles().count(), 22, "22 roles, no fingers");
    // Blender Z-up, the character facing -Y, so +X is his left.
    assert_eq!(table.aim("hips"), Some(DVec3::Z));
    assert_eq!(table.aim("left_shoulder"), Some(DVec3::X));
    assert_eq!(table.aim("left_upper_leg"), Some(-DVec3::Z));
    assert_eq!(table.aim("left_toe"), Some(-DVec3::Y));
    assert_eq!(table.aim("neither"), None);
}

/// A direction, not a unit vector: `[1, 0, -1]` is the arm 45 degrees below
/// horizontal, and the reader is what makes it unit length.
#[test]
fn every_aim_is_a_unit_direction() {
    let table = table();

    for role in table.roles() {
        let aim = table.aim(role).unwrap();
        assert!((aim.length() - 1.0).abs() < 1e-12, "{role} aims {aim}");
    }
    let arm = table.aim("left_arm").unwrap();
    assert!((arm - DVec3::new(0.5_f64.sqrt(), 0.0, -0.5_f64.sqrt())).length() < 1e-12);
}

/// The reason the rows are whole numbers: a reflection has to be exact, and
/// a sign error on one row is invisible until a clip comes out wrong.
#[test]
fn every_left_row_is_the_exact_reflection_of_its_right_one() {
    let table = table();
    let mut pairs = 0;

    for role in table.roles() {
        let Some(other) = mirrored_role(role) else {
            continue;
        };
        if !role.starts_with("left_") {
            continue;
        }
        let (left, right) = (table.aim(role).unwrap(), table.aim(&other).unwrap());
        assert_eq!(left, DVec3::new(-right.x, right.y, right.z), "{role}");
        pairs += 1;
    }
    assert_eq!(pairs, 8, "eight sided roles a side");
}

#[test]
fn a_role_has_at_most_one_mirror() {
    assert_eq!(mirrored_role("left_arm").as_deref(), Some("right_arm"));
    assert_eq!(mirrored_role("right_toe").as_deref(), Some("left_toe"));
    assert_eq!(mirrored_role("hips"), None);
}

/// The table is per skeleton and the conventions are what map a role to a
/// bone, so every convention has to fill exactly the roles it aims.
#[test]
fn the_committed_table_maps_every_role_in_every_convention() {
    let table = table();
    let roles: BTreeSet<&str> = table.roles().collect();

    for convention in ["meshy", "mixamo", "standard"] {
        let bones = table.bones(convention).unwrap();
        assert_eq!(
            bones.keys().map(String::as_str).collect::<BTreeSet<&str>>(),
            roles,
            "{convention}"
        );
    }
    // The one row that says why three tables are three tables: Meshy
    // numbers its spine from the top, so its lowest spine bone is `Spine02`
    // and ours is `Spine`.
    assert_eq!(table.bones("meshy").unwrap()["spine_lower"], "Spine02");
    assert_eq!(table.bones("mixamo").unwrap()["spine_lower"], "Spine");
    assert_eq!(table.bones("standard").unwrap()["spine_lower"], "Spine");
}

/// `clip.floor_snap` puts the lowest of these on the floor, and which joints
/// stand on it is skeleton data: a quadruped has four of them.
#[test]
fn the_committed_table_names_both_toes_as_the_joints_that_stand_on_the_floor() {
    assert_eq!(table().ground_roles(), ["left_toe", "right_toe"]);
}

#[test]
fn a_ground_role_no_convention_maps_is_refused() {
    let error = refused(&[(
        r#"ground_roles = ["left_fin", "right_fin"]"#,
        r#"ground_roles = ["left_fin", "flipper"]"#,
    )]);

    assert!(
        error.contains(r#"ground_roles names ["flipper"]"#),
        "got: {error}"
    );
}

/// The lowest of one joint and itself is that joint, so the second row would
/// add nothing and hide a foot nobody reads.
#[test]
fn the_same_ground_role_twice_is_refused() {
    let error = refused(&[(
        r#"ground_roles = ["left_fin", "right_fin"]"#,
        r#"ground_roles = ["left_fin", "left_fin"]"#,
    )]);

    assert!(
        error.contains("ground_roles names left_fin twice"),
        "got: {error}"
    );
}

#[test]
fn a_skeleton_with_no_ground_role_is_refused() {
    let error = refused(&[(
        r#"ground_roles = ["left_fin", "right_fin"]"#,
        "ground_roles = []",
    )]);

    assert!(error.contains("no ground role"), "got: {error}");
}

/// `clip.stride_ratio` measures across these two on each rig, so a row that
/// is not a role has no joint to measure and a repeat has no length at all.
#[test]
fn the_committed_table_names_the_femur_as_the_segment_a_step_is_sized_by() {
    assert_eq!(
        table().stride_segment().as_slice(),
        ["left_upper_leg", "left_leg"]
    );
}

#[test]
fn a_stride_segment_role_no_convention_maps_is_refused() {
    let error = refused(&[(
        r#"stride_segment = ["hips", "left_fin"]"#,
        r#"stride_segment = ["hips", "flipper"]"#,
    )]);

    assert!(
        error.contains(r#"stride_segment names ["flipper"]"#),
        "got: {error}"
    );
}

#[test]
fn the_same_stride_segment_role_twice_is_refused() {
    let error = refused(&[(
        r#"stride_segment = ["hips", "left_fin"]"#,
        r#"stride_segment = ["hips", "hips"]"#,
    )]);

    assert!(
        error.contains("stride_segment names hips twice"),
        "got: {error}"
    );
}

#[test]
fn an_unknown_convention_names_the_ones_that_exist() {
    let error = format!("{:#}", table().bones("maya").unwrap_err());

    assert!(
        error.contains("maya") && error.contains("meshy, mixamo, standard"),
        "got: {error}"
    );
}

#[test]
fn a_missing_skeleton_file_is_reported_by_path() {
    let error = format!("{:#}", AimTable::of(&repo_root(), "quadruped").unwrap_err());

    assert!(
        error.contains("quadruped") && error.contains("art/skeletons"),
        "got: {error}"
    );
}

// --- what the loader refuses ----------------------------------------------

/// A missing row is refused rather than filled from the source's own rest
/// pose. A silent fallback is how the code this replaces left seven bones
/// uncorrected.
#[test]
fn a_role_with_no_aim_row_is_refused() {
    let error = refused(&[("hips = [0.0, 0.0, 1.0]\n", "")]);

    assert!(
        error.contains("no row for \"hips\""),
        "the refusal names the role: {error}"
    );
}

#[test]
fn an_aim_row_no_convention_maps_is_refused() {
    let error = refused(&[(
        "hips = [0.0, 0.0, 1.0]",
        "hips = [0.0, 0.0, 1.0]\ntail = [0.0, 1.0, 0.0]",
    )]);

    assert!(error.contains("tail"), "got: {error}");
}

#[test]
fn a_mirror_pair_that_is_not_a_reflection_is_refused() {
    // The X part not negated, which is the sign error the rule exists for.
    let error = refused(&[(
        "right_fin = [-1.0, 0.0, -1.0]",
        "right_fin = [1.0, 0.0, -1.0]",
    )]);
    assert!(error.contains("reflection"), "got: {error}");

    // And a Y or Z part that differs, which a mirror must not change.
    let error = refused(&[(
        "right_fin = [-1.0, 0.0, -1.0]",
        "right_fin = [-1.0, 1.0, -1.0]",
    )]);
    assert!(error.contains("reflection"), "got: {error}");
}

/// A convention with no right side leaves the left row nothing to reflect,
/// which is a different fault from a row that is simply missing.
#[test]
fn a_sided_role_with_no_mirror_row_is_refused() {
    let error = refused(&[
        ("right_fin = \"RightFin\"\n", ""),
        ("right_fin = [-1.0, 0.0, -1.0]\n", ""),
    ]);

    assert!(
        error.contains("no mirror row \"right_fin\""),
        "got: {error}"
    );
}

#[test]
fn a_row_that_is_no_direction_at_all_is_refused() {
    for broken in ["[0.0, 0.0, 0.0]", "[nan, 0.0, 1.0]", "[inf, 0.0, 1.0]"] {
        let error = refused(&[("hips = [0.0, 0.0, 1.0]", &format!("hips = {broken}"))]);
        assert!(error.contains("hips"), "{broken}: {error}");
    }
}

#[test]
fn a_row_that_is_not_three_numbers_is_refused() {
    // Two and four both reach our own check: serde reads three out of a
    // longer array without a word, which is how the fourth used to vanish.
    for broken in ["[0.0, 1.0]", "[0.0, 0.0, 1.0, 0.0]"] {
        let error = refused(&[("hips = [0.0, 0.0, 1.0]", &format!("hips = {broken}"))]);
        assert!(error.contains("a direction is three"), "{broken}: {error}");
    }
    // A row that is not an array at all never gets that far.
    let error = refused(&[("hips = [0.0, 0.0, 1.0]", "hips = \"up\"")]);
    assert!(error.contains("aim_table"), "got: {error}");
}

/// `[landmarks]` names the joints nothing drives, so a rig whose skull top is
/// missing could never be measured. The same refusals `skeleton.py` makes.
#[test]
fn every_convention_names_the_top_of_its_own_skull() {
    let table = table();

    for convention in ["meshy", "standard", "mixamo"] {
        let marks = table.landmarks(convention).unwrap();
        assert!(marks.contains_key(aim::SKULL_TOP), "{convention}");
    }
    assert_eq!(
        table.landmarks("mixamo").unwrap()[aim::SKULL_TOP],
        "HeadTop_End"
    );
    assert_eq!(
        table.landmarks("standard").unwrap()[aim::SKULL_TOP],
        "head_end"
    );
}

#[test]
fn an_unknown_convention_has_no_landmarks_and_names_the_ones_that_do() {
    let error = format!("{:#}", table().landmarks("maya").unwrap_err());

    assert!(error.contains("meshy, mixamo, standard"), "got: {error}");
}

#[test]
fn a_convention_with_no_landmark_row_is_refused() {
    let error = refused(&[("[landmarks.ours]", "[landmarks.theirs]")]);

    assert!(error.contains("every convention in"), "got: {error}");
}

#[test]
fn a_convention_that_names_no_skull_top_is_refused() {
    let error = refused(&[("skull_top = \"head_end\"", "chin = \"Chin\"")]);

    assert!(error.contains("names no \"skull_top\""), "got: {error}");
}

/// `[fingerprints]` is what reads a rig's convention off the rig, so a
/// convention nothing fingerprints could never be matched to a file.
#[test]
fn a_convention_with_no_fingerprint_of_its_own_is_refused() {
    let error = refused(&[("ours = [\"LeftFin\"]", "theirs = [\"LeftFin\"]")]);

    assert!(error.contains("every convention in"), "got: {error}");
}

#[test]
fn a_convention_whose_fingerprint_names_no_bone_is_refused() {
    let error = refused(&[("ours = [\"LeftFin\"]", "ours = []")]);

    assert!(error.contains("no fingerprint bone"), "got: {error}");
}

/// A bone two conventions both claim tells them apart from nothing, which is
/// the refusal `skeleton.py` makes on the other side of the same file.
#[test]
fn a_fingerprint_bone_two_conventions_share_is_refused() {
    let error = refused(&[
        (
            "[conventions.ours]",
            "[conventions.theirs]\nhips = \"Hips\"\nleft_fin = \"LeftFin\"\nright_fin = \"RightFin\"\n\n[conventions.ours]",
        ),
        (
            "ours = [\"LeftFin\"]",
            "ours = [\"LeftFin\"]\ntheirs = [\"leftfin\"]",
        ),
    ]);

    assert!(
        error.contains("tells them apart from nothing"),
        "got: {error}"
    );
}

#[test]
fn a_canonical_convention_that_is_not_declared_is_refused() {
    let error = refused(&[("canonical = \"ours\"", "canonical = \"theirs\"")]);

    assert!(error.contains("theirs"), "got: {error}");
}

#[test]
fn a_file_with_no_aim_table_at_all_is_refused() {
    let error = refused(&[("[aim_table]", "[unread]")]);

    assert!(error.contains("aim_table"), "got: {error}");
}

// --- the rule, on the committed rig ---------------------------------------

/// Every role of both committed files sits inside the band. The audit's
/// sideways `Hips` read 97.801 degrees on the rig this replaces, out of a hip
/// socket rather than up the spine; the conform turned it onto its own tail
/// and it reads 2.412.
#[test]
fn every_role_of_the_committed_rig_sits_inside_the_band() {
    for file in COMMITTED {
        let findings = findings_for(file);

        assert_eq!(rejected(&findings), [] as [&str; 0], "{file}");
        assert!(
            (measured(&findings, "hips") - 2.412).abs() < 0.01,
            "{file} measured {}, and 2.41 degrees was measured by hand",
            measured(&findings, "hips")
        );
    }
}

/// The numbers the committed rig produces, so the rule is pinned to values
/// and not only to a pass. Measured by an independent reader of the same
/// glTF, in Blender Z-up space.
#[test]
fn the_committed_rig_measures_what_was_measured_by_hand() {
    let findings = findings_for(COMMITTED[0]);

    for (role, expected) in [
        ("head", 0.90),
        ("right_hand", 32.93),
        ("left_hand", 31.57),
        ("left_arm", 20.78),
        ("left_shoulder", 4.22),
        ("neck", 2.10),
        ("left_toe", 7.33),
    ] {
        let measured = measured(&findings, role);
        assert!(
            (measured - expected).abs() < 0.01,
            "{role} measured {measured}, expected {expected}"
        );
    }
}

/// Why the arm rows aim 45 degrees below horizontal rather than straight out.
///
/// A T-pose table describes Mixamo's rig perfectly and ours not at all: the
/// hands land past a band the profile calls wide enough for an A-pose. The
/// numbers are here so that widening `max_bind_deviation_degrees` for another
/// rule's sake cannot quietly make a T-pose table legal.
#[test]
fn a_t_pose_table_would_put_this_rigs_hands_outside_the_band() {
    let mut text = std::fs::read_to_string(repo_root().join(SKELETON_FILE)).unwrap();
    for side in ["left", "right"] {
        let out = if side == "left" { "1.0" } else { "-1.0" };
        for bone in ["arm", "forearm", "hand"] {
            let row = format!("{side}_{bone} = [{out}, 0.0, -1.0]");
            assert!(text.contains(&row), "{row} is not in the committed table");
            text = text.replace(&row, &format!("{side}_{bone} = [{out}, 0.0, 0.0]"));
        }
    }
    let t_pose = AimTable::parse(&text).expect("a T-pose table is a valid table");

    let findings = aim::check_file(
        &committed_glb(COMMITTED[0]),
        &repo_root(),
        &profile(),
        &t_pose,
        t_pose.canonical(),
        1,
    )
    .unwrap();

    assert_eq!(
        rejected(&findings),
        ["left_hand", "right_hand"],
        "the two hands land outside the band"
    );
    for (role, expected) in [("left_hand", 75.16), ("right_hand", 76.40)] {
        let measured = measured(&findings, role);
        assert!(
            (measured - expected).abs() < 0.01 && measured > profile().max_bind_deviation_degrees,
            "{role} measured {measured}, expected {expected} and past the band"
        );
    }
    // The committed table is what keeps them in: 31.57 and 32.93.
    assert_eq!(rejected(&findings_for(COMMITTED[0])), [] as [&str; 0]);
}

/// A rule that goes quiet when it passes cannot be told from a rule that
/// never ran, so every role it resolves gets a finding either way.
#[test]
fn every_role_the_rig_fills_is_reported_either_way() {
    let findings = findings_for(COMMITTED[0]);

    assert_eq!(findings.len(), 22, "one finding per role");
    assert_eq!(
        findings
            .iter()
            .filter(|finding| finding.severity == Severity::Info)
            .count(),
        22,
        "twenty-two measurements, every one inside the band"
    );
    assert_eq!(
        findings
            .iter()
            .map(|finding| finding.subject.clone())
            .collect::<HashSet<String>>()
            .len(),
        22,
        "no role is measured twice"
    );
}

/// Every role the table aims, sorted, which is the list one rig owes.
fn owed() -> Vec<String> {
    table().roles().map(str::to_owned).collect()
}

/// The subjects a run reported, in the order it reported them.
fn subjects(findings: &[Finding]) -> Vec<String> {
    findings
        .iter()
        .map(|finding| finding.subject.clone())
        .collect()
}

/// A rig is measured on all 22 of its roles, whichever convention it is named
/// in, because it is read in the one its own fingerprint names.
#[test]
fn every_role_is_measured_on_a_vendor_rig_and_on_ours() {
    let owed = owed();
    assert_eq!(owed.len(), ROLES);

    for (file, convention) in [(PRE_RENAME, "meshy"), (COMMITTED[0], "standard")] {
        let findings = aim::check(&read_file(file), &profile(), &table(), convention, 1)
            .expect("a declared convention");

        assert_eq!(findings.len(), ROLES, "{file} under {convention}");
        assert_eq!(subjects(&findings), owed, "{file} under {convention}");
    }
}

/// And what reading one in the wrong convention costs, which is the defect
/// correction 7 of T15a records: `spine_middle`, `spine_upper` and `neck`
/// land on no bone, leave no finding, and the report is three rows short with
/// nothing saying so.
#[test]
fn a_vendor_rig_read_as_ours_silently_loses_three_roles() {
    let findings = aim::check(&read_file(PRE_RENAME), &profile(), &table(), "standard", 1)
        .expect("a declared convention");

    assert_eq!(findings.len(), 19);
    let reported = subjects(&findings);
    let missing: Vec<String> = owed()
        .into_iter()
        .filter(|role| !reported.contains(role))
        .collect();
    assert_eq!(missing, ["neck", "spine_middle", "spine_upper"]);
}

// --- the rule, on hand-built rigs -----------------------------------------

/// The conformant rig is named the way the profile and Mixamo name bones, so
/// it is read in that convention. The table describes it: nothing is
/// rejected.
#[test]
fn a_conformant_rig_breaks_no_row() {
    let findings = findings_of(&SyntheticRig::conformant(), "mixamo");

    assert_eq!(findings.len(), 22);
    assert_eq!(rejected(&findings), Vec::<String>::new(), "{findings:#?}");
    // Its arms hang at the 40 degrees the profile asks for, against an aim of
    // 45. Its toes are leaves, so they carry the foot's own axis, and that is
    // the widest row on a rig that breaks nothing.
    assert!((measured(&findings, "left_arm") - 5.0).abs() < 0.01);
    assert!(measured(&findings, "hips") < 1e-4);
    assert!((measured(&findings, "left_toe") - 33.690).abs() < 0.01);
}

/// A rig that names three of its bones something no convention uses. Those
/// roles are `rig.bone_set`'s business, so there is no rest aim here to
/// measure.
#[test]
fn a_role_whose_bone_the_rig_lacks_is_left_to_the_bone_set_rule() {
    let odd = SyntheticRig::conformant()
        .renamed("Spine1", "spine1")
        .renamed("Spine2", "spine2")
        .renamed("Neck", "neck");

    let findings = findings_of(&odd, "standard");

    assert_eq!(findings.len(), 19, "22 roles less Spine1, Spine2 and Neck");
    assert_eq!(rejected(&findings), Vec::<String>::new());
}

/// The `[synth]` negative for the rule itself: one bone's own axes turned 90
/// degrees while every joint stays exactly where it was. Nothing else can see
/// it, which is what the rest aim is for.
#[test]
fn a_bone_turned_off_its_aim_is_rejected_on_that_role_alone() {
    let rolled = SyntheticRig::conformant().re_aimed(
        "LeftHand",
        DQuat::from_rotation_z(std::f64::consts::FRAC_PI_2),
    );
    let skeleton = read(&rolled);

    let findings = aim::check(&skeleton, &profile(), &table(), "mixamo", 1).unwrap();

    assert_eq!(rejected(&findings), ["left_hand"]);
    // A quarter turn from a rest aim that already sat 5 degrees out.
    assert!(
        (measured(&findings, "left_hand") - 95.0).abs() < 0.01,
        "measured {}",
        measured(&findings, "left_hand")
    );
    // The mirror of it is untouched, so the fixture is one row and not a pose.
    assert!(measured(&findings, "right_hand") < 5.01);
    // And the claim this whole rule rests on: no other rule can see it. Every
    // joint sits exactly where it did, and `LeftHand` is a leaf, so nothing
    // measures a direction toward it. If a `rig.*` rule fired here,
    // `rig.aim_table` would be a second copy of that rule.
    let elsewhere = rig::check(
        "fixture.gltf",
        &skeleton,
        &profile(),
        HEIGHT_METERS,
        Symmetry::Enforced,
        1,
    );
    assert_eq!(broken(&elsewhere), Vec::<String>::new(), "{elsewhere:#?}");
}

/// A bone with no scale has no axis at all. Reporting that beats normalizing
/// a zero vector into a NaN.
#[test]
fn a_bone_with_no_axis_is_an_undefined_measurement() {
    let findings = findings_of(&SyntheticRig::conformant().without_scale("Hips"), "mixamo");

    let hips = findings
        .iter()
        .find(|finding| finding.subject == "hips")
        .unwrap();
    assert_eq!(hips.severity, Severity::Error);
    assert_eq!(hips.unit, "undefined measurements");
    assert!(hips.message.contains("scale is zero"), "{}", hips.message);
}

#[test]
fn a_file_that_holds_no_rig_is_one_error_and_never_a_skip() {
    let table = table();
    let findings = aim::check_file(
        &repo_root().join(SKELETON_FILE),
        &repo_root(),
        &profile(),
        &table,
        table.canonical(),
        1,
    )
    .unwrap();

    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].severity, Severity::Error);
    assert_eq!(findings[0].subject, SKELETON_FILE);
}

#[test]
fn a_rig_read_in_a_convention_the_file_does_not_declare_is_an_error() {
    let skeleton = Skeleton::from_slice(SyntheticRig::conformant().to_gltf().as_bytes()).unwrap();

    let error = format!(
        "{:#}",
        aim::check(&skeleton, &profile(), &table(), "maya", 1).unwrap_err()
    );

    assert!(error.contains("maya"), "got: {error}");
}

// --- the registry ---------------------------------------------------------

#[test]
fn the_rule_is_listed_once_under_the_rig_family() {
    let ids: BTreeSet<&str> = aim::RULES.iter().map(|rule| rule.id).collect();

    assert_eq!(ids.len(), aim::RULES.len());
    assert_eq!(aim::RULES.len(), 1);
    assert_eq!(ids.into_iter().next(), Some("rig.aim_table"));
}

/// `--list-rules` prints the registry and a finding is built through it, so
/// neither can advertise a limit, a unit or a space the other does not carry.
#[test]
fn a_finding_carries_exactly_what_the_rule_list_advertises() {
    let profile = profile();
    let findings = [
        findings_of(&SyntheticRig::conformant(), "mixamo"),
        findings_for(COMMITTED[0]),
    ]
    .concat();

    for finding in &findings {
        let rule: &&Rule = aim::RULES
            .iter()
            .find(|rule| rule.id == finding.rule)
            .unwrap_or_else(|| panic!("{} is not in the rule list", finding.rule));
        assert_eq!(finding.measured_on, rule.space);
        assert!(
            finding.measured_on.contains("Blender") && finding.measured_on.contains("child axis"),
            "the space is named: {}",
            finding.measured_on
        );
        // An undefined measurement carries its own unit and comparison, and
        // says so, which is the one documented exception.
        if finding.unit != "undefined measurements" {
            assert_eq!(finding.unit, rule.unit);
            assert_eq!(finding.comparison, rule.comparison);
            assert_eq!(finding.limit, profile.max_bind_deviation_degrees);
        }
    }
}

/// The comparison decides the severity, so the rule cannot file its own
/// broken measurement as information.
#[test]
fn the_severity_of_every_finding_follows_from_its_own_comparison() {
    let findings = [
        findings_of(&SyntheticRig::conformant(), "mixamo"),
        findings_for(COMMITTED[0]),
        findings_of(
            &SyntheticRig::conformant().turned(DQuat::from_rotation_x(std::f64::consts::FRAC_PI_2)),
            "mixamo",
        ),
    ]
    .concat();

    for finding in &findings {
        assert_eq!(
            finding.severity == Severity::Info,
            finding.holds(),
            "{} on {}: {finding:#?}",
            finding.rule,
            finding.subject
        );
    }
}

/// A rig laid on its side, which is what the band is wide enough to catch
/// and nothing narrower. The height is passed to keep the fixture honest
/// about which rig it is.
#[test]
fn a_rig_lying_down_is_rejected_on_most_of_its_roles() {
    assert_eq!(HEIGHT_METERS, 1.70);
    let laid =
        SyntheticRig::conformant().turned(DQuat::from_rotation_x(std::f64::consts::FRAC_PI_2));

    let findings = findings_of(&laid, "mixamo");

    assert_eq!(
        errors(&findings).len(),
        14,
        "the torso, the legs and the feet all point along the floor: {:#?}",
        rejected(&findings)
    );
}
