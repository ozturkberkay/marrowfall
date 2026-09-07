//! The fifteen mesh gates: thirteen read off one file, two off the pair the
//! fixer produced.
//!
//! Every rule here has three columns, per the design's test plan: a positive
//! fixture, a negative fixture it must reject, and a calibration that proves
//! it stays quiet on art it should accept.
//!
//! The calibration asset is `art/characters/survivor/model.glb`. Every
//! `[profile.mesh]` limit is set from the real bare mesh, which is derived
//! and not committed, so what this file can hold is the reading beside the
//! rule that reads it.
//!
//! The negatives are synthetic, because a synthetic mesh is the only way to
//! carry a known defect count above a limit calibrated on real art. The one
//! `[mut]` fixture takes a step of the measurement out instead, which tests
//! the metric rather than the gate.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use glam::DQuat;
use xtask_art::check::gltf_mesh::{Surface, WELD_METERS};
use xtask_art::check::profile::Profile;
use xtask_art::check::{Comparison, Finding, Report, Severity, Symmetry, mesh};
use xtask_art::library::HUMANOID;

use crate::meshes::{ALLOWED, HEIGHT_METERS, SyntheticMesh};
use crate::support::{committed_glb, repo_root};

/// The rigged survivor, which is the calibration asset for every rule that
/// rigging did not change.
const CALIBRATION: &str = "art/characters/survivor/model.glb";

/// What the real bare mesh measures, from
/// `art/staging/reports/mesh.survivor-unset.1.json`: the height Meshy chose,
/// and how far that is from the 1.700 the spec asked for.
const BARE_METERS: f64 = 1.8970;
const BARE_PERCENT: f64 = 11.588;

fn profile() -> Profile {
    Profile::of(&repo_root(), HUMANOID).expect("the committed humanoid profile")
}

fn under_repo(file: &str) -> PathBuf {
    repo_root().join(file)
}

/// Every finding for one file on disk, with no `print/analyze` response,
/// which is the state of every run today.
fn findings_for(file: &Path) -> Vec<Finding> {
    findings_with(file, None)
}

/// The same, with a recorded response for the remote rule to read.
fn findings_with(file: &Path, printability: Option<&str>) -> Vec<Finding> {
    mesh::check_file(
        file,
        &repo_root(),
        &profile(),
        HEIGHT_METERS,
        Symmetry::Enforced,
        printability,
        1,
    )
    .expect("the mesh rules run without a Blender")
}

/// Every finding for a hand-built mesh.
fn findings_of(fixture: &SyntheticMesh) -> Vec<Finding> {
    let surface = Surface::from_slice(&fixture.to_glb()).expect("a readable fixture");
    mesh::check(
        "fixture.glb",
        &surface,
        &profile(),
        HEIGHT_METERS,
        Symmetry::Enforced,
        None,
        1,
    )
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

fn finding<'a>(findings: &'a [Finding], rule: &str) -> &'a Finding {
    findings
        .iter()
        .find(|finding| finding.rule == rule)
        .unwrap_or_else(|| panic!("{rule} reported nothing"))
}

/// The units a finding carries when there was nothing to measure. Each one
/// replaces the rule's own unit, comparison and limit, and says why.
const NOT_A_MEASUREMENT: [&str; 2] = ["undefined measurements", "unavailable calls"];

/// What each rule measures on a fixture, so a rule cannot quietly stop
/// measuring a subject that passes.
const SUBJECTS: [(&str, usize); 13] = [
    ("mesh.holes", 1),
    ("mesh.non_manifold", 1),
    ("mesh.islands", 1),
    ("mesh.self_intersect", 1),
    ("mesh.mirror", 1),
    ("mesh.world_size", 1),
    ("mesh.facing", 1),
    ("mesh.stray_object", 1),
    ("mesh.budget", 1),
    ("mesh.uv", 1),
    ("mesh.texture", 1),
    ("mesh.quads", 1),
    ("mesh.printability", 1),
];

// --- the calibration ------------------------------------------------------

/// The whole calibration, as one table of numbers.
///
/// These are the measurements the published limits were set from. If one
/// moves, the limit beside it in `[profile.mesh]` is stale and the design's
/// table is wrong.
#[test]
fn the_calibration_asset_measures_exactly_what_the_limits_were_set_from() {
    let findings = findings_for(&committed_glb(CALIBRATION));

    for (rule, expected) in [
        ("mesh.holes", 18.0),
        ("mesh.non_manifold", 17.0),
        ("mesh.islands", 1.0),
        ("mesh.self_intersect", 1113.0),
        ("mesh.budget", 55_533.0),
        ("mesh.facing", 0.0),
    ] {
        assert_eq!(measured(&findings, rule, CALIBRATION), expected, "{rule}");
    }
    for (rule, subject, expected) in [
        ("mesh.stray_object", ALLOWED, 0.0),
        ("mesh.texture", ALLOWED, 0.0),
        ("mesh.uv", ALLOWED, 0.0),
        ("mesh.quads", ALLOWED, 0.0),
    ] {
        assert_eq!(measured(&findings, rule, subject), expected, "{rule}");
    }
    // The fixer mirrors this mesh, so the width it is off by is the `f32` a
    // GLB stores a vertex in. The bare mesh it was made from reads 3.257.
    let mirror = measured(&findings, "mesh.mirror", CALIBRATION);
    assert!(
        mirror < 1e-4,
        "the worst mirror distance is {mirror} percent of width"
    );
    // 1.70000 m against the spec's 1.700, which is the mesh and not the
    // skeleton: the joints span 1.6688 m.
    let height = measured(&findings, "mesh.world_size", CALIBRATION);
    assert!(height < 1e-4, "the mesh is {height} percent off 1.70 m");
}

