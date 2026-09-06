//! The three foot contact rules, on the delivered file.
//!
//! Two halves. The maths runs on hand-built paths whose answers are counted
//! by hand, and the same paths and the same answers are in
//! `tools/blender/tests/unit/test_plant.py`, so the two implementations are
//! pinned to one set of numbers rather than to each other. The rules
//! themselves run on the synthetic cross-rig pair, standing on the floor of
//! its own rig, with one thing moved per negative.

use glam::DVec3;
use xtask_art::check::aim::AimTable;
use xtask_art::check::clip::Fitted;
use xtask_art::check::foot::{PENETRATION, PLANTS, RULES, SKATE, drift, plant_runs, scale_of};
use xtask_art::check::profile::Profile;
use xtask_art::check::{Finding, Severity, foot};

use super::clips::CrossRig;
use super::support::repo_root;

const ATTEMPT: u32 = 1;

/// What the committed clips declare, and the rate the fixture's keys sit on.
const SOURCE_FPS: u32 = 24;

/// One physical path, sampled at whatever rate a case asks for.
///
/// Planted still for the first half second, lifted and moving at 1 m/s for a
/// quarter, stopped but still lifted for a quarter, then planted again from
/// one second on. The landing has no horizontal speed, so both rates read it
/// as a contact on the same frame. `test_plant.py::at` is the same path.
fn at(seconds: f64) -> DVec3 {
    match seconds {
        _ if seconds < 0.5 => DVec3::ZERO,
        _ if seconds < 0.75 => DVec3::new(seconds - 0.5, 0.0, 0.2),
        _ if seconds < 1.0 => DVec3::new(0.25, 0.0, 0.2),
        _ => DVec3::new(0.25, 0.0, 0.0),
    }
}

fn sampled(fps: u32) -> Vec<DVec3> {
    (0..2 * fps)
        .map(|frame| at(f64::from(frame) / f64::from(fps)))
        .collect()
}

// --- the maths ------------------------------------------------------------

#[test]
fn the_vote_width_is_odd_and_at_least_three_at_every_rate() {
    for rate in [8, 12, 15, 20, 24, 30, 48, 60, 120] {
        let width = foot::vote_width(rate);
        assert!(width >= 3, "{rate} fps votes over {width}");
        assert_eq!(width % 2, 1, "{rate} fps votes over {width}");
    }
}

/// Python rounds a half to even and Rust rounds it away from zero, so the two
/// differ by one at 30 fps and at every other rate whose window lands on a
/// half. Making the width odd is what brings them back together.
#[test]
fn the_two_rates_the_library_runs_slowest_and_fastest_both_vote_over_three() {
    assert_eq!(foot::vote_width(8), 3);
    assert_eq!(foot::vote_width(30), 3);
}

/// `plant.py::voted` raises on the same set, and an even window is not
/// centered on its own frame either, so the answer would be wrong.
#[test]
#[should_panic(expected = "a majority window is odd and at least 3, got 4")]
fn a_vote_that_cannot_carry_a_majority_is_refused() {
    foot::voted(&[true, false, true], 4);
}

#[test]
fn a_path_with_two_plants_reports_both_run_ranges() {
    assert_eq!(plant_runs(&sampled(30), 30, 1.0), [(0, 14), (30, 59)]);
}

#[test]
fn a_path_that_never_settles_reports_no_run_at_all() {
    let flying: Vec<DVec3> = (0..20)
        .map(|step| DVec3::new(f64::from(step) * 0.1, 0.0, 0.5))
        .collect();

    assert!(plant_runs(&flying, 30, 1.0).is_empty());
}

#[test]
fn the_same_path_at_eight_and_thirty_gives_the_same_runs() {
    let (slow, fast) = (
        plant_runs(&sampled(8), 8, 1.0),
        plant_runs(&sampled(30), 30, 1.0),
    );

    assert_eq!(slow.len(), 2);
    assert_eq!(fast.len(), 2);
    for (run, other) in slow.iter().zip(&fast) {
        for (at, other_at) in [(run.0, other.0), (run.1, other.1)] {
            let apart = at as f64 / 8.0 - other_at as f64 / 30.0;
            assert!(apart.abs() <= 1.0 / 8.0, "{at} at 8 fps against {other_at}");
        }
    }
}

