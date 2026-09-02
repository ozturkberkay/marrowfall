//! The thirteen rig gates.
//!
//! Every rule here has three columns, per the design's test plan: a positive
//! fixture, a negative fixture it must reject, and a calibration that proves
//! it stays quiet on art it should accept.
//!
//! The negative is the **committed rig** wherever real broken art exists,
//! because that is the failure this pipeline shipped. Seven rules reject it
//! today. The gates become required CI checks when the survivor is
//! regenerated, so these tests assert the failure rather than fixing it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use glam::{DQuat, DVec3};
use xtask_art::check::gltf_world::Skeleton;
use xtask_art::check::profile::Profile;
use xtask_art::check::{Comparison, Finding, Report, Rule, Severity, rig};
use xtask_art::library::HUMANOID;

use crate::rigs::{HEIGHT_METERS, HUMERUS_BELOW_HORIZONTAL, SyntheticRig};
use crate::support::{committed_glb, repo_root};

/// The seven rules the committed rig breaks. Every other rule must stay
/// quiet on it, or the gates could never be made required.
const BROKEN_TODAY: [&str; 7] = [
    "rig.bone_set",
    "rig.child_axis",
    "rig.humerus_angle",
    "rig.mirror_direction",
    "rig.mirror_length",
    "rig.names_standard",
    "rig.parents",
];

/// The shared skeleton, and the survivor rigged onto it. The same 24 bones,
/// so both fail the same seven rules.
const COMMITTED: [&str; 2] = [
    "art/skeletons/humanoid.glb",
    "art/characters/survivor/model.glb",
];

fn profile() -> Profile {
    Profile::of(&repo_root(), HUMANOID).expect("the committed humanoid profile")
}

/// A path under the repository, for the two tests that hand the rules
/// something that is not a rig at all.
fn under_repo(file: &str) -> PathBuf {
    repo_root().join(file)
}

/// Every finding for one file, at the height the survivor's spec asks for.
fn findings_for(file: &Path) -> Vec<Finding> {
    rig::check_file(file, &repo_root(), &profile(), HEIGHT_METERS, 1)
        .expect("the rig rules run without a Blender")
}

/// Every finding for a hand-built rig.
fn findings_of(fixture: &SyntheticRig) -> Vec<Finding> {
    let skeleton = Skeleton::from_slice(fixture.to_gltf().as_bytes()).expect("valid glTF");
    rig::check("fixture.gltf", &skeleton, &profile(), HEIGHT_METERS, 1)
}