/// The facing is a choice of six axes, so the pass alone would hold with the
/// foot slice set to anything up to half the body. What makes the slice a
/// measurement is the margin: the message carries the angle, and on the
/// calibration asset it is 0.8 degrees off +Z, not 44.
#[test]
fn the_calibration_assets_feet_point_forward_with_room_to_spare() {
    let findings = findings_for(&committed_glb(CALIBRATION));
    let facing = finding(&findings, "mesh.facing");

    assert!(
        facing.message.contains("+z of the body's center"),
        "{facing:#?}"
    );
    let degrees: f64 = facing
        .message
        .split_whitespace()
        .find_map(|word| word.parse().ok())
        .expect("the message states the angle");
    assert!(
        degrees < 5.0,
        "the feet sit {degrees} degrees off the declared axis, and the rule accepts anything under 45, so a wider slice would not be caught"
    );
}

/// Silence on known-good art is the other half of the contract, and the one
/// that lets these gates become required checks.
///
/// Through `CLEANED_RULES`, because this file is what rigging returned of the
/// file the fixer wrote: `mesh.non_manifold`'s ceiling is calibrated before
/// the fixer, and filling a hole raises that count on purpose.
#[test]
fn the_calibration_asset_breaks_no_rule() {
    let findings = mesh::only(
        &mesh::CLEANED_RULES,
        findings_for(&committed_glb(CALIBRATION)),
    );

    assert_eq!(broken(&findings), Vec::<String>::new(), "{findings:#?}");
}

/// The headroom each published limit leaves over the calibration. A limit
/// set exactly at the measurement would fire on the first regeneration.
#[test]
fn every_published_limit_leaves_headroom_over_the_calibration() {
    let findings = findings_for(&committed_glb(CALIBRATION));

    // `mesh.non_manifold`'s ceiling is calibrated before the fixer, and this
    // file is what rigging returned of the file the fixer wrote, so the
    // reading it has to leave headroom over is `mesh.non_manifold_post`'s.
    for rule in [
        "mesh.holes",
        "mesh.islands",
        "mesh.self_intersect",
        "mesh.mirror",
        "mesh.budget",
    ] {
        let finding = finding(&findings, rule);
        assert!(
            finding.measured < finding.limit,
            "{rule}: {} against a limit of {}",
            finding.measured,
            finding.limit
        );
    }

    // `mesh.world_size` is not calibrated on this file: rigging is what
    // scaled it to the spec's height, so the reading with headroom to leave
    // is the bare mesh's own.
    let limit = finding(&findings, "mesh.world_size").limit;
    assert!(
        BARE_PERCENT < limit,
        "mesh.world_size: {BARE_PERCENT} against a limit of {limit}"
    );
}

#[test]
fn every_rule_measures_every_subject_it_should() {
    let findings = findings_for(&committed_glb(CALIBRATION));

    for (rule, expected) in SUBJECTS {
        let counted = findings.iter().filter(|f| f.rule == rule).count();
        assert_eq!(counted, expected, "{rule} measured {counted} subjects");
    }
    assert_eq!(findings.len(), 13, "and no rule reports outside the list");
}

/// A second mesh object doubles only the four per-object rules.
#[test]
fn a_second_object_is_measured_by_the_four_rules_that_are_about_objects() {
    let findings = findings_of(&SyntheticMesh::figure().plus_an_object("Icosphere"));

    for rule in ["mesh.stray_object", "mesh.texture", "mesh.uv", "mesh.quads"] {
        assert_eq!(
            findings.iter().filter(|f| f.rule == rule).count(),
            2,
            "{rule}"
        );
    }
    assert_eq!(findings.len(), 17);
}

/// A rule that goes quiet when it passes is indistinguishable from a rule
/// that never ran, and this pipeline has already shipped one of those.
#[test]
fn every_rule_reports_on_good_art_and_on_bad() {
    for findings in [
        findings_of(&SyntheticMesh::figure()),
        findings_for(&committed_glb(CALIBRATION)),
        findings_with(&committed_glb(CALIBRATION), Some(RECORDED)),
        findings_of(&SyntheticMesh::figure().without_a_face().repeated(67)),
    ] {
        for rule in mesh::FILE_RULES {
            assert!(
                findings.iter().any(|finding| finding.rule == rule.id),
                "{} reported nothing at all",
                rule.id
            );
        }
    }
}

// --- the positive fixture -------------------------------------------------

#[test]
fn a_conformant_figure_breaks_no_rule() {
    let findings = findings_of(&SyntheticMesh::figure());

    assert_eq!(broken(&findings), Vec::<String>::new(), "{findings:#?}");
}

#[test]
fn a_conformant_figure_measures_zero_on_every_count_rule() {
    let findings = findings_of(&SyntheticMesh::figure());

    for rule in [
        "mesh.holes",
        "mesh.non_manifold",
        "mesh.self_intersect",
        "mesh.facing",
    ] {
        assert_eq!(measured(&findings, rule, "fixture.glb"), 0.0, "{rule}");
    }
    assert_eq!(measured(&findings, "mesh.islands", "fixture.glb"), 1.0);
    assert_eq!(measured(&findings, "mesh.budget", "fixture.glb"), 12.0);
    assert!(measured(&findings, "mesh.mirror", "fixture.glb") < 1e-6);
    // glTF stores a node matrix in f32, so the 0.01 scale is not exactly
    // 0.01 and 1.70 m comes back a tenth of a micron short.
    assert!(measured(&findings, "mesh.world_size", "fixture.glb") < 1e-5);
}