/// The threshold is meters per second, so the per-frame step it allows is
/// 3.75x larger at 8 fps than at 30, and the answer is the same.
#[test]
fn a_foot_creeping_below_the_rate_is_a_contact_at_both_rates() {
    let creep = |fps: u32| -> Vec<DVec3> {
        (0..fps)
            .map(|frame| DVec3::new(f64::from(frame) / f64::from(fps) * 0.2, 0.0, 0.0))
            .collect()
    };

    assert_eq!(plant_runs(&creep(8), 8, 1.0), [(0, 7)]);
    assert_eq!(plant_runs(&creep(30), 30, 1.0), [(0, 29)]);
}

#[test]
fn a_taller_rig_scales_the_thresholds_it_is_read_against() {
    assert_eq!(scale_of(foot::REFERENCE_HEIGHT_METERS), Some(1.0));
    assert_eq!(scale_of(foot::REFERENCE_HEIGHT_METERS / 2.0), Some(0.5));
    assert_eq!(scale_of(0.0), None);
    assert_eq!(scale_of(f64::NAN), None);
}

#[test]
fn a_foot_just_over_the_scaled_ceiling_is_not_a_contact() {
    let half = scale_of(foot::REFERENCE_HEIGHT_METERS / 2.0).expect("half a reference rig");
    let held =
        |factor: f64| vec![DVec3::new(0.0, 0.0, foot::CONTACT_HEIGHT_METERS * half * factor); 5];

    assert_eq!(plant_runs(&held(0.99), 30, half), [(0, 4)]);
    assert!(plant_runs(&held(1.01), 30, half).is_empty());
}

/// The first frame has no previous one, so it is read against the next. Read
/// against itself it is still by construction, and a foot that starts on the
/// ground and leaves at once plants for exactly one frame.
#[test]
fn a_foot_low_at_the_first_frame_and_gone_at_the_second_never_plants() {
    let leaving: Vec<DVec3> = (0..8)
        .map(|step| DVec3::new(f64::from(step) * 0.3, 0.0, f64::from(step) * 0.3))
        .collect();

    assert!(plant_runs(&leaving, 30, 1.0).is_empty());
}

#[test]
fn the_drift_inside_a_run_is_read_from_its_first_frame() {
    let path = [
        DVec3::ZERO,
        DVec3::new(0.03, 0.04, 0.0),
        DVec3::new(0.0, 0.0, 0.5),
    ];

    assert!((drift(&path, (0, 2)) - 0.05).abs() < 1e-12);
}

/// The thresholds scale by the rig's joint span, and
/// `retarget_animation.py::rig_height` spans the same bones inside Blender.
/// The committed rig also carries two scene nodes that are no bone,
/// `Armature` and `skin_carrier`, and spanning every node instead reads
/// 1.6959 m: that is the reading this test rules out.
#[test]
fn the_thresholds_scale_by_the_skin_joints_and_not_by_every_node() {
    let rig = std::fs::read(crate::support::committed_glb("art/skeletons/humanoid.glb"))
        .expect("the committed rig");
    let gltf = gltf::Gltf::from_slice(&rig).expect("a readable rig");
    let every_node = gltf
        .document
        .nodes()
        .len()
        .checked_sub(gltf.document.skins().flat_map(|skin| skin.joints()).count())
        .expect("more nodes than joints");

    let span = xtask_art::check::gltf_clip::joint_span(&rig).expect("the rig's span");

    assert_eq!(every_node, 2, "the Armature and the skin_carrier");
    assert!((span - 1.665_165).abs() < 5e-7, "{span}");
}

#[test]
fn no_rule_id_is_listed_twice() {
    let ids: Vec<&str> = RULES.iter().map(|rule| rule.id).collect();

    assert_eq!(
        ids.iter()
            .collect::<std::collections::BTreeSet<&&str>>()
            .len(),
        RULES.len()
    );
    assert_eq!(RULES.len(), 3);
}