fn errors(findings: &[Finding]) -> Vec<&Finding> {
    findings
        .iter()
        .filter(|finding| finding.severity == Severity::Error)
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

/// The subjects one rule reported an error on.
fn rejected(findings: &[Finding], rule: &str) -> Vec<String> {
    errors(findings)
        .iter()
        .filter(|finding| finding.rule == rule)
        .map(|finding| finding.subject.clone())
        .collect()
}

/// One rule's measurement of one subject.
fn measured(findings: &[Finding], rule: &str, subject: &str) -> f64 {
    findings
        .iter()
        .find(|finding| finding.rule == rule && finding.subject == subject)
        .unwrap_or_else(|| panic!("{rule} never measured {subject}"))
        .measured
}

/// The worst measurement one rule made.
fn worst(findings: &[Finding], rule: &str) -> f64 {
    findings
        .iter()
        .filter(|finding| finding.rule == rule)
        .map(|finding| finding.measured)
        .reduce(f64::max)
        .unwrap_or_else(|| panic!("{rule} measured nothing"))
}

// --- the positive fixture -------------------------------------------------

#[test]
fn a_conformant_rig_breaks_no_rule() {
    let findings = findings_of(&SyntheticRig::conformant());

    assert_eq!(broken(&findings), Vec::<String>::new(), "{findings:#?}");
}

/// The numbers a conformant rig produces, so the rules are pinned to values
/// and not only to a pass.
#[test]
fn a_conformant_rig_measures_zero_on_every_rule() {
    let findings = findings_of(&SyntheticRig::conformant());

    for (rule, subject) in [
        ("rig.names_standard", "Hips"),
        ("rig.single_root", "LeftHand"),
        ("rig.parents", "LeftShoulder"),
        ("rig.facing", "LeftFoot"),
        ("rig.up_axis", "Hips to head_end"),
        ("rig.object_transform", "fixture.gltf"),
    ] {
        assert_eq!(
            measured(&findings, rule, subject),
            0.0,
            "{rule} on {subject}"
        );
    }
    assert_eq!(measured(&findings, "rig.bone_set", "Neck"), 1.0);
    for rule in [
        "rig.child_axis",
        "rig.mirror_length",
        "rig.mirror_direction",
        "rig.humerus_angle",
        "rig.bind_deviation",
        "rig.world_height",
    ] {
        // glTF stores f32, so this is the file's own precision, not a lean.
        assert!(
            worst(&findings, rule) < 1e-4,
            "{rule}: {}",
            worst(&findings, rule)
        );
    }
}

/// How many subjects each rule measures on the committed rig, worked out
/// from the profile and the rig by hand.
///
/// The count is the rest of the family: without it a rule that quietly stops
/// measuring one *passing* subject changes nothing any other test reads.
/// `parents` skips the 3 bones the rig does not have, and `child_axis` and
/// `bind_deviation` skip the chain steps that need them.
const SUBJECTS: [(&str, usize); 13] = [
    ("rig.names_standard", 24),
    ("rig.bone_set", 24),
    ("rig.single_root", 24),
    ("rig.parents", 20),
    ("rig.child_axis", 14),
    ("rig.mirror_length", 6),
    ("rig.mirror_direction", 6),
    ("rig.humerus_angle", 2),
    ("rig.facing", 2),
    ("rig.up_axis", 1),
    ("rig.bind_deviation", 2),
    ("rig.world_height", 1),
    ("rig.object_transform", 1),
];

#[test]
fn every_rule_measures_every_subject_it_should() {
    let findings = findings_for(&committed_glb(COMMITTED[0]));

    for (rule, expected) in SUBJECTS {
        let measured = findings.iter().filter(|f| f.rule == rule).count();
        assert_eq!(measured, expected, "{rule} measured {measured} subjects");
    }
    assert_eq!(
        findings.len(),
        SUBJECTS.iter().map(|(_, count)| count).sum::<usize>(),
        "127 measurements, and no rule reports outside the list"
    );
}

/// The same for the conformant rig, which resolves every subject there is.
#[test]
fn a_conformant_rig_leaves_no_subject_unmeasured() {
    let findings = findings_of(&SyntheticRig::conformant());

    for (rule, subjects) in [
        ("rig.parents", 23),
        ("rig.child_axis", 18),
        ("rig.bind_deviation", 6),
    ] {
        let measured = findings.iter().filter(|f| f.rule == rule).count();
        assert_eq!(measured, subjects, "{rule} measured {measured} subjects");
    }
    assert_eq!(findings.len(), 138);
}

/// A rule that goes quiet when it passes is indistinguishable from a rule
/// that never ran, and this pipeline has already shipped one of those.
#[test]
fn every_rule_reports_on_good_art_and_on_bad() {
    for findings in [
        findings_of(&SyntheticRig::conformant()),
        findings_for(&committed_glb(COMMITTED[0])),
    ] {
        for rule in rig::RULES {
            assert!(
                findings.iter().any(|finding| finding.rule == rule.id),
                "{} reported nothing at all",
                rule.id
            );
        }
    }
}

// --- the committed rig, which seven rules reject --------------------------

#[test]
fn the_committed_rig_breaks_exactly_the_seven_rules_the_design_names() {
    for file in COMMITTED {
        assert_eq!(
            broken(&findings_for(&committed_glb(file))),
            BROKEN_TODAY,
            "{file}"
        );
    }
}

#[test]
fn the_committed_rig_is_named_by_no_convention() {
    let findings = findings_for(&committed_glb(COMMITTED[0]));

    assert_eq!(
        rejected(&findings, "rig.names_standard"),
        ["Spine02", "Spine01", "neck"]
    );
}

#[test]
fn the_committed_rig_is_missing_three_of_the_bones_the_profile_requires() {
    let findings = findings_for(&committed_glb(COMMITTED[0]));

    // In the order the profile lists them.
    assert_eq!(
        rejected(&findings, "rig.bone_set"),
        ["Spine1", "Spine2", "Neck"]
    );
    assert_eq!(measured(&findings, "rig.bone_set", "Neck"), 0.0);
    assert_eq!(measured(&findings, "rig.bone_set", "Hips"), 1.0);
}

/// This rig's `Spine` is its highest and the profile's is its lowest, which
/// is the rename the whole convention decision turns on.
#[test]
fn the_committed_rig_hangs_four_bones_from_the_wrong_parent() {
    let findings = findings_for(&committed_glb(COMMITTED[0]));

    assert_eq!(
        rejected(&findings, "rig.parents"),
        ["Head", "LeftShoulder", "RightShoulder", "Spine"]
    );
    let message = &findings
        .iter()
        .find(|finding| finding.rule == "rig.parents" && finding.subject == "Spine")
        .unwrap()
        .message;
    assert!(
        message.contains("Spine01") && message.contains("Hips"),
        "a defect names what it found and what it wanted: {message}"
    );
}

/// The audit's sideways `Hips`, whose own +Y points out of a hip socket
/// instead of up the spine, and the head pitched off its own child.
#[test]
fn the_committed_rigs_hips_and_head_axes_point_the_wrong_way() {
    let findings = findings_for(&committed_glb(COMMITTED[0]));

    assert_eq!(rejected(&findings, "rig.child_axis"), ["Head", "Hips"]);
    assert!(
        (measured(&findings, "rig.child_axis", "Hips") - 97.617).abs() < 0.01,
        "measured {}, and 97.617 degrees was measured by hand",
        measured(&findings, "rig.child_axis", "Hips")
    );
    assert!((measured(&findings, "rig.child_axis", "Head") - 26.002).abs() < 0.01);
    // Every limb bone in this rig does point at its child.
    assert!(measured(&findings, "rig.child_axis", "LeftArm") < 0.01);
}

#[test]
fn the_committed_rig_is_asymmetric_by_up_to_three_point_seven_percent() {
    let findings = findings_for(&committed_glb(COMMITTED[0]));

    assert_eq!(
        rejected(&findings, "rig.mirror_length"),
        ["Foot", "ForeArm", "Leg", "Shoulder", "UpLeg"],
        "only the upper arms are inside the 1 percent band"
    );
    assert!((measured(&findings, "rig.mirror_length", "Foot") - 3.678).abs() < 0.01);
    assert!((measured(&findings, "rig.mirror_length", "Arm") - 0.945).abs() < 0.01);
}

#[test]
fn the_committed_rigs_left_and_right_segments_point_different_ways() {
    let findings = findings_for(&committed_glb(COMMITTED[0]));

    assert_eq!(
        rejected(&findings, "rig.mirror_direction"),
        ["Foot", "ForeArm", "Leg"]
    );
    assert!((measured(&findings, "rig.mirror_direction", "Foot") - 2.162).abs() < 0.01);
    assert!((measured(&findings, "rig.mirror_direction", "ForeArm") - 1.453).abs() < 0.01);
}

/// The prompt asks for 40 degrees and the auto-rigger delivered 59, which
/// nothing has ever checked.
#[test]
fn the_committed_rigs_arms_hang_nineteen_degrees_below_the_target() {
    let findings = findings_for(&committed_glb(COMMITTED[0]));

    assert_eq!(
        rejected(&findings, "rig.humerus_angle"),
        ["LeftArm", "RightArm"]
    );
    assert!((measured(&findings, "rig.humerus_angle", "LeftArm") - 19.141).abs() < 0.01);
    assert!((measured(&findings, "rig.humerus_angle", "RightArm") - 19.350).abs() < 0.01);
    let message = &findings
        .iter()
        .find(|finding| finding.rule == "rig.humerus_angle")
        .unwrap()
        .message;
    assert!(message.contains("59.1"), "got: {message}");
}

// --- the committed rig, calibration: what must stay quiet -----------------

/// 1.6652 m against the spec's 1.7 is 2.05 percent, inside the 5 percent
/// band, so the one height rule accepts the rig as it stands.
#[test]
fn the_committed_rig_is_two_percent_short_and_that_is_allowed() {
    let findings = findings_for(&committed_glb(COMMITTED[0]));
    let measured = measured(&findings, "rig.world_height", COMMITTED[0]);

    assert!(
        (measured - 2.049).abs() < 0.01,
        "measured {measured} percent, and 2.05 was measured by hand"
    );
    assert!(rejected(&findings, "rig.world_height").is_empty());
}

#[test]
fn the_committed_rig_stands_up_faces_forward_and_carries_no_action() {
    let findings = findings_for(&committed_glb(COMMITTED[0]));

    for rule in [
        "rig.single_root",
        "rig.up_axis",
        "rig.facing",
        "rig.bind_deviation",
        "rig.object_transform",
    ] {
        assert!(rejected(&findings, rule).is_empty(), "{rule}");
    }
    // The rest pose leans a couple of degrees, well inside the wide band
    // that exists to catch a rig lying down.
    assert!(worst(&findings, "rig.bind_deviation") < 5.0);
}

/// The survivor's own file carries a bind-pose action on its bones. That is
/// not an object-level action, which is the state this rule refuses.
#[test]
fn a_rig_whose_bones_carry_an_action_still_passes_the_object_rule() {
    let findings = findings_for(&committed_glb("art/characters/survivor/model.glb"));

    assert_eq!(
        measured(&findings, "rig.object_transform", COMMITTED[1]),
        0.0
    );
}

// --- the synthetic negatives ----------------------------------------------

#[test]
fn a_bone_parented_outside_the_root_is_rejected() {
    let findings = findings_of(&SyntheticRig::conformant().reparented("LeftUpLeg", None));

    assert_eq!(
        rejected(&findings, "rig.single_root"),
        ["LeftUpLeg", "LeftLeg", "LeftFoot", "LeftToeBase"],
        "the whole branch leaves the skeleton"
    );
    assert!(broken(&findings).contains(&"rig.single_root".to_owned()));
}

/// The 100x mistake, made on purpose: the same rig under an object node
/// scaled by 100 stands 170 m tall.
#[test]
fn a_rig_scaled_by_a_hundred_is_rejected_and_nothing_else_notices() {
    let findings = findings_of(&SyntheticRig::conformant().scaled(100.0));

    assert_eq!(broken(&findings), ["rig.world_height"]);
    let measured = measured(&findings, "rig.world_height", "fixture.gltf");
    assert!(
        (measured - 9900.0).abs() < 1.0,
        "170 m against 1.7 is 9900 percent, measured {measured}"
    );
}

#[test]
fn a_rig_lying_on_its_side_is_rejected() {
    let side = DQuat::from_rotation_z(std::f64::consts::FRAC_PI_2);
    let findings = findings_of(&SyntheticRig::conformant().turned(side));

    assert!(broken(&findings).contains(&"rig.bind_deviation".to_owned()));
    let worst = worst(&findings, "rig.bind_deviation");
    assert!(
        (worst - 90.0).abs() < 1e-6,
        "measured {worst} degrees from up"
    );
}

#[test]
fn a_rig_yawed_a_hundred_and_eighty_degrees_faces_the_wrong_way() {
    let away = DQuat::from_rotation_y(std::f64::consts::PI);
    let findings = findings_of(&SyntheticRig::conformant().turned(away));

    assert_eq!(rejected(&findings, "rig.facing"), ["LeftFoot", "RightFoot"]);
    // Turning about the up axis leaves the body upright, so the rules that
    // measure that stay quiet.
    assert_eq!(broken(&findings), ["rig.facing"]);
    let message = &findings
        .iter()
        .find(|finding| finding.rule == "rig.facing")
        .unwrap()
        .message;
    assert!(
        message.contains("-z") && message.contains("180"),
        "got: {message}"
    );
}

/// A Y-up rig exported with no axis conversion: the body runs along glTF +Z,
/// which is minus Y once Blender has imported it.
#[test]
fn a_rig_with_no_up_axis_conversion_is_rejected() {
    let unconverted = DQuat::from_rotation_x(std::f64::consts::FRAC_PI_2);
    let findings = findings_of(&SyntheticRig::conformant().turned(unconverted));

    assert_eq!(rejected(&findings, "rig.up_axis"), ["Hips to head_end"]);
    let message = &findings
        .iter()
        .find(|finding| finding.rule == "rig.up_axis")
        .unwrap()
        .message;
    assert!(message.contains("-y"), "got: {message}");
}

/// An axis is a choice of six, so `rig.facing` and `rig.up_axis` accept
/// anything inside 45 degrees of the declared one by design. A yaw between
/// the two is not their business: `rig.mirror_direction` is what sees it,
/// because a reflection turns a yaw into twice that angle between the sides.
#[test]
fn a_rig_yawed_thirty_degrees_is_caught_by_the_mirror_rule() {
    let part_way = DQuat::from_rotation_y(std::f64::consts::FRAC_PI_6);
    let findings = findings_of(&SyntheticRig::conformant().turned(part_way));

    assert_eq!(broken(&findings), ["rig.mirror_direction"]);
    let worst = worst(&findings, "rig.mirror_direction");
    assert!(
        (worst - 60.0).abs() < 1e-4,
        "a 30 degree yaw is 60 degrees between the sides, measured {worst}"
    );
    // And the two axis rules stay quiet, which is what makes this test the
    // one that covers the gap between them.
    for rule in ["rig.facing", "rig.up_axis"] {
        assert!(rejected(&findings, rule).is_empty(), "{rule}");
    }
}

#[test]
fn a_rig_carrying_an_action_on_its_object_is_rejected() {
    let findings = findings_of(&SyntheticRig::conformant().with_object_action());

    assert_eq!(broken(&findings), ["rig.object_transform"]);
    assert_eq!(
        measured(&findings, "rig.object_transform", "fixture.gltf"),
        1.0
    );
    let message = &findings
        .iter()
        .find(|finding| finding.rule == "rig.object_transform")
        .unwrap()
        .message;
    assert!(message.contains("Armature"), "got: {message}");
}

#[test]
fn a_renamed_bone_is_rejected_by_both_name_rules() {
    let findings = findings_of(&SyntheticRig::conformant().renamed("Neck", "neck"));

    assert_eq!(rejected(&findings, "rig.names_standard"), ["neck"]);
    assert_eq!(rejected(&findings, "rig.bone_set"), ["Neck"]);
}

#[test]
fn a_bone_named_twice_is_rejected() {
    let findings = findings_of(&SyntheticRig::conformant().renamed("Spine1", "Spine"));

    assert_eq!(measured(&findings, "rig.bone_set", "Spine"), 2.0);
    assert!(rejected(&findings, "rig.bone_set").contains(&"Spine".to_owned()));
}

#[test]
fn an_arm_lifted_out_of_the_band_is_rejected() {
    // 40 degrees below horizontal is the target and 15 is the band, so
    // lifting the left forearm to level is 40 degrees out.
    let level = DVec3::new(0.0, 0.26 * HUMERUS_BELOW_HORIZONTAL.to_radians().sin(), 0.0);
    let findings = findings_of(&SyntheticRig::conformant().moved("LeftForeArm", level));

    assert!(rejected(&findings, "rig.humerus_angle").contains(&"LeftArm".to_owned()));
    assert!(
        (measured(&findings, "rig.humerus_angle", "LeftArm") - HUMERUS_BELOW_HORIZONTAL).abs()
            < 1e-6
    );
    assert!(
        rejected(&findings, "rig.humerus_angle").len() == 1,
        "the right arm is untouched"
    );
}

#[test]
fn a_limb_that_is_longer_on_one_side_is_rejected() {
    let stretched = DVec3::new(0.02, 0.0, 0.0);
    let findings = findings_of(&SyntheticRig::conformant().moved("LeftHand", stretched));

    assert!(rejected(&findings, "rig.mirror_length").contains(&"ForeArm".to_owned()));
    assert!(rejected(&findings, "rig.mirror_direction").contains(&"ForeArm".to_owned()));
}

/// The audit's sideways `Hips`, in miniature: the child moves and the bone's
/// own axis stays where it was.
#[test]
fn a_bone_whose_axis_misses_its_child_is_rejected() {
    let aside = DVec3::new(0.0, 0.0, 0.2);
    let findings = findings_of(&SyntheticRig::conformant().nudged("LeftHand", aside));

    assert_eq!(rejected(&findings, "rig.child_axis"), ["LeftForeArm"]);
    assert!(measured(&findings, "rig.child_axis", "LeftForeArm") > 2.0);
}

// --- measurements that do not exist ---------------------------------------

/// A gate never emits a NaN. Where a measurement is undefined it says so, as
/// an error, with its own unit.
#[test]
fn two_joints_at_the_same_place_report_an_undefined_measurement() {
    let findings = findings_of(&SyntheticRig::conformant().coincident("LeftToeBase", "LeftFoot"));

    let undefined: Vec<&Finding> = findings
        .iter()
        .filter(|finding| finding.unit == "undefined measurements")
        .collect();
    assert!(
        undefined
            .iter()
            .all(|finding| finding.severity == Severity::Error),
        "an undefined measurement can never pass: {undefined:#?}"
    );
    for rule in [
        "rig.child_axis",
        "rig.facing",
        "rig.mirror_direction",
        "rig.mirror_length",
    ] {
        assert!(
            undefined.iter().any(|finding| finding.rule == rule),
            "{rule} had nothing to measure and said nothing"
        );
    }
    assert!(findings.iter().all(|finding| finding.measured.is_finite()));
}

/// Cancellation noise is not a segment. Dividing a percentage by a mean of
/// it, or normalizing it into a direction, reports a precise number about
/// two segments that do not exist. **Both** mirror rules have to refuse it,
/// and the upper arms are where that noise is not exactly zero.
#[test]
fn a_mirror_pair_with_no_length_is_undefined_for_both_mirror_rules() {
    let findings = findings_of(
        &SyntheticRig::conformant()
            .coincident("LeftForeArm", "LeftArm")
            .coincident("RightForeArm", "RightArm"),
    );

    for rule in ["rig.mirror_length", "rig.mirror_direction"] {
        let pair = findings
            .iter()
            .find(|finding| finding.rule == rule && finding.subject == "Arm")
            .unwrap_or_else(|| panic!("{rule} never reported on the arms"));
        assert_eq!(pair.unit, "undefined measurements", "{pair:#?}");
        assert_eq!(pair.severity, Severity::Error);
    }
}

/// The other side of that guard: one side missing is not a pair either, so
/// neither mirror rule invents a percentage out of the side that is there.
#[test]
fn a_mirror_pair_with_no_length_on_one_side_is_undefined() {
    let findings = findings_of(&SyntheticRig::conformant().coincident("LeftToeBase", "LeftFoot"));

    for rule in ["rig.mirror_length", "rig.mirror_direction"] {
        let pair = findings
            .iter()
            .find(|finding| finding.rule == rule && finding.subject == "Foot")
            .unwrap_or_else(|| panic!("{rule} never reported on the feet"));
        assert_eq!(pair.unit, "undefined measurements", "{pair:#?}");
    }
}

/// The other side of the noise floor: a segment can be absurdly short and
/// still be a segment, so the floor stays at the file's own precision rather
/// than at a size a modeler might use.
#[test]
fn a_bone_a_tenth_of_a_millimeter_long_is_still_measured() {
    let tiny = DVec3::new(0.0, 0.0, 1e-4);
    let findings = findings_of(
        &SyntheticRig::conformant()
            .coincident("LeftToeBase", "LeftFoot")
            .moved("LeftToeBase", tiny),
    );
    let foot = findings
        .iter()
        .find(|finding| finding.rule == "rig.child_axis" && finding.subject == "LeftFoot")
        .unwrap();

    assert_eq!(foot.unit, "degrees", "a real measurement: {foot:#?}");
    assert!(measured(&findings, "rig.mirror_length", "Foot") > 100.0);
}

/// The facing comes from the horizontal part of the foot, and a foot that
/// points straight up has none. The design names this case: a gate reports
/// the reason rather than normalizing a zero into a NaN.
#[test]
fn a_foot_pointing_straight_up_leaves_the_facing_undefined() {
    let upright = DVec3::new(0.0, 0.12, -0.12);
    let findings = findings_of(&SyntheticRig::conformant().moved("LeftToeBase", upright));
    let facing = findings
        .iter()
        .find(|finding| finding.rule == "rig.facing" && finding.subject == "LeftFoot")
        .unwrap();

    assert_eq!(facing.severity, Severity::Error);
    assert!(facing.message.contains("faces nowhere"), "{facing:#?}");
    assert!(findings.iter().all(|finding| finding.measured.is_finite()));
}

#[test]
fn an_arm_with_no_length_leaves_its_angle_undefined() {
    let findings = findings_of(&SyntheticRig::conformant().coincident("LeftForeArm", "LeftArm"));
    let humerus = findings
        .iter()
        .find(|finding| finding.rule == "rig.humerus_angle" && finding.subject == "LeftArm")
        .unwrap();

    assert_eq!(humerus.severity, Severity::Error);
    assert!(humerus.message.contains("same place"), "{humerus:#?}");
}

/// The root chain is what says which way the body stands up, so a rig whose
/// head sits in its hips has no up direction to name.
#[test]
fn a_root_chain_with_no_length_leaves_the_up_axis_undefined() {
    let findings = findings_of(&SyntheticRig::conformant().coincident("head_end", "Hips"));
    let up = findings
        .iter()
        .find(|finding| finding.rule == "rig.up_axis")
        .unwrap();

    assert_eq!(up.severity, Severity::Error);
    assert!(up.message.contains("same place"), "{up:#?}");
}

#[test]
fn a_root_chain_step_with_no_length_leaves_the_lean_undefined() {
    let findings = findings_of(&SyntheticRig::conformant().coincident("head_end", "Head"));
    let step = findings
        .iter()
        .find(|finding| {
            finding.rule == "rig.bind_deviation" && finding.subject == "Head to head_end"
        })
        .unwrap();

    assert_eq!(step.severity, Severity::Error);
    assert!(step.message.contains("same place"), "{step:#?}");
}

/// A rig missing the top of its root chain has no up direction to measure,
/// and `rig.bone_set` is the rule that reports the missing bone.
#[test]
fn a_missing_chain_end_is_reported_once_by_the_rule_that_owns_it() {
    let findings = findings_of(&SyntheticRig::conformant().renamed("head_end", "tip"));

    assert!(!findings.iter().any(|finding| finding.rule == "rig.up_axis"));
    assert_eq!(rejected(&findings, "rig.bone_set"), ["head_end"]);
    assert_eq!(rejected(&findings, "rig.names_standard"), ["tip"]);
}

/// A mirror rule needs both sides. One side missing is `rig.bone_set`'s to
/// report, and the pair is left unmeasured rather than half measured.
#[test]
fn a_segment_with_no_other_side_is_not_measured_as_a_mirror() {
    let findings = findings_of(&SyntheticRig::conformant().renamed("RightFoot", "footR"));

    for rule in ["rig.mirror_length", "rig.mirror_direction"] {
        assert!(
            !findings
                .iter()
                .any(|finding| finding.rule == rule && finding.subject == "Foot"),
            "{rule} measured a pair with one side missing"
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.rule == rule && finding.subject == "Arm"),
            "{rule} still measures the pairs that are whole"
        );
    }
    assert_eq!(rejected(&findings, "rig.bone_set"), ["RightFoot"]);
    // The right leg loses its tail with it, so that pair goes unmeasured too.
    assert!(!findings.iter().any(|finding| finding.subject == "Leg"));
}