// --- one negative per rule ------------------------------------------------

/// 67 boxes each missing one face: 201 boundary edges against a limit of
/// 200.
#[test]
fn a_mesh_with_more_holes_than_the_limit_is_rejected() {
    let findings = findings_of(&SyntheticMesh::figure().without_a_face().repeated(67));

    assert_eq!(rejected(&findings, "mesh.holes"), ["fixture.glb"]);
    assert_eq!(measured(&findings, "mesh.holes", "fixture.glb"), 201.0);
    let message = &finding(&findings, "mesh.holes").message;
    assert!(message.contains("used by one face"), "got: {message}");
}

/// One face short of the limit passes, which is what makes the test above a
/// test of the limit and not of the direction of the comparison.
#[test]
fn a_mesh_one_hole_inside_the_limit_is_accepted() {
    let findings = findings_of(&SyntheticMesh::figure().without_a_face().repeated(66));

    assert_eq!(measured(&findings, "mesh.holes", "fixture.glb"), 198.0);
    assert!(rejected(&findings, "mesh.holes").is_empty());
}

/// Eleven edges each carrying three faces, against a limit of ten.
#[test]
fn a_mesh_with_too_many_non_manifold_edges_is_rejected() {
    let findings = findings_of(
        &SyntheticMesh::figure()
            .with_three_faces_on_one_edge()
            .repeated(11),
    );

    assert_eq!(rejected(&findings, "mesh.non_manifold"), ["fixture.glb"]);
    assert_eq!(
        measured(&findings, "mesh.non_manifold", "fixture.glb"),
        11.0
    );
}

/// Nine separate pieces against a limit of eight.
#[test]
fn a_mesh_that_falls_into_too_many_pieces_is_rejected() {
    let findings = findings_of(&SyntheticMesh::figure().repeated(9));

    assert_eq!(rejected(&findings, "mesh.islands"), ["fixture.glb"]);
    assert_eq!(measured(&findings, "mesh.islands", "fixture.glb"), 9.0);
    let message = &finding(&findings, "mesh.islands").message;
    assert!(message.contains("9 connected pieces"), "got: {message}");
}

/// Debris inside the mesh is a piece of its own, which is what the fixer's
/// `delete_small_pieces` exists to remove.
#[test]
fn a_five_millimeter_cube_inside_the_mesh_is_counted_as_a_piece() {
    let findings = findings_of(&SyntheticMesh::figure().plus_debris());

    assert_eq!(measured(&findings, "mesh.islands", "fixture.glb"), 2.0);
    // And it changes nothing else: it is inside the body, so the bounds, the
    // reflection and the facing are all untouched.
    assert_eq!(broken(&findings), Vec::<String>::new(), "{findings:#?}");
}

/// One box poking through another crosses in 14 faces, so 120 of those pairs
/// is 1,680 faces against a limit of 1,500.
#[test]
fn a_mesh_whose_faces_cross_each_other_is_rejected() {
    let one_pair = findings_of(&SyntheticMesh::figure().plus_an_overlapping_box());
    let findings = findings_of(
        &SyntheticMesh::figure()
            .plus_an_overlapping_box()
            .repeated(120),
    );

    assert_eq!(
        measured(&one_pair, "mesh.self_intersect", "fixture.glb"),
        14.0
    );
    assert_eq!(rejected(&findings, "mesh.self_intersect"), ["fixture.glb"]);
    assert_eq!(
        measured(&findings, "mesh.self_intersect", "fixture.glb"),
        1_680.0
    );
    let message = &finding(&findings, "mesh.self_intersect").message;
    assert!(message.contains("share no vertex with"), "got: {message}");
}

/// The other side of it: a 5 mm cube sitting inside the body without
/// touching it crosses nothing, so a separated pair is not a defect.
#[test]
fn faces_that_are_merely_close_are_not_counted() {
    let findings = findings_of(&SyntheticMesh::figure().plus_debris());

    assert_eq!(
        measured(&findings, "mesh.self_intersect", "fixture.glb"),
        0.0
    );
    assert_eq!(measured(&findings, "mesh.islands", "fixture.glb"), 2.0);
}

/// One side pushed out 2 cm on a 32 cm mesh: 6.25 percent, against 3.5.
#[test]
fn a_lopsided_mesh_is_rejected_by_the_mirror_rule() {
    let findings = findings_of(&SyntheticMesh::figure().lopsided(0.02));

    assert_eq!(rejected(&findings, "mesh.mirror"), ["fixture.glb"]);
    let worst = measured(&findings, "mesh.mirror", "fixture.glb");
    assert!(
        (worst - 6.25).abs() < 0.01,
        "2 cm over a width of 32 cm is 6.25 percent, measured {worst}"
    );
}

/// The `[mut]` fixture: the same vertex data with the node scale taken out
/// of the file, which is what a reader that skips the node chain sees.
#[test]
fn the_same_mesh_read_without_its_node_scale_is_a_hundred_times_too_big() {
    let findings = findings_of(&SyntheticMesh::figure().without_the_node_scale());

    assert_eq!(rejected(&findings, "mesh.world_size"), ["fixture.glb"]);
    let off = measured(&findings, "mesh.world_size", "fixture.glb");
    assert!(
        (off - 9900.0).abs() < 1.0,
        "170 m against 1.7 is 9900 percent, measured {off}"
    );
    let message = &finding(&findings, "mesh.world_size").message;
    assert!(message.contains("170.0000 m"), "got: {message}");
}

