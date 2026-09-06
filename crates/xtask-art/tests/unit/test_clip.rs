//! The three clip rules that read the delivered file.
//!
//! Every case runs on the synthetic cross-rig pair in `clips.rs`, whose two
//! expected values are hand-computed rather than measured: a correct fit
//! reads 0 on both, because the rotation between the two rigs is a pure twist
//! about each bone's own axis at rest and at every frame alike. What the
//! rules read on the built file is the `f32` a GLB stores, and that is the
//! calibration `[profile.clip]` carries.

use std::collections::BTreeMap;

use glam::{DQuat, DVec3};

use xtask_art::check::aim::AimTable;
use xtask_art::check::clip::{FLOOR_SNAP, Fitted, OBJECT_TRANSFORM, RULES, SWING, Stride, TWIST};
use xtask_art::check::gltf_world::Skeleton;
use xtask_art::check::motion::{Frame, Motion, SourceLengths};
use xtask_art::check::profile::Profile;
use xtask_art::check::{Comparison, Finding, Report, Severity, gltf_clip};

use super::clips::{CrossRig, LEG_ROLL_DEGREES};
use super::rigs::{Clip, Interpolation, OBJECT_SCALE, SyntheticRig};
use super::support::repo_root;

/// The 22 roles the canonical convention maps, which is the subject count
/// both rules are pinned to.
const ROLES: usize = 22;

const ATTEMPT: u32 = 1;

/// What `library.ron` declares for the three committed Meshy clips: their key
/// times sit 1/24 s apart.
const MESHY_SOURCE_FPS: u32 = 24;

/// The committed rig, and a source sidecar that is deliberately not there.
fn beside(root: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    (
        root.join("art/staging/reports/retarget.no_such_source.1.source.json"),
        root.join("art/skeletons/humanoid.glb"),
    )
}

fn profile() -> Profile {
    Profile::of(&repo_root(), "humanoid").expect("the humanoid profile")
}

/// Role to bone, out of the committed skeleton file, so the fixture measures
/// the real role set rather than one of its own.
fn bones() -> BTreeMap<String, String> {
    let table = AimTable::of(&repo_root(), "humanoid").expect("the humanoid aim table");
    table
        .bones(table.canonical())
        .expect("the canonical convention")
        .clone()
}

/// Both rules on one cross-rig pair.
fn measure(pair: &CrossRig) -> Vec<Finding> {
    let bones = bones();
    let output = gltf_clip::read(&pair.output_glb(), &bones).expect("the output clip");
    let source = Motion::parse(&pair.source_motion()).expect("the source motion");
    xtask_art::check::clip::compare(&output, &source, &bones, &profile(), ATTEMPT)
}

/// The worst measurement one rule reports, with its subject.
fn worst<'a>(findings: &'a [Finding], rule: &str) -> &'a Finding {
    findings
        .iter()
        .filter(|finding| finding.rule == rule)
        .max_by(|a, b| a.measured.total_cmp(&b.measured))
        .unwrap_or_else(|| panic!("{rule} reported nothing"))
}

fn subjects<'a>(findings: &'a [Finding], rule: &str) -> Vec<&'a str> {
    findings
        .iter()
        .filter(|finding| finding.rule == rule)
        .map(|finding| finding.subject.as_str())
        .collect()
}

fn errors(findings: &[Finding]) -> Vec<&Finding> {
    findings
        .iter()
        .filter(|finding| finding.severity == Severity::Error)
        .collect()
}

fn correct() -> CrossRig {
    CrossRig::new(bones())
}

// --- calibration -----------------------------------------------------------

#[test]
fn a_correct_fit_across_two_rigs_is_quiet_on_both_rules() {
    let findings = measure(&correct());

    assert_eq!(errors(&findings), Vec::<&Finding>::new());
    assert_eq!(findings.len(), ROLES * 2);
}

#[test]
fn the_maths_alone_gives_the_zero_the_fixture_was_built_to_give() {
    // The known answer, without a file in the way: a pure twist about each
    // bone's own axis cannot move where the bone points, and it is the same
    // twist at rest as at every frame. Both are 0, so what is left here is
    // `f64` rounding.
    let pair = correct();
    let source = Motion::parse(&pair.source_motion()).expect("the source motion");
    let findings = xtask_art::check::clip::compare(
        &pair.output_motion(),
        &source,
        &bones(),
        &profile(),
        ATTEMPT,
    );

    assert!(worst(&findings, SWING.id).measured < 1e-9);
    assert!(worst(&findings, TWIST.id).measured < 1e-9);
}

#[test]
fn what_the_file_costs_is_the_whole_calibration() {
    // The same pair through a GLB. Measured: 4.3e-6 degrees of swing and
    // 3.5e-6 of twist, all of it the `f32` a glTF accessor stores. Pinned an
    // order tighter, so a change that costs digits is a failing test rather
    // than a widened limit.
    let findings = measure(&correct());

    assert!(
        worst(&findings, SWING.id).measured < 1e-5,
        "{:?}",
        worst(&findings, SWING.id)
    );
    assert!(
        worst(&findings, TWIST.id).measured < 1e-5,
        "{:?}",
        worst(&findings, TWIST.id)
    );
}

#[test]
fn a_pure_ten_degree_twist_splits_to_ten_and_zero() {
    // The same numeric known answer `test_transfer.py` pins on the Python
    // side, so the two implementations answer to one external number rather
    // than to each other.
    let findings = one_bone(DQuat::from_rotation_y(10.0_f64.to_radians()));

    assert!((worst(&findings, TWIST.id).measured - 10.0).abs() < 1e-9);
    assert!(worst(&findings, SWING.id).measured < 1e-9);
}