#[test]
fn a_bone_with_no_scale_has_no_axis_to_measure() {
    let findings = findings_of(&SyntheticRig::conformant().without_scale("LeftFoot"));
    let message = &findings
        .iter()
        .find(|finding| finding.rule == "rig.child_axis" && finding.subject == "LeftFoot")
        .unwrap()
        .message;

    assert!(message.contains("scale is zero"), "got: {message}");
    assert!(findings.iter().all(|finding| finding.measured.is_finite()));
}

#[test]
fn a_file_that_is_no_rig_at_all_is_an_error_and_not_a_skip() {
    let findings = findings_for(&under_repo("art/skeletons/humanoid.toml"));

    assert_eq!(errors(&findings).len(), 1, "{findings:#?}");
    assert_eq!(errors(&findings)[0].rule, "rig.bone_set");
    assert!(
        errors(&findings)[0]
            .message
            .contains("no readable skeleton")
    );
}

#[test]
fn a_missing_file_is_an_error_and_not_a_skip() {
    let findings = findings_for(&under_repo("art/skeletons/nothing.glb"));

    assert_eq!(errors(&findings).len(), 1);
    assert!(errors(&findings)[0].message.contains("nothing.glb"));
}

// --- the rule list ---------------------------------------------------------

#[test]
fn every_rule_is_listed_once_under_its_own_family() {
    let ids: BTreeSet<&str> = rig::RULES.iter().map(|rule| rule.id).collect();

    assert_eq!(ids.len(), rig::RULES.len(), "a rule id is listed twice");
    assert_eq!(rig::RULES.len(), 13);
    assert!(ids.iter().all(|id| id.starts_with("rig.")));
}