// --- the rules, on a delivered file ---------------------------------------

fn profile() -> Profile {
    Profile::of(&repo_root(), "humanoid").expect("the humanoid profile")
}

fn table() -> AimTable {
    AimTable::of(&repo_root(), "humanoid").expect("the humanoid aim table")
}

fn bones() -> std::collections::BTreeMap<String, String> {
    let table = table();
    table
        .bones(table.canonical())
        .expect("the canonical convention")
        .clone()
}

/// Every file-side rule on one pair, through the path the runner takes.
fn measured(pair: &CrossRig, travels: bool) -> Vec<Finding> {
    let root = repo_root();
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (clip, sidecar) = (dir.path().join("fit.glb"), dir.path().join("source.json"));
    std::fs::write(&clip, pair.output_glb()).unwrap();
    std::fs::write(&sidecar, pair.source_motion()).unwrap();

    xtask_art::check::clip::check_files(
        &Fitted {
            output: &clip,
            source_motion: &sidecar,
            rig: &crate::support::committed_glb("art/skeletons/humanoid.glb"),
            repo_root: &root,
            source_fps: SOURCE_FPS,
            loops: false,
            travels,
        },
        &profile(),
        &table(),
        ATTEMPT,
    )
    .expect("the gate itself runs")
}

fn of<'a>(findings: &'a [Finding], rule: &str) -> Vec<&'a Finding> {
    findings
        .iter()
        .filter(|finding| finding.rule == rule)
        .collect()
}

fn standing() -> CrossRig {
    CrossRig::new(bones()).standing()
}

#[test]
fn a_standing_pair_plants_both_feet_and_holds_them_still() {
    let findings = measured(&standing(), true);

    assert_eq!(
        of(&findings, PLANTS.id)
            .iter()
            .map(|finding| (finding.subject.as_str(), finding.measured))
            .collect::<Vec<(&str, f64)>>(),
        [("LeftToeBase", 1.0), ("RightToeBase", 1.0)]
    );
    for finding in of(&findings, SKATE.id) {
        assert_eq!(finding.severity, Severity::Info, "{finding:?}");
        assert!(finding.measured < 0.005, "{finding:?}");
    }
    for finding in of(&findings, PENETRATION.id) {
        assert_eq!(finding.severity, Severity::Info, "{finding:?}");
        assert!(finding.measured.abs() < 1e-4, "{finding:?}");
    }
}

/// The message is the whole diagnostic, so it is pinned word for word.
#[test]
fn a_planted_foot_says_which_frames_it_stood_on() {
    let findings = measured(&standing(), true);

    assert_eq!(
        of(&findings, PLANTS.id)[0].message,
        "LeftToeBase plants 1 time(s) over the clip, on frames 0..7"
    );
    assert_eq!(
        of(&findings, SKATE.id)[0].subject,
        "LeftToeBase run 1",
        "one subject per run, so two runs cannot average into one reading"
    );
    assert_eq!(
        of(&findings, SKATE.id)[0].message,
        "LeftToeBase drifts 0.0035 m over frames 0..7, where it is planted"
    );
}

/// `[synth]` a clip whose feet ride along with its root: 2.04 m of strafe in
/// a third of a second is 6 m/s of foot, which comes to rest nowhere.
#[test]
fn a_clip_whose_feet_never_come_to_rest_fails_the_plant_count() {
    let findings = measured(&CrossRig::new(bones()), true);

    for finding in of(&findings, PLANTS.id) {
        assert_eq!(finding.severity, Severity::Error, "{finding:?}");
        assert_eq!(finding.measured, 0.0);
        assert!(
            finding
                .message
                .ends_with("never comes to rest on the ground over the clip"),
            "{finding:?}"
        );
    }
    // And an empty set has no maximum worth trusting, so the drift is a
    // measurement that does not exist rather than a skate of 0.0.
    for finding in of(&findings, SKATE.id) {
        assert_eq!(finding.severity, Severity::Error, "{finding:?}");
        assert_eq!(finding.unit, "undefined measurements");
        assert!(
            finding
                .message
                .ends_with("never plants, so it has no stance to be read across"),
            "{finding:?}"
        );
    }
}