#[test]
fn a_pure_thirty_degree_swing_splits_to_thirty_and_zero() {
    let findings = one_bone(DQuat::from_rotation_x(30.0_f64.to_radians()));

    assert!((worst(&findings, SWING.id).measured - 30.0).abs() < 1e-9);
    assert!(worst(&findings, TWIST.id).measured < 1e-9);
}

/// Two clips of one bone that rest alike and differ by `by` at their one
/// frame, so both rules read `by` alone.
fn one_bone(by: DQuat) -> Vec<Finding> {
    let role = "hips";
    let bones = BTreeMap::from([(role.to_owned(), "Hips".to_owned())]);
    let motion = |rotation: DQuat| {
        Motion::new(
            BTreeMap::from([(role.to_owned(), DQuat::IDENTITY)]),
            vec![Frame {
                seconds: 0.0,
                rotations: BTreeMap::from([(role.to_owned(), rotation)]),
            }],
        )
        .expect("a one bone motion")
    };
    xtask_art::check::clip::compare(
        &motion(by),
        &motion(DQuat::IDENTITY),
        &bones,
        &profile(),
        ATTEMPT,
    )
}

#[test]
fn the_published_limits_are_the_ones_this_task_calibrated() {
    let profile = profile();

    assert_eq!(profile.clip.swing_degrees, 0.01);
    assert_eq!(profile.clip.twist_degrees, 15.0);
}

#[test]
fn the_two_rigs_really_are_a_cross_rig_pair() {
    // Without this the fixture could be one rig against itself, and both
    // rules would be quiet for the wrong reason.
    let pair = correct();
    let bones = bones();
    let output = gltf_clip::read(&pair.output_glb(), &bones).expect("the output clip");
    let source = Motion::parse(&pair.source_motion()).expect("the source motion");

    let apart = |role: &str| {
        let relative = source.rest(role).expect("a source rest").inverse()
            * output.rest(role).expect("an output rest");
        (2.0 * relative.y.atan2(relative.w).to_degrees()).abs()
    };
    assert!((apart("left_upper_leg") - LEG_ROLL_DEGREES).abs() < 1e-3);
    assert!((apart("right_leg") - LEG_ROLL_DEGREES).abs() < 1e-3);
}

// --- what a combined metric would read -------------------------------------

#[test]
fn one_combined_orientation_angle_would_reject_this_correct_fit() {
    // The reason there are two rules. A combined angle reads the legs' rest
    // roll, which a correct fit preserves, so it must fail every correct clip
    // or be widened past the defect it exists to catch.
    let pair = correct();
    let bones = bones();
    let output = gltf_clip::read(&pair.output_glb(), &bones).expect("the output clip");
    let source = Motion::parse(&pair.source_motion()).expect("the source motion");

    let out = output.frames()[0].rotations["left_upper_leg"];
    let src = source.frames()[0].rotations["left_upper_leg"];
    let combined = 2.0 * src.dot(out).abs().min(1.0).acos().to_degrees();

    assert!(
        (combined - LEG_ROLL_DEGREES).abs() < 1e-3,
        "a combined angle reads {combined} on a correct fit"
    );
    assert!(combined > profile().clip.swing_degrees);
}

#[test]
fn a_twist_read_against_the_source_s_own_rest_would_reject_it_too() {
    // The other mutation: subtracting the source's rest twist rather than the
    // roll between the two rest poses leaves the whole convention difference
    // in the number.
    let pair = correct();
    let bones = bones();
    let output = gltf_clip::read(&pair.output_glb(), &bones).expect("the output clip");
    let source = Motion::parse(&pair.source_motion()).expect("the source motion");

    let twist = |rotation: glam::DQuat| 2.0 * rotation.y.atan2(rotation.w).to_degrees();
    let out = output.frames()[0].rotations["left_upper_leg"];
    let src = source.frames()[0].rotations["left_upper_leg"];
    // A rotation and its negation are the same rotation, hence the wrap.
    let apart = (twist(out) - twist(src) + 180.0).rem_euclid(360.0) - 180.0;
    let against_the_source_rest = apart.abs();

    assert!(
        (against_the_source_rest - LEG_ROLL_DEGREES).abs() < 1e-3,
        "a twist against the source's own rest reads {against_the_source_rest}"
    );
    assert!(against_the_source_rest > profile().clip.twist_degrees);
}

// --- clip.twist ------------------------------------------------------------

#[test]
fn a_ninety_degree_twist_injected_in_the_bone_s_own_frame_is_rejected() {
    // The design's negative, `q @ Quaternion((0, 1, 0), radians(90))` on
    // `LeftUpLeg`.
    let findings = measure(&correct().rolled("left_upper_leg", 90.0));

    let twist = worst(&findings, TWIST.id);
    assert_eq!(twist.subject, "left_upper_leg");
    assert!((twist.measured - 90.0).abs() < 1e-3, "{}", twist.measured);
    assert_eq!(twist.severity, Severity::Error);
}

#[test]
fn the_same_injection_leaves_the_swing_quiet_which_is_what_proves_it_is_a_twist() {
    // A yaw would move where a downward thigh points and fire `clip.swing`
    // instead, and then the fixture would prove nothing about the twist.
    let findings = measure(&correct().rolled("left_upper_leg", 90.0));

    let swing = worst(&findings, SWING.id);
    assert!(swing.measured < profile().clip.swing_degrees, "{swing:?}");
    assert_eq!(errors(&findings).len(), 1);
}

#[test]
fn a_thigh_rolled_the_other_way_reads_the_same_size_and_is_rejected() {
    // The rule reports how far the roll is out, not which way. Without the
    // magnitude a roll of minus 90 reads as minus 90, holds against a limit
    // of 15, and files a broken clip as information.
    let findings = measure(&correct().rolled("left_upper_leg", -90.0));

    let twist = worst(&findings, TWIST.id);
    assert_eq!(twist.subject, "left_upper_leg");
    assert!((twist.measured - 90.0).abs() < 1e-3, "{}", twist.measured);
    assert_eq!(twist.severity, Severity::Error);
}

