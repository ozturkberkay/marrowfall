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
use xtask_art::check::{Finding, Rule, Severity, rig};
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

/// A tiny valid skeleton file, for one broken line at a time. One sided pair,
/// so the mirror rows have something to reflect.
const SMALL: &str = r#"
canonical = "ours"

[conventions.ours]
hips = "Hips"
left_fin = "LeftFin"
right_fin = "RightFin"

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

    assert_eq!(table.canonical(), "meshy");
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

    for convention in ["meshy", "mixamo"] {
        let bones = table.bones(convention).unwrap();
        assert_eq!(
            bones.keys().map(String::as_str).collect::<BTreeSet<&str>>(),
            roles,
            "{convention}"
        );
    }
    assert_eq!(table.bones("meshy").unwrap()["spine_lower"], "Spine02");
    assert_eq!(table.bones("mixamo").unwrap()["spine_lower"], "Spine");
}

#[test]
fn an_unknown_convention_names_the_ones_that_exist() {
    let error = format!("{:#}", table().bones("maya").unwrap_err());

    assert!(
        error.contains("maya") && error.contains("meshy, mixamo"),
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

/// The audit's sideways `Hips`, whose own +Y points out of a hip socket
/// rather than up the spine. Every other role of both committed files sits
/// inside the band, so this rule can be made required once the rig is
/// regenerated.
#[test]
fn the_committed_rig_is_rejected_on_its_hips_and_on_nothing_else() {
    for file in COMMITTED {
        let findings = findings_for(file);

        assert_eq!(rejected(&findings), ["hips"], "{file}");
        assert!(
            (measured(&findings, "hips") - 97.801).abs() < 0.01,
            "{file} measured {}, and 97.80 degrees was measured by hand",
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
        ("head", 30.06),
        ("right_hand", 34.86),
        ("left_hand", 32.41),
        ("left_arm", 21.43),
        ("left_shoulder", 4.25),
        ("neck", 2.47),
        ("left_toe", 8.96),
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
        ["hips", "left_hand", "right_hand"],
        "the two hands join the hips outside the band"
    );
    for (role, expected) in [("left_hand", 76.15), ("right_hand", 78.22)] {
        let measured = measured(&findings, role);
        assert!(
            (measured - expected).abs() < 0.01 && measured > profile().max_bind_deviation_degrees,
            "{role} measured {measured}, expected {expected} and past the band"
        );
    }
    // The committed table is what keeps them in: 32.41 and 34.86.
    assert_eq!(rejected(&findings_for(COMMITTED[0])), ["hips"]);
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
        21,
        "twenty-one measurements inside the band, and the hips outside it"
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

/// The same rig read in the other convention, where three roles name bones
/// this rig does not have. Those roles are `rig.bone_set`'s business, so
/// there is no rest aim here to measure.
#[test]
fn a_role_whose_bone_the_rig_lacks_is_left_to_the_bone_set_rule() {
    let findings = findings_of(&SyntheticRig::conformant(), "meshy");

    assert_eq!(
        findings.len(),
        19,
        "22 roles less Spine02, Spine01 and neck"
    );
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
    let elsewhere = rig::check("fixture.gltf", &skeleton, &profile(), HEIGHT_METERS, 1);
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