/// The height a generator picks for itself, which is what `mesh.world_size`
/// reads before anything has been asked to scale the body. The real bare mesh
/// arrives at [`BARE_METERS`], and this fixture stands where it stands:
/// inside `mesh.height_percent`, and far outside the rig's 5 percent band.
#[test]
fn a_bare_mesh_at_the_height_the_generator_chose_is_accepted() {
    let findings = findings_of(&SyntheticMesh::figure().at_height(BARE_METERS));

    let off = measured(&findings, "mesh.world_size", "fixture.glb");
    assert!(
        (off - BARE_PERCENT).abs() < 0.01,
        "{BARE_METERS} m against {HEIGHT_METERS} is {BARE_PERCENT} percent, measured {off}"
    );
    assert!(
        off > profile().height_tolerance_percent,
        "the rig's band is {}, so this fixture only passes on a band of its own",
        profile().height_tolerance_percent
    );
    assert_eq!(broken(&findings), Vec::<String>::new(), "{findings:#?}");
}

#[test]
fn a_mesh_yawed_a_hundred_and_eighty_degrees_faces_the_wrong_way() {
    let away = DQuat::from_rotation_y(std::f64::consts::PI);
    let findings = findings_of(&SyntheticMesh::figure().turned(away));

    assert_eq!(rejected(&findings, "mesh.facing"), ["fixture.glb"]);
    assert_eq!(broken(&findings), ["mesh.facing"], "and nothing else");
    let message = &finding(&findings, "mesh.facing").message;
    assert!(
        message.contains("-z") && message.contains("180"),
        "got: {message}"
    );
}

/// A quarter turn is the other half of the axis rule: an axis is a choice of
/// six, so a mesh looking sideways names one of the other five.
#[test]
fn a_mesh_yawed_ninety_degrees_faces_the_wrong_way_too() {
    let aside = DQuat::from_rotation_y(std::f64::consts::FRAC_PI_2);
    let findings = findings_of(&SyntheticMesh::figure().turned(aside));

    assert_eq!(rejected(&findings, "mesh.facing"), ["fixture.glb"]);
    let message = &finding(&findings, "mesh.facing").message;
    assert!(
        message.contains("+x") || message.contains("-x"),
        "got: {message}"
    );
}

#[test]
fn a_mesh_object_the_profile_does_not_name_is_rejected() {
    let findings = findings_of(&SyntheticMesh::figure().plus_an_object("Icosphere"));

    assert_eq!(rejected(&findings, "mesh.stray_object"), ["Icosphere"]);
    assert_eq!(measured(&findings, "mesh.stray_object", ALLOWED), 0.0);
    let message = &findings
        .iter()
        .find(|f| f.rule == "mesh.stray_object" && f.subject == "Icosphere")
        .expect("the stray object")
        .message;
    assert!(message.contains("debris"), "got: {message}");
}

#[test]
fn a_mesh_over_the_triangle_budget_is_rejected() {
    let findings = findings_of(&SyntheticMesh::figure().strip_of(300_001));

    assert_eq!(rejected(&findings, "mesh.budget"), ["fixture.glb"]);
    assert_eq!(measured(&findings, "mesh.budget", "fixture.glb"), 300_001.0);
}

#[test]
fn a_texture_coordinate_off_the_tile_is_rejected() {
    let findings = findings_of(&SyntheticMesh::figure().with_a_uv_at(1.4));

    assert_eq!(rejected(&findings, "mesh.uv"), [ALLOWED]);
    assert_eq!(measured(&findings, "mesh.uv", ALLOWED), 1.0);
    let message = &finding(&findings, "mesh.uv").message;
    assert!(message.contains("[0,1]"), "got: {message}");
}

#[test]
fn a_mesh_with_no_base_color_image_is_rejected() {
    let findings = findings_of(&SyntheticMesh::figure().untextured());

    assert_eq!(rejected(&findings, "mesh.texture"), [ALLOWED]);
    let message = &finding(&findings, "mesh.texture").message;
    assert!(message.contains("untextured mesh"), "got: {message}");
}

/// glTF cannot express a quad, so `mesh.quads` reports what it can: whether
/// every primitive is triangles. A primitive of lines is a primitive every
/// other rule silently did not measure.
#[test]
fn a_primitive_that_is_not_triangles_is_rejected() {
    let findings = findings_of(&SyntheticMesh::figure().made_of_lines());

    assert_eq!(rejected(&findings, "mesh.quads"), [ALLOWED]);
    assert_eq!(measured(&findings, "mesh.quads", ALLOWED), 1.0);
    let message = &finding(&findings, "mesh.quads").message;
    assert!(
        message.contains("points, lines or strips"),
        "got: {message}"
    );
}

// --- measurements that do not exist ---------------------------------------

/// Every whole-surface rule says why rather than reporting zero. Zero holes
/// on no triangles is the shape of a gate that passed by doing nothing.
#[test]
fn a_mesh_with_no_triangle_leaves_every_surface_rule_undefined() {
    let findings = findings_of(&SyntheticMesh::figure().made_of_lines());

    for rule in [
        "mesh.holes",
        "mesh.non_manifold",
        "mesh.islands",
        "mesh.self_intersect",
    ] {
        let finding = finding(&findings, rule);
        assert_eq!(finding.unit, "undefined measurements", "{finding:#?}");
        assert_eq!(finding.severity, Severity::Error);
        assert!(finding.message.contains("mesh.quads"), "{finding:#?}");
    }
    assert!(findings.iter().all(|finding| finding.measured.is_finite()));
}