#[test]
fn a_re_rolled_thigh_just_past_the_limit_is_rejected_either_way_round() {
    // The limit is load-bearing rather than decoration: 16 degrees fails and
    // 14 holds, either side of the 15 the profile publishes, and a roll the
    // other way sits the same distance out.
    let past = measure(&correct().rolled("left_upper_leg", 16.0));
    let past_the_other_way = measure(&correct().rolled("left_upper_leg", -16.0));
    let inside = measure(&correct().rolled("right_upper_leg", 14.0));

    assert_eq!(errors(&past).len(), 1);
    assert_eq!(errors(&past_the_other_way).len(), 1);
    assert_eq!(errors(&inside).len(), 0);
}

// --- clip.swing ------------------------------------------------------------

#[test]
fn a_swing_injected_into_one_bone_is_rejected_and_names_it() {
    let findings = measure(&correct().swung("left_hand", 3.0));

    let swing = worst(&findings, SWING.id);
    assert_eq!(swing.subject, "left_hand");
    assert!((swing.measured - 3.0).abs() < 1e-3, "{}", swing.measured);
    assert_eq!(swing.severity, Severity::Error);
}

#[test]
fn a_clip_read_one_frame_out_of_the_source_is_rejected() {
    // The mutation that reads the sidecar for the wrong frame. Every bone
    // moves between two frames, so the whole clip goes red rather than one
    // bone.
    let findings = measure(&correct().source_off_by_one());

    assert!(
        errors(&findings).len() > ROLES,
        "only {} finding(s) failed",
        errors(&findings).len()
    );
    assert!(worst(&findings, SWING.id).measured > 1.0);
}

// --- alignment and gaps ----------------------------------------------------

#[test]
fn a_clip_one_frame_short_of_its_source_is_undefined_on_every_role() {
    let findings = measure(&correct().without_the_last_frame());

    assert_eq!(errors(&findings).len(), ROLES * 2);
    assert!(
        worst(&findings, SWING.id)
            .message
            .contains("7 frame(s) and the source has 8"),
        "{}",
        worst(&findings, SWING.id).message
    );
}

#[test]
fn two_clips_read_at_different_rates_are_undefined_rather_than_measured() {
    // Fact 6's shape: a 30 fps clip sampled in a 24 fps scene runs 0.8 to
    // 16.8, so the two sides drift apart frame by frame.
    let findings = measure(&correct().source_at_rate(30.0 / 24.0));

    assert_eq!(errors(&findings).len(), ROLES * 2);
    assert!(worst(&findings, TWIST.id).message.contains("tolerance"));
}

#[test]
fn a_rate_a_millionth_out_still_aligns() {
    // The tolerance is a tenth of a millisecond, so the f32 a GLB stores its
    // key times in cannot push a correct clip off its own source.
    let findings = measure(&correct().source_at_rate(1.0 + 1e-6));

    assert_eq!(errors(&findings), Vec::<&Finding>::new());
}

#[test]
fn a_role_the_source_does_not_drive_is_reported_and_never_dropped() {
    let findings = measure(&correct().source_without("left_toe"));

    assert_eq!(findings.len(), ROLES * 2);
    let named = errors(&findings);
    assert_eq!(named.len(), 2);
    assert!(named[0].message.contains("drives no left_toe"), "{named:?}");
}

#[test]
fn a_role_the_clip_has_no_bone_for_is_reported_and_never_dropped() {
    let findings = measure(&correct().output_without("right_hand"));

    assert_eq!(findings.len(), ROLES * 2);
    let named = errors(&findings);
    assert_eq!(named.len(), 2);
    assert!(
        named[0].message.contains("no bone for the right_hand role"),
        "{named:?}"
    );
}

#[test]
fn a_bone_pointing_exactly_opposite_the_source_reports_the_singular_case() {
    // Half a turn of swing is the one place a twist does not exist. A gate
    // cannot raise, so it says so and never returns a NaN.
    let findings = measure(&correct().swung("left_foot", 180.0));

    let twist = findings
        .iter()
        .find(|finding| finding.rule == TWIST.id && finding.subject == "left_foot")
        .expect("a left_foot twist");
    assert_eq!(twist.severity, Severity::Error);
    assert!(twist.measured.is_finite());
    assert!(
        twist
            .message
            .contains("swing is 180.000 degrees, twist undefined"),
        "{}",
        twist.message
    );
}

#[test]
fn every_subject_is_reported_even_when_the_measurement_holds() {
    let findings = measure(&correct());

    for rule in [SWING.id, TWIST.id] {
        let reported = subjects(&findings, rule);
        assert_eq!(reported.len(), ROLES);
        assert!(reported.contains(&"left_toe"), "terminals included");
        assert!(reported.contains(&"right_hand"), "terminals included");
    }
    assert!(
        findings
            .iter()
            .all(|finding| finding.severity == Severity::Info)
    );
}

#[test]
fn no_finding_is_ever_a_nan() {
    for pair in [
        correct(),
        correct().swung("left_foot", 180.0),
        correct().source_without("hips"),
        correct().without_the_last_frame(),
    ] {
        for finding in measure(&pair) {
            assert!(finding.measured.is_finite(), "{finding:?}");
            assert!(finding.limit.is_finite(), "{finding:?}");
        }
    }
}

// --- clip.object_transform -------------------------------------------------

#[test]
fn a_clip_that_carries_the_rig_s_own_object_transform_holds() {
    let findings = object_transform(&correct());

    assert_eq!(errors(&findings), Vec::<&Finding>::new());
    assert_eq!(subjects(&findings, OBJECT_TRANSFORM.id), ["Armature"]);
}