/// `--list-rules` prints the registry, and a finding is built through it, so
/// neither can advertise a limit, a unit or a space the other does not
/// carry.
#[test]
fn a_finding_carries_exactly_what_the_rule_list_advertises() {
    let profile = profile();
    let listed: Vec<&&Rule> = rig::RULES.iter().collect();
    let findings = [
        findings_of(&SyntheticRig::conformant()),
        findings_for(&committed_glb(COMMITTED[0])),
    ]
    .concat();

    for finding in &findings {
        let rule = listed
            .iter()
            .find(|rule| rule.id == finding.rule)
            .unwrap_or_else(|| panic!("{} is not in the rule list", finding.rule));
        assert_eq!(finding.measured_on, rule.space, "{}", rule.id);
        // An undefined measurement carries its own unit and comparison, and
        // says so, which is the one documented exception.
        if finding.unit != "undefined measurements" {
            assert_eq!(finding.unit, rule.unit, "{}", rule.id);
            assert_eq!(finding.comparison, rule.comparison, "{}", rule.id);
            assert_eq!(finding.limit, (rule.limit)(&profile), "{}", rule.id);
        }
    }
}

/// The comparison decides the severity, so no rule can file its own broken
/// measurement as information.
#[test]
fn the_severity_of_every_finding_follows_from_its_own_comparison() {
    let findings = [
        findings_of(&SyntheticRig::conformant()),
        findings_for(&committed_glb(COMMITTED[0])),
        findings_of(&SyntheticRig::conformant().scaled(100.0)),
    ]
    .concat();

    for finding in &findings {
        assert_eq!(
            finding.severity == Severity::Error,
            !finding.holds(),
            "{finding:#?}"
        );
    }
}