/// A percent of a width that is zero is a precise number about nothing.
#[test]
fn a_mesh_with_no_width_leaves_the_mirror_rule_undefined() {
    let findings = findings_of(&SyntheticMesh::figure().flat_in_x());
    let mirror = finding(&findings, "mesh.mirror");

    assert_eq!(mirror.unit, "undefined measurements", "{mirror:#?}");
    assert!(mirror.message.contains("nothing to reflect"), "{mirror:#?}");
}

/// The facing comes from where the lowest slice sits against the whole
/// body. A mesh whose bottom sits directly under its middle has no facing,
/// and the rule reports the reason rather than normalizing a zero to a NaN.
#[test]
fn a_mesh_with_no_feet_leaves_the_facing_undefined() {
    let findings = findings_of(&SyntheticMesh::figure().without_feet());
    let facing = finding(&findings, "mesh.facing");

    assert_eq!(facing.severity, Severity::Error);
    assert!(facing.message.contains("faces nowhere"), "{facing:#?}");
    assert!(findings.iter().all(|finding| finding.measured.is_finite()));
}

/// One error names the whole surface, the way `rig.bone_set` speaks for
/// every bone rule on a file that is not a rig. The remote rule still
/// reports, because it measures what Meshy saw and not what is on this disk.
#[test]
fn a_file_that_holds_no_mesh_is_an_error_and_not_a_skip() {
    let findings = findings_for(&under_repo("art/skeletons/humanoid.toml"));

    assert_eq!(errors(&findings).len(), 1, "{findings:#?}");
    assert_eq!(errors(&findings)[0].rule, "mesh.holes");
    assert!(errors(&findings)[0].message.contains("no readable surface"));
    let rules: Vec<&str> = findings.iter().map(|f| f.rule.as_str()).collect();
    assert_eq!(rules, ["mesh.holes", "mesh.printability"]);
}

/// A name nothing writes, because `bare.glb` is on disk the moment anyone
/// runs the model stage and a test that passes only on a fresh checkout
/// proves nothing.
#[test]
fn a_missing_file_is_an_error_and_not_a_skip() {
    let findings = findings_for(&under_repo("art/staging/survivor/never_downloaded.glb"));

    assert_eq!(errors(&findings).len(), 1);
    assert!(
        errors(&findings)[0]
            .message
            .contains("never_downloaded.glb")
    );
}

// --- the remote rule ------------------------------------------------------

/// The recorded response shape, of the two fields a gate reads.
const RECORDED: &str = r#"{ "non_manifold_edges": 179, "is_watertight": false }"#;

/// Meshy adds boundary and true non-manifold edges together, which is why it
/// reads 179 where our own pass reads 171 plus 8. That sum is in the space
/// the finding names.
#[test]
fn the_recorded_printability_response_is_the_sum_of_our_own_two_counts() {
    let finding = mesh::printability(RECORDED, CALIBRATION, &profile(), 1).expect("a valid report");

    assert_eq!(finding.measured, 179.0);
    assert_eq!(finding.severity, Severity::Info);
    assert!(finding.message.contains("not watertight"), "{finding:#?}");
    assert!(
        finding.measured_on.contains("together"),
        "the space says the two counts are added: {finding:#?}"
    );
}

#[test]
fn an_unprintable_response_is_rejected() {
    let broken = r#"{ "non_manifold_edges": 13368, "is_watertight": false }"#;

    let finding = mesh::printability(broken, CALIBRATION, &profile(), 1).expect("a valid report");

    assert_eq!(finding.severity, Severity::Error);
    assert_eq!(finding.measured, 13_368.0);
}

/// The other verdict, so the message is not a one-branch string.
#[test]
fn a_watertight_response_says_so() {
    let clean = r#"{ "non_manifold_edges": 0, "is_watertight": true }"#;

    let finding = mesh::printability(clean, CALIBRATION, &profile(), 1).expect("a valid report");

    assert_eq!(finding.measured, 0.0);
    assert!(
        finding.message.contains("calls the mesh watertight"),
        "{finding:#?}"
    );
}

#[test]
fn a_response_that_is_not_a_printability_report_is_refused() {
    let error = mesh::printability("{}", CALIBRATION, &profile(), 1)
        .expect_err("a report needs its two fields");

    assert!(format!("{error:#}").contains("print/analyze"), "{error:#}");
}

/// An unavailable remote call is a warning and never silence: a check that
/// reports nothing cannot be told from one that passed.
#[test]
fn an_unavailable_remote_call_is_a_warning_that_says_why() {
    let finding = mesh::printability_unavailable(CALIBRATION, 1, "the network is down");

    assert_eq!(finding.severity, Severity::Warning);
    assert_eq!(finding.unit, "unavailable calls");
    assert!(finding.message.contains("the network is down"));
    assert!(finding.measured.is_finite());
}

// --- the rule list --------------------------------------------------------

#[test]
fn every_rule_is_listed_once_under_its_own_family() {
    let ids: BTreeSet<&str> = mesh::RULES.iter().map(|rule| rule.id).collect();

    assert_eq!(ids.len(), mesh::RULES.len(), "a rule id is listed twice");
    assert_eq!(mesh::RULES.len(), 15);
    assert_eq!(
        mesh::FILE_RULES.len() + mesh::CLEANUP_RULES.len(),
        mesh::RULES.len(),
        "every rule is measured somewhere, and on one file or on the pair"
    );
    assert!(ids.iter().all(|id| id.starts_with("mesh.")));
}