#[test]
fn a_clip_whose_object_scale_was_applied_is_rejected() {
    // What `transform_apply(scale=True)` leaves: the 0.01 moves out of the
    // object and into the rest geometry, and every location key keeps its
    // bytes while its meaning changes 100x.
    let findings = object_transform(&correct().with_the_object_scale_applied());

    assert_eq!(errors(&findings).len(), 1);
    assert!(
        errors(&findings)[0].message.contains("where the rig has"),
        "{:?}",
        errors(&findings)[0]
    );
}

#[test]
fn a_clip_whose_armature_object_is_animated_is_rejected() {
    // What the static reading cannot see: an action on the object moves the
    // whole clip while every node's own transform still matches the rig's.
    let findings = object_transform(&correct().with_an_animated_object_node());

    assert_eq!(errors(&findings).len(), 1);
    assert_eq!(errors(&findings)[0].subject, "Armature");
    assert!(
        errors(&findings)[0].message.contains("drive Armature"),
        "{:?}",
        errors(&findings)[0]
    );
}

#[test]
fn the_committed_clip_reports_the_armature_and_the_skin_carrier_and_nothing_else() {
    // The subject list is pinned. `strip_animation.py` weights a tiny
    // triangle to the root bone so the armature exports at all, so the
    // carrier is a node the file really has and a subject on purpose.
    let findings = xtask_art::check::clip::object_transform(
        &Skeleton::read(&crate::support::committed_glb("art/animations/run.glb")).expect("a clip"),
        &Skeleton::read(&crate::support::committed_glb("art/skeletons/humanoid.glb"))
            .expect("the rig"),
        &profile(),
        ATTEMPT,
    );

    assert_eq!(
        subjects(&findings, OBJECT_TRANSFORM.id),
        ["Armature", "skin_carrier"]
    );
    assert_eq!(errors(&findings), Vec::<&Finding>::new());
}

/// `clip.object_transform` on one pair, against the conformant rig as the
/// committed one.
fn object_transform(pair: &CrossRig) -> Vec<Finding> {
    let rig = SyntheticRig::conformant().to_gltf();
    xtask_art::check::clip::object_transform(
        &Skeleton::from_slice(&pair.output_glb()).expect("the clip"),
        &Skeleton::from_slice(rig.as_bytes()).expect("the rig"),
        &profile(),
        ATTEMPT,
    )
}

// --- the printed rule list -------------------------------------------------

/// `--list-rules` prints the registry, and a finding is built through it, so
/// neither can advertise a limit, a unit or a space the other does not carry.
#[test]
fn a_finding_carries_exactly_what_the_rule_list_advertises() {
    let profile = profile();
    let findings = [
        measure(&correct()),
        measure(&correct().rolled("left_upper_leg", 90.0)),
        object_transform(&correct()),
    ]
    .concat();

    for finding in &findings {
        let rule = RULES
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
        measure(&correct()),
        measure(&correct().swung("left_hand", 3.0)),
        object_transform(&correct().with_the_object_scale_applied()),
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

#[test]
fn a_report_that_loosens_a_clip_limit_is_refused() {
    // The mutation the runner has to see: the same rule id with a limit
    // nobody published, which is how a wide threshold would slip in.
    let mut finding = worst(&measure(&correct()), SWING.id).clone();
    finding.limit = 90.0;
    let mut report = Report::new("retarget", "run", ATTEMPT);
    report.add(finding).expect("a well formed finding");

    let off = report.off_registry(&profile());

    assert_eq!(off.len(), 1);
    assert!(off[0].contains("limit"), "{off:?}");
}

#[test]
fn a_report_that_flips_a_clip_comparison_is_refused() {
    let mut finding = worst(&measure(&correct()), TWIST.id).clone();
    finding.comparison = Comparison::Ge;
    let mut report = Report::new("retarget", "run", ATTEMPT);
    report.add(finding).expect("a well formed finding");

    let off = report.off_registry(&profile());

    assert_eq!(off.len(), 1);
    assert!(off[0].contains("comparison"), "{off:?}");
}

#[test]
fn two_bind_poses_pointing_opposite_leave_the_swing_measurable() {
    // The other half of the singular case, and the reason the two rules are
    // reported separately: with no roll between the two bind poses there is
    // no twist to read, and where the bone points is still a real number.
    let findings = measure(&correct().source_resting_opposite("right_foot"));

    let named = |rule: &str| {
        findings
            .iter()
            .find(|finding| finding.rule == rule && finding.subject == "right_foot")
            .unwrap_or_else(|| panic!("{rule} reported nothing on right_foot"))
    };
    assert_eq!(named(SWING.id).severity, Severity::Info);
    assert!(named(SWING.id).measured < profile().clip.swing_degrees);
    assert_eq!(named(TWIST.id).severity, Severity::Error);
    assert!(
        named(TWIST.id).message.contains("at rest"),
        "{}",
        named(TWIST.id).message
    );
}

// --- what each reader refuses ----------------------------------------------

/// The error a reader gives, so a test can name the reason rather than the
/// fact that something went wrong.
fn refused(bytes: &[u8]) -> String {
    format!(
        "{:#}",
        gltf_clip::read(bytes, &bones()).expect_err("this file cannot be read")
    )
}

#[test]
fn a_file_with_no_skin_holds_no_skeleton_to_measure() {
    let error = refused(&crate::meshes::a_scene_with_no_mesh());

    assert!(error.contains("no skin"), "got: {error}");
}

#[test]
fn a_rig_with_no_animation_holds_no_clip() {
    let error = refused(SyntheticRig::conformant().to_gltf().as_bytes());

    assert!(error.contains("no animation"), "got: {error}");
}

#[test]
fn a_cubic_sampler_is_refused_rather_than_read_as_a_straight_line() {
    // A cubic sampler stores two tangents beside every value, so its output
    // is three times as long and reading it as `LINEAR` would measure the
    // tangents as poses.
    let error = refused(&correct().with_cubic_keys().output_glb());

    assert!(error.contains("CUBICSPLINE"), "got: {error}");
}

#[test]
fn a_key_that_is_no_rotation_is_refused_rather_than_normalized_into_a_nan() {
    let error = refused(&correct().with_a_key_that_is_no_rotation().output_glb());

    assert!(error.contains("no rotation at all"), "got: {error}");
}

#[test]
fn a_source_motion_that_is_not_a_rotation_is_refused() {
    let error = format!(
        "{:#}",
        Motion::parse(
            r#"{"rest": {"hips": [0.0, 0.0, 0.0, 0.0]}, "travel": 0.0,
                "stride_segment": 0.4,
                "frames": [{"seconds": 0.0, "rotations": {"hips": [1.0, 0.0, 0.0, 0.0]}}]}"#
        )
        .expect_err("a quaternion of no length")
    );

    assert!(error.contains("no rotation at all"), "got: {error}");
}

#[test]
fn a_source_motion_with_no_frame_is_refused() {
    let error = format!(
        "{:#}",
        Motion::parse(
            r#"{"rest": {"hips": [1.0, 0.0, 0.0, 0.0]}, "frames": [],
                "travel": 0.0, "stride_segment": 0.4}"#
        )
        .expect_err("a clip with no frame")
    );

    assert!(error.contains("no frame"), "got: {error}");
}