/// Every finding has to survive the harness, which is what enforces a named
/// space, a message, and a number that is not a NaN.
#[test]
fn every_finding_is_one_the_report_accepts() {
    let mut report = Report::new(rig::STAGE, "survivor", 1);

    for fixture in [
        findings_of(&SyntheticRig::conformant()),
        findings_of(&SyntheticRig::conformant().coincident("LeftHand", "LeftForeArm")),
        findings_for(&committed_glb(COMMITTED[0])),
        findings_for(&committed_glb(COMMITTED[1])),
    ] {
        report
            .extend(fixture)
            .expect("the harness accepts them all");
    }
    assert!(report.has_errors(), "the committed rig is in there");
    assert_eq!(report.exit_code(), 1);
}

/// Two runs of the same gates on the same art must say the same thing.
#[test]
fn the_same_rig_measures_the_same_twice() {
    let once = findings_for(&committed_glb(COMMITTED[0]));
    let twice = findings_for(&committed_glb(COMMITTED[0]));

    assert_eq!(once, twice);
}

#[test]
fn a_rule_that_holds_is_information_and_never_a_warning() {
    let findings = findings_of(&SyntheticRig::conformant());

    assert!(
        findings
            .iter()
            .all(|finding| finding.severity == Severity::Info),
        "a rig gate has nothing to warn about: it measures or it refuses"
    );
    assert!(
        findings
            .iter()
            .any(|finding| finding.comparison == Comparison::Le),
        "the angle rules read at most"
    );
}