/// The two rules the file the fixer wrote is **not** read against, out of
/// the thirteen every other mesh file is. Both would be read against
/// something that is not true of a cleaned file.
#[test]
fn the_cleaned_mesh_is_read_against_every_file_rule_but_two() {
    let dropped: Vec<&str> = mesh::FILE_RULES
        .iter()
        .filter(|rule| !mesh::CLEANED_RULES.iter().any(|kept| kept.id == rule.id))
        .map(|rule| rule.id)
        .collect();

    assert_eq!(dropped, ["mesh.non_manifold", "mesh.printability"]);
}

/// `mesh.non_manifold`'s ceiling is calibrated on the mesh as it arrived,
/// and filling a hole raises that count on purpose. After the fixer it is
/// `mesh.non_manifold_post`'s, which is read against a ceiling of its own,
/// so the reading is dropped here rather than failed twice.
#[test]
fn a_cleaned_mesh_is_not_held_to_the_ceiling_the_mesh_arrived_under() {
    let over = SyntheticMesh::figure()
        .with_three_faces_on_one_edge()
        .repeated(11);

    let whole = findings_of(&over);
    let cleaned = mesh::only(&mesh::CLEANED_RULES, whole.clone());

    assert_eq!(rejected(&whole, "mesh.non_manifold"), ["fixture.glb"]);
    assert!(
        !cleaned
            .iter()
            .any(|finding| finding.rule == "mesh.non_manifold"),
        "{cleaned:#?}"
    );
    // And the remote rule, which asks about a task a local file has none of.
    assert!(
        !cleaned
            .iter()
            .any(|finding| finding.rule == "mesh.printability"),
        "{cleaned:#?}"
    );
    assert_eq!(cleaned.len(), 11);
}

/// `--list-rules` prints the registry, and a finding is built through it, so
/// neither can advertise a limit, a unit or a space the other does not
/// carry.
#[test]
fn a_finding_carries_exactly_what_the_rule_list_advertises() {
    let profile = profile();
    let findings = [
        findings_of(&SyntheticMesh::figure()),
        findings_for(&committed_glb(CALIBRATION)),
        findings_with(&committed_glb(CALIBRATION), Some(RECORDED)),
    ]
    .concat();

    for finding in &findings {
        let rule = mesh::RULES
            .iter()
            .find(|rule| rule.id == finding.rule)
            .unwrap_or_else(|| panic!("{} is not in the rule list", finding.rule));
        assert_eq!(finding.measured_on, rule.space, "{}", rule.id);
        // A measurement that does not exist carries its own unit and says
        // so, which is the one documented exception. `undefined
        // measurements` is the local form and `unavailable calls` is the
        // remote one, the same shape the validator uses for a missing file.
        if !NOT_A_MEASUREMENT.contains(&finding.unit.as_str()) {
            assert_eq!(finding.unit, rule.unit, "{}", rule.id);
            assert_eq!(finding.comparison, rule.comparison, "{}", rule.id);
            assert_eq!(finding.limit, (rule.limit)(&profile), "{}", rule.id);
        }
    }
}

/// A precise number on the wrong representation is the failure this whole
/// module exists to stop, so the space a finding names has to be the space
/// the code measured in.
#[test]
fn the_space_every_topology_rule_names_is_the_weld_distance_it_used() {
    let findings = findings_for(&committed_glb(CALIBRATION));
    let surface = Surface::read(&committed_glb(CALIBRATION)).expect("the survivor");

    assert_eq!(surface.weld_meters(), Some(WELD_METERS));
    for rule in [
        "mesh.holes",
        "mesh.non_manifold",
        "mesh.islands",
        "mesh.budget",
    ] {
        let space = &finding(&findings, rule).measured_on;
        assert!(
            space.contains("welded at 1e-5 m") && space.contains("node chain"),
            "{rule} names the space {space:?}"
        );
    }
    assert_eq!(
        WELD_METERS, 1e-5,
        "the space strings above spell this number out"
    );
}