#[test]
fn a_source_motion_whose_frame_leaves_a_role_out_is_refused() {
    let error = format!(
        "{:#}",
        Motion::parse(
            r#"{"rest": {"hips": [1.0, 0.0, 0.0, 0.0], "head": [1.0, 0.0, 0.0, 0.0]},
                "travel": 0.0, "stride_segment": 0.4,
                "frames": [{"seconds": 0.0, "rotations": {"hips": [1.0, 0.0, 0.0, 0.0]}}]}"#
        )
        .expect_err("a frame with a hole in it")
    );

    assert!(error.contains("rest pose carries"), "got: {error}");
}

#[test]
fn a_source_motion_that_runs_backwards_is_refused() {
    let error = format!(
        "{:#}",
        Motion::parse(
            r#"{"rest": {"hips": [1.0, 0.0, 0.0, 0.0]}, "travel": 0.0,
                "stride_segment": 0.4, "frames": [
                 {"seconds": 1.0, "rotations": {"hips": [1.0, 0.0, 0.0, 0.0]}},
                 {"seconds": 0.0, "rotations": {"hips": [1.0, 0.0, 0.0, 0.0]}}]}"#
        )
        .expect_err("frames out of order")
    );

    assert!(error.contains("backwards"), "got: {error}");
}

/// The sidecar's own two lengths, refused before either rule divides by one.
#[test]
fn a_source_motion_whose_stride_segment_is_zero_is_refused() {
    let error = format!(
        "{:#}",
        SourceLengths::parse(r#"{"rest": {}, "frames": [], "travel": 1.0, "stride_segment": 0.0}"#)
            .expect_err("a femur of no length")
    );

    assert!(error.contains("no ratio can be taken"), "got: {error}");
}

#[test]
fn a_source_motion_that_travels_a_negative_distance_is_refused() {
    let error = format!(
        "{:#}",
        SourceLengths::parse(
            r#"{"rest": {}, "frames": [], "travel": -1.0, "stride_segment": 0.4}"#
        )
        .expect_err("a travel of less than nothing")
    );

    assert!(error.contains("no distance at all"), "got: {error}");
}

#[test]
fn a_clip_that_is_not_on_disk_is_eleven_errors_and_never_a_skip() {
    let root = repo_root();
    let (sidecar, rig) = beside(&root);
    let findings = xtask_art::check::clip::check_files(
        &Fitted {
            output: &root.join("art/animations/local/never_fetched.glb"),
            source_motion: &sidecar,
            rig: &rig,
            repo_root: &root,
            source_fps: MESHY_SOURCE_FPS,
            loops: true,
            travels: false,
        },
        &profile(),
        &AimTable::of(&root, "humanoid").expect("the humanoid aim table"),
        ATTEMPT,
    )
    .expect("the gate itself runs");

    assert_eq!(errors(&findings).len(), 11);
    assert!(
        findings[0]
            .message
            .contains("never_fetched.glb cannot be read"),
        "{:?}",
        findings[0]
    );
}

#[test]
fn a_clip_on_disk_with_no_source_motion_beside_it_is_still_reported() {
    let root = repo_root();
    let (sidecar, rig) = beside(&root);
    let findings = xtask_art::check::clip::check_files(
        &Fitted {
            output: &crate::support::committed_glb("art/animations/run.glb"),
            source_motion: &sidecar,
            rig: &rig,
            repo_root: &root,
            source_fps: MESHY_SOURCE_FPS,
            loops: true,
            travels: false,
        },
        &profile(),
        &AimTable::of(&root, "humanoid").expect("the humanoid aim table"),
        ATTEMPT,
    )
    .expect("the gate itself runs");

    // Four rules cannot run: the two that compare against the source, and
    // the two that size the fit against the travel it had. The object
    // transform still can, because it needs no source at all and the
    // committed clip holds the same armature and skin carrier the committed
    // rig does. So does the floor: `run.glb` puts its lowest toe 0.0017 m
    // under where the rig itself rests, inside the 5 mm the profile
    // publishes.
    assert_eq!(errors(&findings).len(), 4);
    assert_eq!(
        subjects(&findings, OBJECT_TRANSFORM.id),
        ["Armature", "skin_carrier"]
    );
    assert_eq!(
        subjects(&findings, FLOOR_SNAP.id),
        ["RightToeBase at 0.083333 s"]
    );
}

/// Every file-side rule reports on a real fit, or one that measures nothing
/// cannot be told from one that was never called. `check_files` is where the
/// Rust half of the retarget's report comes from, so this is its own
/// `refuse_unread_rules`.
#[test]
fn every_file_side_rule_reports_on_the_synthetic_pair() {
    let root = repo_root();
    let pair = CrossRig::new(bones());
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (clip, sidecar) = (dir.path().join("fit.glb"), dir.path().join("source.json"));
    std::fs::write(&clip, pair.output_glb()).unwrap();
    std::fs::write(&sidecar, pair.source_motion()).unwrap();

    let findings = xtask_art::check::clip::check_files(
        &Fitted {
            output: &clip,
            source_motion: &sidecar,
            rig: &crate::support::committed_glb("art/skeletons/humanoid.glb"),
            repo_root: &root,
            source_fps: MESHY_SOURCE_FPS,
            loops: false,
            travels: true,
        },
        &profile(),
        &AimTable::of(&root, "humanoid").expect("the humanoid aim table"),
        ATTEMPT,
    )
    .expect("the gate itself runs");

    for rule in xtask_art::check::clip::FILE_RULES {
        assert!(
            findings.iter().any(|finding| finding.rule == rule.id),
            "{} reported nothing at all",
            rule.id
        );
    }
}

// --- the frame grid and the loop -----------------------------------------

/// One clip's key grid, read out of a committed GLB.
fn keys_of(name: &str) -> gltf_clip::Keys {
    let bytes = std::fs::read(crate::support::committed_glb(&format!(
        "art/animations/{name}.glb"
    )))
    .expect("a committed clip");
    gltf_clip::keys(&bytes).expect("its key grid")
}

/// The largest measurement in a set of findings, all of one rule.
fn largest(findings: &[Finding]) -> f64 {
    findings
        .iter()
        .map(|finding| finding.measured)
        .fold(0.0, f64::max)
}

/// Requirement 3, on real art: every key of every committed clip lands on a
/// whole frame of the rate `library.ron` declares for it.
#[test]
fn every_committed_clip_sits_on_the_grid_its_own_rate_sets() {
    for name in ["idle", "run", "walk_back"] {
        let keys = keys_of(name);
        let findings =
            xtask_art::check::clip::fps_grid(&keys, MESHY_SOURCE_FPS, &profile(), ATTEMPT);

        assert_eq!(findings.len(), keys.seconds.len(), "one per key of {name}");
        // The `f32` a GLB stores key times in, and nothing else: 9.5e-7 of a
        // frame against a limit of 1e-4.
        assert!(largest(&findings) < 1e-6, "{name}: {}", largest(&findings));
        assert!(errors(&findings).is_empty(), "{name}");
    }
}

/// And the negative: the same clip read on another rate's grid. This is the
/// shipped `strafe_left.glb` in a 24 fps scene, 0.8 to 16.8, in the one form
/// CI can hold: `art/animations/local/` may not be redistributed.
#[test]
fn a_clip_read_on_another_rate_s_grid_is_rejected_key_by_key() {
    let keys = keys_of("run");
    let findings = xtask_art::check::clip::fps_grid(&keys, 30, &profile(), ATTEMPT);

    assert_eq!(errors(&findings).len(), 15, "of 21 keys, 6 land whole");
    assert!(
        (largest(&findings) - 0.5).abs() < 1e-5,
        "a 24 fps clip read at 30 drifts a quarter frame per key: {}",
        largest(&findings)
    );
}

/// A rate of zero is no grid at all, and a gate never emits a NaN.
#[test]
fn a_declared_rate_of_zero_is_undefined_rather_than_infinite() {
    let findings = xtask_art::check::clip::fps_grid(&keys_of("run"), 0, &profile(), ATTEMPT);

    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].unit, "undefined measurements");
}