/// `[synth]` a planted foot translated 5 cm while it is down, which is slow
/// enough to still read as contact and twice the published limit.
#[test]
fn a_planted_foot_translated_five_centimeters_fails_the_skate() {
    let findings = measured(&standing().creeping(0.05 / 0.8815), true);

    for finding in of(&findings, SKATE.id) {
        assert_eq!(finding.severity, Severity::Error, "{finding:?}");
        assert!((finding.measured - 0.05).abs() < 1e-3, "{finding:?}");
    }
    assert_eq!(of(&findings, PLANTS.id)[0].severity, Severity::Info);
}

/// `[synth]` the root keyed 2 cm below the floor its own rig rests on.
#[test]
fn a_clip_keyed_under_the_floor_fails_the_penetration() {
    let findings = measured(&standing().sunk(0.02), true);

    // The message names which of the two sole points sank, because the depth
    // alone says nothing about where the foot is wrong.
    for finding in of(&findings, PENETRATION.id) {
        assert_eq!(finding.severity, Severity::Error, "{finding:?}");
        assert!((finding.measured - 0.02).abs() < 1e-4, "{finding:?}");
        assert_eq!(
            finding.message,
            format!(
                "the sole of {} under the toe sinks 0.0200 m below the ground \
                 at 0.000000 s",
                finding.subject
            )
        );
    }
}

/// And the pair either side of the published 5 mm, so the limit itself is
/// what decides rather than the shape of the fixture.
#[test]
fn four_millimeters_under_the_floor_holds_and_six_does_not() {
    assert_eq!(
        of(&measured(&standing().sunk(0.004), true), PENETRATION.id)[0].severity,
        Severity::Info
    );
    assert_eq!(
        of(&measured(&standing().sunk(0.006), true), PENETRATION.id)[0].severity,
        Severity::Error
    );
}

/// A sole that never reaches the floor reports the clearance it kept, which
/// is a negative depth. Reporting it as zero would read like a landing.
#[test]
fn a_foot_that_never_reaches_the_floor_reports_its_clearance() {
    let finding = of(&measured(&standing().sunk(-0.03), true), PENETRATION.id)[0].clone();

    assert_eq!(finding.severity, Severity::Info);
    assert!((finding.measured + 0.03).abs() < 1e-3, "{finding:?}");
    assert!(
        finding.message.contains("over the ground at"),
        "{finding:?}"
    );
}

/// An in-place cycle's ground moves under it, so its feet must slide and
/// neither stance rule has anything to read. The declaration says so, and
/// nothing else does.
#[test]
fn a_clip_the_library_declares_in_place_switches_both_stance_rules_off() {
    let findings = measured(&standing(), false);

    for rule in [PLANTS.id, SKATE.id] {
        for finding in of(&findings, rule) {
            assert_eq!(finding.severity, Severity::Skipped, "{finding:?}");
            assert_eq!(
                finding.message,
                format!(
                    "the library declares travels: false, so {} stands on ground \
                     that moves under it and cannot plant",
                    finding.subject
                )
            );
        }
    }
    // The floor is not a declaration, so this one still measures.
    assert_eq!(of(&findings, PENETRATION.id)[0].severity, Severity::Info);
}

/// A file with nothing to stand on is three errors, never a skip: a gate that
/// goes quiet on absent input proves nothing.
#[test]
fn a_clip_with_no_sole_to_read_is_three_errors_and_never_a_skip() {
    let findings = foot::undefined(
        "art/animations/no.glb",
        ATTEMPT,
        "no.glb carries no readable sole to stand on",
    );

    assert_eq!(findings.len(), RULES.len());
    for finding in &findings {
        assert_eq!(finding.severity, Severity::Error);
        assert_eq!(finding.unit, "undefined measurements");
        assert_eq!(finding.subject, "art/animations/no.glb");
    }
}