/// The comparison decides the severity, so no rule can file its own broken
/// measurement as information.
#[test]
fn the_severity_of_every_finding_follows_from_its_own_comparison() {
    let findings = [
        findings_of(&SyntheticMesh::figure()),
        findings_for(&committed_glb(CALIBRATION)),
        findings_of(&SyntheticMesh::figure().lopsided(0.02)),
        findings_of(&SyntheticMesh::figure().without_the_node_scale()),
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
    let mut report = Report::new(mesh::STAGE, "survivor", 1);

    for fixture in [
        findings_of(&SyntheticMesh::figure()),
        findings_of(&SyntheticMesh::figure().made_of_lines()),
        findings_of(&SyntheticMesh::figure().flat_in_x()),
        findings_for(&committed_glb(CALIBRATION)),
        findings_with(&committed_glb(CALIBRATION), Some(RECORDED)),
    ] {
        report
            .extend(fixture)
            .expect("the harness accepts them all");
    }
    assert!(report.has_errors(), "the broken fixtures are in there");
    assert_eq!(report.exit_code(), 1);
}

/// Two runs of the same gates on the same art must say the same thing.
#[test]
fn the_same_mesh_measures_the_same_twice() {
    let once = findings_for(&committed_glb(CALIBRATION));
    let twice = findings_for(&committed_glb(CALIBRATION));

    assert_eq!(once, twice);
}

/// A local gate measures or it refuses. The one warning in the family is the
/// remote call, which is the only rule that can be unavailable.
#[test]
fn only_the_remote_rule_can_warn() {
    let findings = findings_of(&SyntheticMesh::figure());

    let warned: Vec<&str> = findings
        .iter()
        .filter(|finding| finding.severity == Severity::Warning)
        .map(|finding| finding.rule.as_str())
        .collect();
    assert_eq!(warned, ["mesh.printability"]);
    assert!(
        findings
            .iter()
            .filter(|finding| finding.rule != "mesh.printability")
            .all(|finding| finding.severity == Severity::Info)
    );
    assert!(
        findings
            .iter()
            .any(|finding| finding.comparison == Comparison::Le),
        "the count rules read at most"
    );
}

/// With a response on record the rule measures, so nothing warns.
#[test]
fn a_recorded_response_leaves_nothing_unavailable() {
    let findings = findings_with(&committed_glb(CALIBRATION), Some(RECORDED));

    assert_eq!(measured(&findings, "mesh.printability", CALIBRATION), 179.0);
    assert!(
        findings
            .iter()
            .filter(|finding| finding.rule != "mesh.non_manifold")
            .all(|finding| finding.severity == Severity::Info),
        "{findings:#?}"
    );
}

/// A response that cannot be read is an answer we do not have, so it lands
/// the same way an absent one does rather than aborting the run.
#[test]
fn a_response_that_cannot_be_read_is_the_same_as_no_response() {
    let findings = findings_with(&committed_glb(CALIBRATION), Some("{}"));
    let remote = finding(&findings, "mesh.printability");

    assert_eq!(remote.severity, Severity::Warning);
    assert_eq!(remote.unit, "unavailable calls");
    assert!(remote.message.contains("print/analyze"), "{remote:#?}");
}

/// And with nothing on record it says which of the two it is.
#[test]
fn no_recorded_response_says_that_nobody_asked() {
    let findings = findings_for(&committed_glb(CALIBRATION));
    let remote = finding(&findings, "mesh.printability");

    assert_eq!(remote.severity, Severity::Warning);
    assert!(remote.message.contains(mesh::NO_RESPONSE), "{remote:#?}");
}

// --- what the fixer left behind -------------------------------------------

/// The pair the two post-cleanup rules read: the mesh the fixer was given,
/// beside the mesh it wrote.
fn cleanup_findings(before: &SyntheticMesh, after: &SyntheticMesh) -> Vec<Finding> {
    let (before, after) = (
        Surface::from_slice(&before.to_glb()).expect("a readable fixture"),
        Surface::from_slice(&after.to_glb()).expect("a readable fixture"),
    );
    mesh::check_cleanup(
        "clean.glb",
        mesh::Fixed::Cleaned {
            before: &before,
            after: &after,
        },
        &profile(),
        1,
    )
}

/// What the fixer is given: holes, debris and crossing faces, all at once.
fn a_mesh_worth_cleaning() -> SyntheticMesh {
    SyntheticMesh::figure()
        .without_a_face()
        .plus_debris()
        .plus_an_overlapping_box()
}

/// Both rules report on both sides of the declaration, so neither can go
/// quiet on a character that switched the fixer off.
#[test]
fn every_post_cleanup_rule_reports_whether_or_not_the_fixer_ran() {
    for findings in [
        cleanup_findings(&a_mesh_worth_cleaning(), &SyntheticMesh::figure()),
        mesh::check_cleanup("bare.glb", mesh::Fixed::Declined, &profile(), 1),
    ] {
        for rule in mesh::CLEANUP_RULES {
            assert!(
                findings.iter().any(|finding| finding.rule == rule.id),
                "{} reported nothing at all",
                rule.id
            );
        }
        assert_eq!(findings.len(), 2, "a ceiling and a before-and-after");
    }
}

/// The positive: a fixer that removed defects of both counted classes. Each
/// pair is in the message, and their sum is what is read.
#[test]
fn a_cleanup_that_took_every_class_down_holds() {
    let findings = cleanup_findings(&a_mesh_worth_cleaning(), &SyntheticMesh::figure());

    assert_eq!(broken(&findings), Vec::<String>::new(), "{findings:#?}");
    let effective = finding(&findings, "mesh.cleanup_effective");
    assert_eq!(effective.comparison, Comparison::Lt);
    assert_eq!(
        (effective.measured, effective.limit),
        (1.0, 6.0),
        "3 holes and 3 pieces, down to one whole piece"
    );
    assert_eq!(
        effective.message,
        "the fixer took holes 3 to 0, islands 3 to 1"
    );
}

/// Crossing faces are not counted, and the reason is measured: mirroring a
/// mesh copies the crossings of the half it keeps. `mesh.self_intersect` is
/// the gate that owns them, and it still fires on this fixture.
#[test]
fn a_fixer_that_left_more_crossing_faces_than_it_found_is_still_effective() {
    let crossing = SyntheticMesh::figure().plus_an_overlapping_box();

    let findings = cleanup_findings(&a_mesh_worth_cleaning(), &crossing);

    let effective = finding(&findings, "mesh.cleanup_effective");
    assert_eq!(effective.severity, Severity::Info, "{effective:#?}");
    assert!(
        !effective.message.contains("self-intersections"),
        "{effective:#?}"
    );
    assert_eq!(
        measured(
            &findings_of(&crossing),
            "mesh.self_intersect",
            "fixture.glb"
        ),
        14.0,
        "and the rule that owns them still counts them"
    );
}

/// The negative the design names: a fixer stub that wrote its input back. It
/// reads the same number it was given, and `lt` refuses it. Under `le` this
/// is the gate that cannot fail.
#[test]
fn a_fixer_that_changed_nothing_is_refused() {
    let stub = a_mesh_worth_cleaning();
    let findings = cleanup_findings(&stub, &stub);

    assert_eq!(rejected(&findings, "mesh.cleanup_effective"), ["clean.glb"]);
    let effective = finding(&findings, "mesh.cleanup_effective");
    assert_eq!((effective.measured, effective.limit), (6.0, 6.0));
    assert_eq!(
        effective.message,
        "the fixer took holes 3 to 3, islands 3 to 3"
    );
}

/// A class the fixer was given nothing to remove in cannot go down, and a
/// fixer that fixed the other one is still a fixer that worked. This is what
/// one reading per class would refuse.
#[test]
fn a_class_that_arrived_clean_does_not_refuse_a_fixer_that_worked() {
    let arrived = SyntheticMesh::figure().plus_debris();

    let findings = cleanup_findings(&arrived, &SyntheticMesh::figure());

    let effective = finding(&findings, "mesh.cleanup_effective");
    assert!(effective.message.contains("holes 0 to 0"), "{effective:#?}");
    assert_eq!(effective.severity, Severity::Info, "{effective:#?}");
}

/// And the ceiling on what filling holes leaves behind. Fourteen edges each
/// carrying three faces, against a limit of 20 minus the figure's own 0.
#[test]
fn a_cleaned_mesh_over_the_post_cleanup_ceiling_is_rejected() {
    let findings = cleanup_findings(
        &a_mesh_worth_cleaning(),
        &SyntheticMesh::figure()
            .with_three_faces_on_one_edge()
            .repeated(21),
    );

    assert_eq!(rejected(&findings, "mesh.non_manifold_post"), ["clean.glb"]);
    assert_eq!(
        measured(&findings, "mesh.non_manifold_post", "clean.glb"),
        21.0
    );
}

/// One under the ceiling passes, which is what makes the test above a test
/// of the limit rather than of the direction of the comparison.
#[test]
fn a_cleaned_mesh_one_edge_inside_the_post_cleanup_ceiling_is_accepted() {
    let findings = cleanup_findings(
        &a_mesh_worth_cleaning(),
        &SyntheticMesh::figure()
            .with_three_faces_on_one_edge()
            .repeated(20),
    );

    assert_eq!(
        rejected(&findings, "mesh.non_manifold_post"),
        Vec::<String>::new()
    );
}

/// `cleanup: false` is a declaration and not a measurement, so both rules
/// say so rather than reporting a zero that would read as a clean mesh.
#[test]
fn a_spec_that_declines_the_cleanup_switches_both_rules_off() {
    let findings = mesh::check_cleanup("bare.glb", mesh::Fixed::Declined, &profile(), 1);

    for finding in &findings {
        assert_eq!(finding.severity, Severity::Skipped, "{finding:#?}");
        assert_eq!(
            finding.message,
            "spec.subject.cleanup is false, so no fixer ran and nothing was \
             written to measure"
        );
    }
    assert_eq!(errors(&findings).len(), 0);
}

/// A fixer that emptied the mesh leaves zero non-manifold edges, which reads
/// as a perfect result. Both rules say there was nothing to measure instead.
#[test]
fn a_fixer_that_left_no_triangle_is_undefined_rather_than_a_clean_mesh() {
    let findings = cleanup_findings(
        &a_mesh_worth_cleaning(),
        &SyntheticMesh::figure().made_of_lines(),
    );

    assert_eq!(findings.len(), 2);
    for finding in &findings {
        assert_eq!(finding.severity, Severity::Error, "{finding:#?}");
        assert_eq!(finding.unit, "undefined measurements", "{finding:#?}");
        assert!(finding.message.contains("no triangle"), "{finding:#?}");
    }
}

// --- what a character that declines symmetry reports -----------------------

/// `symmetry: false` switches the mirror rule off, on the declaration and
/// never on a number: a monster can be asymmetric on purpose.
#[test]
fn a_character_that_declines_symmetry_skips_the_mirror_rule() {
    let surface = Surface::from_slice(&SyntheticMesh::figure().lopsided(0.02).to_glb()).unwrap();

    let findings = mesh::check(
        "fixture.glb",
        &surface,
        &profile(),
        HEIGHT_METERS,
        Symmetry::Declined,
        None,
        1,
    );

    let mirror = finding(&findings, "mesh.mirror");
    assert_eq!(mirror.severity, Severity::Skipped, "{mirror:#?}");
    assert_eq!(
        mirror.message,
        "spec.subject.symmetry is false, so this character is not mirrored \
         and nothing reads it against its own reflection"
    );
    assert_eq!(errors(&findings), Vec::<&Finding>::new(), "{findings:#?}");
}

/// And the negative control still runs: the same fixture under a spec that
/// asks for symmetry is still rejected, so the proof the rule works does not
/// leave with the flag.
#[test]
fn the_same_lopsided_mesh_is_still_rejected_when_symmetry_is_enforced() {
    let findings = findings_of(&SyntheticMesh::figure().lopsided(0.02));

    assert_eq!(rejected(&findings, "mesh.mirror"), ["fixture.glb"]);
}