/// The message a human reads, whole. Pinned because a format string is the
/// one place a stray run of spaces survives every other test.
#[test]
fn a_key_off_the_grid_says_so_in_one_readable_sentence() {
    let findings = xtask_art::check::clip::fps_grid(&keys_of("run"), 30, &profile(), ATTEMPT);

    assert_eq!(
        findings[1].message,
        "0.041667 s is frame 1.2500 at 30 fps, 0.250000 frames off a whole one"
    );
}

/// `clip.loop`'s calibration, on the two committed clips that do loop.
#[test]
fn the_two_committed_loops_come_back_round() {
    for (name, expected) in [("idle", 0.487), ("run", 0.000)] {
        let findings =
            xtask_art::check::clip::closes_the_loop(&keys_of(name), true, &profile(), ATTEMPT);

        assert_eq!(findings.len(), 24, "one per joint of {name}");
        assert!(
            (largest(&findings) - expected).abs() < 0.001,
            "{name} reads {}",
            largest(&findings)
        );
        assert!(errors(&findings).is_empty(), "{name}");
    }
}

/// The `[art]` negative, which `library.ron` has documented all along:
/// `walk_back` does not return to its start pose, so it hitches once a loop.
#[test]
fn the_committed_clip_that_does_not_close_its_loop_is_rejected() {
    let findings =
        xtask_art::check::clip::closes_the_loop(&keys_of("walk_back"), true, &profile(), ATTEMPT);

    let defects = errors(&findings);
    assert_eq!(defects.len(), 9, "nine bones end somewhere else");
    assert!(
        (largest(&findings) - 6.910).abs() < 0.001,
        "worst reads {}",
        largest(&findings)
    );
    assert_eq!(worst(&findings, "clip.loop").subject, "LeftForeArm");
}

/// And the `[synth]` one: a clip cut one frame short of its own cycle.
#[test]
fn a_clip_cut_one_frame_short_no_longer_closes() {
    let whole = correct().looping();
    let short = correct().looping().without_the_last_frame();
    let read = |pair: &CrossRig| {
        let glb = pair.output_glb();
        let keys = gltf_clip::keys(&glb).expect("the fixture's key grid");
        xtask_art::check::clip::closes_the_loop(&keys, true, &profile(), ATTEMPT)
    };

    assert!(errors(&read(&whole)).is_empty(), "the whole cycle closes");
    assert!(
        !errors(&read(&short)).is_empty(),
        "one frame short does not"
    );
}

/// A clip the library does not call a loop has no reason for its two ends to
/// agree, so every joint reports `skipped` rather than a number nobody reads.
#[test]
fn a_clip_that_does_not_repeat_reports_every_joint_as_skipped() {
    let findings =
        xtask_art::check::clip::closes_the_loop(&keys_of("walk_back"), false, &profile(), ATTEMPT);

    assert_eq!(findings.len(), 24);
    assert!(
        findings
            .iter()
            .all(|finding| finding.severity == Severity::Skipped)
    );
    assert!(findings[0].message.contains("does not repeat"));
}

// --- the floor, on the delivered file --------------------------------------

/// The conformant rig held at rest, with its root keyed `meters` of world
/// height below where the file says that rig rests.
///
/// Keyed and not nudged: a rig moved at rest takes its own floor with it, and
/// a fixture that moves both sides of a comparison cancels the rule it is
/// meant to prove. What the retarget's snap moves is the root's location
/// keys, so that is what this moves.
fn a_clip_off_the_floor(meters: f64) -> Vec<u8> {
    let rig = SyntheticRig::conformant();
    // The object node above the skeleton carries a 0.01 scale, so a local
    // step is 100x its world one.
    let sunk = rig.rest_translation("Hips") + DVec3::new(0.0, meters / OBJECT_SCALE, 0.0);
    rig.to_glb(&Clip {
        seconds: vec![0.0, 1.0],
        rotations: BTreeMap::from([("Hips".to_owned(), vec![DQuat::IDENTITY; 2])]),
        translations: BTreeMap::from([("Hips".to_owned(), vec![sunk; 2])]),
        interpolation: Interpolation::Linear,
    })
}

/// The two toe bones the committed skeleton names as its ground roles.
fn ground_bones() -> std::collections::BTreeSet<String> {
    let table = AimTable::of(&repo_root(), "humanoid").expect("the humanoid aim table");
    let bones = bones();
    table
        .ground_roles()
        .iter()
        .map(|role| bones[role].clone())
        .collect()
}

fn floor_of(bytes: &[u8]) -> Finding {
    let ground = gltf_clip::ground(bytes, &ground_bones()).expect("a readable clip");
    xtask_art::check::clip::floor_snap(&ground, "the clip", &profile(), ATTEMPT)
}

/// The calibration: a rig whose toes rest exactly on the floor reads 4e-9 m
/// out of the file, which is the `f32` a GLB stores a joint position in.
#[test]
fn a_clip_whose_lowest_toe_sits_on_the_floor_holds() {
    let sits = floor_of(&a_clip_off_the_floor(0.0));

    assert_eq!(sits.severity, Severity::Info, "{sits:?}");
    assert!(sits.measured < 1e-8, "{sits:?}");
    assert_eq!(sits.subject, "LeftToeBase at 0.000000 s");
}

/// The calibration pair, either side of the published 5 mm.
#[test]
fn four_millimeters_of_toe_under_the_floor_holds_and_six_does_not() {
    let inside = floor_of(&a_clip_off_the_floor(-0.004));
    let outside = floor_of(&a_clip_off_the_floor(-0.006));

    assert_eq!(inside.severity, Severity::Info, "{inside:?}");
    assert!((inside.measured - 0.004).abs() < 1e-6, "{inside:?}");
    assert_eq!(outside.severity, Severity::Error, "{outside:?}");
    assert!((outside.measured - 0.006).abs() < 1e-6, "{outside:?}");
}

#[test]
fn a_clip_with_the_snap_step_removed_is_rejected_by_the_file_it_delivered() {
    let sunk = floor_of(&a_clip_off_the_floor(-0.06));

    assert_eq!(sunk.severity, Severity::Error, "{sunk:?}");
    assert!((sunk.measured - 0.06).abs() < 1e-6, "{sunk:?}");
    assert_eq!(
        sunk.message,
        "LeftToeBase at 0.000000 s is the lowest any ground joint of the clip \
         gets, -0.0600 m from the rest height the snap aims at"
    );
}

/// The `[art]` negative: `idle.glb` was fitted before the snap existed, and
/// its lowest toe hangs **0.0773 m** above the height its own rig rests at.
/// A refit through the retarget brings it to 9.3e-9 m, so this reading is
/// the clip and not the rule.
#[test]
fn the_committed_clip_that_was_never_snapped_is_rejected() {
    let bytes = std::fs::read(crate::support::committed_glb("art/animations/idle.glb"))
        .expect("a committed clip");
    let ground = gltf_clip::ground(&bytes, &ground_bones()).expect("a readable clip");

    let floats = xtask_art::check::clip::floor_snap(&ground, "idle", &profile(), ATTEMPT);

    assert_eq!(floats.severity, Severity::Error, "{floats:?}");
    assert!((floats.measured - 0.0773).abs() < 0.0001, "{floats:?}");
    assert_eq!(floats.subject, "RightToeBase at 1.208333 s");
}

/// A clip whose rig has no toe at all measures nothing, and says so as an
/// error rather than reporting a floor it never found.
#[test]
fn a_clip_that_carries_no_toe_is_undefined_rather_than_on_the_floor() {
    let bytes = a_clip_off_the_floor(0.0);
    let ground =
        gltf_clip::ground(&bytes, &std::collections::BTreeSet::new()).expect("a readable clip");

    let nothing = xtask_art::check::clip::floor_snap(&ground, "the clip", &profile(), ATTEMPT);

    assert_eq!(nothing.severity, Severity::Error);
    assert_eq!(nothing.unit, "undefined measurements");
    assert!(nothing.message.contains("no ground joint"), "{nothing:?}");
}

/// The cross-rig fixture stands on its own floor, and the two sides of that
/// agree: the fixture composes the pose itself to decide the lift, and the
/// rule reads the file it wrote. Neither uses the other's arithmetic.
#[test]
fn the_cross_rig_fixture_stands_where_its_own_rig_rests() {
    let pair = CrossRig::new(bones());

    let sits = floor_of(&pair.output_glb());

    assert_eq!(sits.severity, Severity::Info, "{sits:?}");
    assert!(sits.measured < 1e-6, "{sits:?}");
}

/// The floor is where this rig's own rest pose stands, and two files say so
/// to a reader who never opens the design: the skeleton file's comment on
/// `ground_roles` and `skeleton.py`'s docstring for the same field.
///
/// A lint because the wrong datum is the plausible one. Snapping the toe
/// joint itself to zero sinks every clip 30.7 mm and rejects the committed
/// `run.glb`, and a comment saying so would send the next reader to fix the
/// gate instead of the comment.
#[test]
fn no_authoritative_file_says_the_floor_snap_puts_a_toe_at_zero() {
    let root = repo_root();

    for path in [
        "art/skeletons/humanoid.toml",
        "tools/blender/src/skeleton.py",
    ] {
        let text = std::fs::read_to_string(root.join(path)).expect("a readable file");
        assert!(
            !text.contains("Z = 0"),
            "{path} states the datum the design measured wrong: the lowest \
             ground joint stands where this rig's own rest pose stands"
        );
    }
}

// --- the travel, on the delivered file ------------------------------------

/// Both sides of the pair, gathered the way `check_files` gathers them: our
/// own femur off the rig, and the vendor's two lengths out of the sidecar the
/// fixture wrote. Nothing here restates a number the fixture owns.
fn stride_of(pair: &CrossRig) -> Stride {
    let source = SourceLengths::parse(&pair.source_motion()).expect("the sidecar");
    Stride {
        travel: gltf_clip::travel(&pair.output_glb(), "Hips").expect("a readable root"),
        segment: (pair.femur(), source.stride_segment),
        source_travel: source.travel,
        travels: true,
    }
}

fn sized(pair: &CrossRig) -> (Finding, Finding) {
    let reading = stride_of(pair);
    (
        xtask_art::check::clip::stride(&reading, "the clip", &profile(), ATTEMPT),
        xtask_art::check::clip::stride_ratio(&reading, "the clip", &profile(), ATTEMPT),
    )
}

/// The calibration: a correct fit travels what its source did, sized, and the
/// reading is the `f32` a GLB stores a joint position in.
#[test]
fn a_fit_that_travels_what_the_femur_asks_for_holds() {
    let (travel, ratio) = sized(&CrossRig::new(bones()));

    assert_eq!(travel.severity, Severity::Info, "{travel:?}");
    assert!(travel.measured < 1e-4, "{travel:?}");
    assert_eq!(ratio.severity, Severity::Info, "{ratio:?}");
    assert!((ratio.measured - 0.8815).abs() < 1e-4, "{ratio:?}");
}

/// The negative: the same fit with its root keys five percent too far, which
/// is what sizing every length by the wrong femur leaves.
#[test]
fn a_fit_whose_root_keys_are_scaled_five_percent_is_rejected() {
    let (travel, _) = sized(&CrossRig::new(bones()).travel_sized_by(1.05));

    assert_eq!(travel.severity, Severity::Error, "{travel:?}");
    assert!((travel.measured - 5.0).abs() < 0.01, "{travel:?}");
    assert_eq!(
        travel.message,
        "the clip travels 2.1397 m against the 2.0378 m its source's \
         2.3117 m comes to at a femur ratio of 0.8815"
    );
}

/// A clip the library declares in place has no source travel to be sized,
/// so the declaration switches the rule off rather than dividing by nothing.
#[test]
fn a_clip_the_library_declares_in_place_is_skipped_on_its_flag() {
    let reading = Stride {
        travels: false,
        ..stride_of(&CrossRig::new(bones()))
    };

    let travel = xtask_art::check::clip::stride(&reading, "the clip", &profile(), ATTEMPT);

    assert_eq!(travel.severity, Severity::Skipped, "{travel:?}");
    assert!(travel.message.contains("travels: false"), "{travel:?}");
}

/// `travels: true` on a source that never moved leaves no travel to take a
/// ratio of, and a relative difference against zero is not a number.
#[test]
fn a_traveling_clip_whose_source_stands_still_is_undefined() {
    let reading = Stride {
        source_travel: 0.0,
        ..stride_of(&CrossRig::new(bones()))
    };

    let travel = xtask_art::check::clip::stride(&reading, "the clip", &profile(), ATTEMPT);

    assert_eq!(travel.severity, Severity::Error, "{travel:?}");
    assert_eq!(travel.unit, "undefined measurements");
}

/// And a rig whose two stride joints sit on top of each other has no ratio
/// at all, which both rules report rather than dividing by nothing.
#[test]
fn a_rig_with_no_stride_segment_is_undefined_on_both_rules() {
    let pair = CrossRig::new(bones());
    let reading = Stride {
        segment: (0.0, pair.femur()),
        ..stride_of(&pair)
    };

    let travel = xtask_art::check::clip::stride(&reading, "the clip", &profile(), ATTEMPT);
    let ratio = xtask_art::check::clip::stride_ratio(&reading, "the clip", &profile(), ATTEMPT);

    for finding in [&travel, &ratio] {
        assert_eq!(finding.severity, Severity::Error, "{finding:?}");
        assert_eq!(finding.unit, "undefined measurements");
    }
    assert!(ratio.message.contains("is no ratio"), "{ratio:?}");
}
