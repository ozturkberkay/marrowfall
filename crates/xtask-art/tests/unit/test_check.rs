//! The Finding record, the report, and the exit-code contract.
//!
//! Four separate checks in this pipeline shipped while being unable to fail,
//! so the negative half of every test here matters more than the positive
//! half.

use std::collections::BTreeMap;
use std::path::Path;

use xtask_art::check::aim::{self, AimTable};
use xtask_art::check::gltf_world::Skeleton;
use xtask_art::check::motion::Motion;
use xtask_art::check::profile::Profile;
use xtask_art::check::{
    Artifacts, Comparison, Finding, Report, Severity, clip, every_rule, gltf_clip, mesh, rig,
    source,
};
use xtask_art::library::HUMANOID;

use crate::rigs::HEIGHT_METERS;
use crate::support::{committed_glb, repo_root};

fn a_finding() -> Finding {
    Finding {
        rule: "clip.swing".to_owned(),
        severity: Severity::Error,
        subject: "LeftHand".to_owned(),
        measured: 52.9,
        limit: 2.0,
        comparison: Comparison::Le,
        unit: "degrees".to_owned(),
        attempt: 1,
        measured_on: "world space, aligned by seconds from clip start".to_owned(),
        message: "left hand swings 52.9 degrees from the source".to_owned(),
    }
}

/// The committed humanoid profile, which every published limit comes from.
fn profile() -> Profile {
    Profile::of(&repo_root(), HUMANOID).expect("the committed humanoid profile")
}

fn added(finding: Finding) -> anyhow::Result<()> {
    Report::new("clip", "run", 1).add(finding)
}

// --- the rule list --------------------------------------------------------

/// `--list-rules` prints this list, so a family missing from it is a family
/// whose limits nobody can read.
#[test]
fn every_family_reaches_the_printed_rule_list() {
    let ids: Vec<&str> = every_rule().map(|rule| rule.id).collect();

    assert_eq!(
        ids.len(),
        43,
        "13 rig rules, the aim table, 13 mesh rules, 6 source rules and 10 clip rules"
    );
    assert_eq!(
        ids.iter()
            .collect::<std::collections::BTreeSet<&&str>>()
            .len(),
        ids.len(),
        "a rule id is listed twice"
    );
    for expected in [
        "rig.child_axis",
        "rig.aim_table",
        "mesh.holes",
        "source.child_axis",
        "source.wander",
        "clip.interpolation",
        "clip.root_travel",
        "clip.root_bob",
    ] {
        assert!(ids.contains(&expected), "{expected} is not in the list");
    }
}

/// Some rules are measured inside Blender, which CI has none of, so their
/// side of the contract is a recorded report: the real ones the refit of
/// `run.glb` and the source check of `run.glb` wrote, committed the way
/// `mesh.printability`'s recorded response is.
fn a_recorded_report(stem: &str) -> Report {
    let path = repo_root()
        .join("crates/xtask-art/tests/fixtures")
        .join(format!("{stem}.json"));
    Report::read(&path).unwrap_or_else(|error| panic!("reading {stem}: {error:#}"))
}

#[test]
fn the_recorded_retarget_report_is_quiet_and_inside_the_registry() {
    let report = a_recorded_report("retarget.run.1");

    assert_eq!((report.stage(), report.item()), ("retarget", "run"));
    assert_eq!(
        report.findings().len(),
        66,
        "22 bones on two rules, 21 keys on the grid, and its range"
    );
    assert!(!report.has_errors(), "the refit of our own clip is clean");
    assert_eq!(
        report.off_registry(&profile()),
        Vec::<String>::new(),
        "every finding says what `--list-rules` says"
    );
}

/// And the bake boundary, where `clip.root_travel` reads the stripped copy
/// that is never written to disk. The recorded run is the three committed
/// clips on the committed character; its ring and its render size are bake
/// parameters this rule does not read.
#[test]
fn the_recorded_bake_report_is_quiet_and_inside_the_registry() {
    let report = a_recorded_report("bake.survivor.1");

    assert_eq!((report.stage(), report.item()), ("bake", "survivor"));
    assert_eq!(report.findings().len(), 9, "three clips, three axes");

    assert!(!report.has_errors());
    assert_eq!(report.off_registry(&profile()), Vec::<String>::new());

    for finding in report.findings() {
        assert_eq!(finding.severity, Severity::Info, "{finding:?}");
        if finding.subject.ends_with(" z") {
            // The bob the strip keeps on purpose: 0.0089 on `idle`, 0.0535 on
            // `run` and 0.0391 on `walk_back`, against a limit of 0.15.
            assert_eq!(finding.rule, "clip.root_bob", "{finding:?}");
            assert!(finding.measured > 0.008, "{finding:?}");
        } else {
            // Horizontal is pinned to the first frame's value, so what is
            // left is the `f32` an F-curve stores: about 2 nanometers.
            assert_eq!(finding.rule, "clip.root_travel", "{finding:?}");
            assert!(finding.measured < 1e-6, "{finding:?}");
        }
    }
}

/// The same for the fetch boundary. `run.glb` stands in for a vendor file
/// because the three Mixamo downloads may not be redistributed, and it is a
/// real reading either way: the numbers below are our own rig's, and the
/// vendor's are recorded in the design document.
#[test]
fn the_recorded_source_report_is_quiet_and_inside_the_registry() {
    let report = a_recorded_report("fetch.run.1");

    assert_eq!((report.stage(), report.item()), ("fetch", "run"));
    assert_eq!(
        report.findings().len(),
        33,
        "the rate, both halves of the travel pair, the excursion, 17 tails \
         and 4 readings at 3 frames"
    );
    assert!(!report.has_errors(), "a clip that is what the library says");
    assert_eq!(report.off_registry(&profile()), Vec::<String>::new());

    // The travel pair: `travels: false` picks which half reads, and the
    // other says so rather than going quiet.
    let travel = |rule: &str| {
        report
            .findings()
            .iter()
            .find(|finding| finding.rule == rule)
            .unwrap_or_else(|| panic!("{rule} reported nothing"))
    };
    assert!(travel("source.in_place").measured < 0.02, "run stays put");
    assert_eq!(travel("source.traveling").severity, Severity::Skipped);

    // And the excursion the endpoint cancels, which nothing downstream can
    // read: a run cycle sways 0.0276 m sideways and comes back.
    let wandered = travel("source.wander");
    assert_eq!(wandered.severity, Severity::Info);
    assert!((wandered.measured - 0.0276).abs() < 0.0001, "{wandered:?}");
}

/// One clean finding of a published rule, for the negatives below to vary.
fn a_clip_finding() -> Finding {
    clip::INTERPOLATION.measured(&profile(), "Hips", 0.0, 1, "stub".to_owned())
}

/// Three `[synth]` negatives for the runner's half: a rule nothing
/// published, a published rule reported against a limit of its own, and a
/// defect filed as information. The last is the worst of the three, because
/// the runner's own gate reads severity: four channels left on Bezier called
/// `info` would exit 0 with the number sitting in the report.
#[test]
fn a_report_that_disagrees_with_the_rule_list_is_named_back() {
    let mut report = Report::new("retarget", "run", 1);
    for finding in [
        Finding {
            rule: "clip.made_up".to_owned(),
            ..a_finding()
        },
        Finding {
            comparison: Comparison::Le,
            limit: 5.0,
            ..a_clip_finding()
        },
        Finding {
            severity: Severity::Info,
            measured: 4.0,
            ..a_clip_finding()
        },
    ] {
        report.add(finding).unwrap();
    }

    let off = report.off_registry(&profile());

    assert_eq!(off.len(), 3, "{off:#?}");
    assert!(off[0].contains("clip.made_up"), "{off:#?}");
    assert!(
        off[1].contains("comparison") && off[1].contains("limit"),
        "{off:#?}"
    );
    assert!(
        off[2].contains("severity") && off[2].contains("Info at 4"),
        "{off:#?}"
    );
}

/// The other three severities. A number decides `info` and `error` and
/// nothing else: a warning says the measurement could not be taken and a
/// skip says a declaration switched the rule off, so neither follows from
/// one.
#[test]
fn a_warning_and_a_skip_are_not_held_to_the_comparison() {
    let mut report = Report::new("retarget", "run", 1);
    for severity in [Severity::Warning, Severity::Skipped] {
        report
            .add(Finding {
                severity,
                measured: 4.0,
                ..a_clip_finding()
            })
            .unwrap();
    }

    assert_eq!(report.off_registry(&profile()), Vec::<String>::new());
}

/// An undefined measurement is the one shape that cannot carry its rule's
/// unit or limit: two coincident joints have no angle, so `degrees` and 180
/// would both be a lie. Both sides write it the same way, and the registry
/// has to read it as agreeing rather than as five disagreements at once.
#[test]
fn an_undefined_measurement_agrees_with_the_rule_that_could_not_be_taken() {
    let mut report = Report::new("fetch", "strafe_left", 1);
    report
        .add(source::CHILD_AXIS.undefined("neck", 1, "no direction".to_owned()))
        .unwrap();

    assert_eq!(report.off_registry(&profile()), Vec::<String>::new());
    assert!(report.has_errors(), "and it is still a defect");
}

/// It is the shape that is recognized, not the id: a finding claiming to be
/// undefined on another rule's space is still off the list.
#[test]
fn an_undefined_measurement_read_on_the_wrong_space_is_still_named_back() {
    let mut report = Report::new("fetch", "strafe_left", 1);
    report
        .add(Finding {
            measured_on: "somewhere else".to_owned(),
            ..source::CHILD_AXIS.undefined("neck", 1, "no direction".to_owned())
        })
        .unwrap();

    let off = report.off_registry(&profile());
    assert_eq!(off.len(), 1, "{off:#?}");
    assert!(off[0].contains("measured_on"), "{off:#?}");
}

/// And the clean finding itself, so the check is not passing everything.
#[test]
fn a_defect_reported_as_an_error_agrees_with_the_rule_list() {
    let mut report = Report::new("retarget", "run", 1);
    report
        .add(clip::INTERPOLATION.measured(&profile(), "Hips", 4.0, 1, "stub".to_owned()))
        .unwrap();

    assert_eq!(report.off_registry(&profile()), Vec::<String>::new());
    assert!(report.has_errors(), "the comparison decided that");
}

/// Every rule in that list has to report something on real art, or a rule
/// that measures nothing at all cannot be told from one that never ran. The
/// list is the whole registry, so this covers the next family too.
#[test]
fn every_rule_in_the_list_reports_on_the_committed_art() {
    let root = repo_root();
    let profile = Profile::of(&root, HUMANOID).unwrap();
    let table = AimTable::of(&root, HUMANOID).unwrap();
    let rig_glb = committed_glb("art/skeletons/humanoid.glb");
    // The mesh gates run on the bare mesh, and the rigged file stands in
    // until that one can be downloaded.
    let mesh_glb = committed_glb("art/characters/survivor/model.glb");

    // `clip.swing` and `clip.twist` need the vendor file the motion was
    // bought in, and that one may not be redistributed, so their subject here
    // is the synthetic cross-rig pair. `clip.object_transform` runs on the
    // committed clip against the committed rig.
    let bones = table.bones(table.canonical()).unwrap().clone();
    let pair = crate::clips::CrossRig::new(bones.clone());
    let clip_glb = committed_glb("art/animations/run.glb");
    let run_keys = gltf_clip::keys(&std::fs::read(&clip_glb).unwrap()).unwrap();
    let findings = [
        rig::check_file(&rig_glb, &root, &profile, HEIGHT_METERS, 1).unwrap(),
        aim::check_file(&rig_glb, &root, &profile, &table, table.canonical(), 1).unwrap(),
        mesh::check_file(&mesh_glb, &root, &profile, HEIGHT_METERS, None, 1).unwrap(),
        a_recorded_report("retarget.run.1").findings().to_vec(),
        a_recorded_report("fetch.run.1").findings().to_vec(),
        a_recorded_report("bake.survivor.1").findings().to_vec(),
        clip::compare(
            &gltf_clip::read(&pair.output_glb(), &bones).unwrap(),
            &Motion::parse(&pair.source_motion()).unwrap(),
            &bones,
            &profile,
            1,
        ),
        clip::object_transform(
            &Skeleton::read(&clip_glb).unwrap(),
            &Skeleton::read(&rig_glb).unwrap(),
            &profile,
            1,
        ),
        clip::closes_the_loop(&run_keys, true, &profile, 1),
    ]
    .concat();

    for rule in every_rule() {
        assert!(
            findings.iter().any(|finding| finding.rule == rule.id),
            "{} reported nothing at all",
            rule.id
        );
    }
}

// --- the record -----------------------------------------------------------

#[test]
fn a_finding_round_trips_through_the_json_the_python_side_writes() {
    let json = serde_json::to_string(&a_finding()).unwrap();

    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&json).unwrap()["comparison"],
        "le",
        "the two sides share one spelling: {json}"
    );
    assert_eq!(serde_json::from_str::<Finding>(&json).unwrap(), a_finding());
}

#[test]
fn a_finding_with_no_comparison_cannot_be_parsed() {
    let mut fields: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&a_finding()).unwrap()).unwrap();
    fields.as_object_mut().unwrap().remove("comparison");

    let error = serde_json::from_value::<Finding>(fields)
        .unwrap_err()
        .to_string();
    assert!(error.contains("comparison"), "got: {error}");
}

#[test]
fn a_finding_with_an_unknown_field_cannot_be_parsed() {
    let json = r#"{"rule":"clip.swing","severity":"error","subject":"LeftHand",
        "measured":1.0,"limit":0.0,"comparison":"le","unit":"degrees",
        "attempt":1,"measured_on":"world space","message":"x","waived":true}"#;

    let error = serde_json::from_str::<Finding>(json)
        .unwrap_err()
        .to_string();
    assert!(error.contains("waived"), "got: {error}");
}

#[test]
fn a_rule_that_names_no_space_is_refused() {
    let error = added(Finding {
        measured_on: "   ".to_owned(),
        ..a_finding()
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("measured_on"), "got: {error}");
}

#[test]
fn a_finding_missing_any_of_its_words_is_refused() {
    for field in ["rule", "subject", "unit", "message"] {
        let mut finding = a_finding();
        match field {
            "rule" => finding.rule = String::new(),
            "subject" => finding.subject = String::new(),
            "unit" => finding.unit = String::new(),
            _ => finding.message = String::new(),
        }
        let error = added(finding).unwrap_err().to_string();
        assert!(error.contains(field), "{field}: got {error}");
    }
}

#[test]
fn a_gate_can_never_emit_nan() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(
            added(Finding {
                measured: value,
                ..a_finding()
            })
            .is_err(),
            "an undefined measurement is an error with a message, not {value}"
        );
        assert!(
            added(Finding {
                limit: value,
                ..a_finding()
            })
            .is_err()
        );
    }
}

#[test]
fn the_first_attempt_is_one() {
    assert!(
        Report::new("clip", "run", 0)
            .add(Finding {
                attempt: 0,
                ..a_finding()
            })
            .is_err()
    );
}

#[test]
fn the_comparison_decides_whether_a_measurement_holds() {
    for (comparison, measured, holds) in [
        (Comparison::Le, 8.0, true),
        (Comparison::Le, 8.1, false),
        (Comparison::Lt, 8.0, false),
        (Comparison::Lt, 7.9, true),
        (Comparison::Eq, 8.0, true),
        (Comparison::Eq, 7.9, false),
        (Comparison::Ge, 8.0, true),
        (Comparison::Ge, 7.9, false),
    ] {
        let finding = Finding {
            comparison,
            measured,
            limit: 8.0,
            ..a_finding()
        };
        assert_eq!(finding.holds(), holds, "{comparison:?} {measured} vs 8.0");
    }
}

/// A passing measurement and a rule a declaration switched off both belong in
/// the report, and a consumer has to tell them apart by severity rather than
/// by reading the message. `symmetry: false` is the case that needs it.
#[test]
fn a_measurement_that_passed_and_a_rule_that_was_switched_off_differ() {
    let passed = Finding {
        severity: Severity::Info,
        ..a_finding()
    };
    let switched_off = Finding {
        severity: Severity::Skipped,
        measured: 0.0,
        message: "spec.subject.symmetry is false".to_owned(),
        ..a_finding()
    };

    assert_ne!(passed.severity, switched_off.severity);
    assert_eq!(
        serde_json::to_value(switched_off.severity).unwrap(),
        "skipped"
    );
    // Neither stops a build, which is what `error` alone does.
    let mut report = Report::new("mesh", "survivor", 1);
    report.extend([passed, switched_off]).unwrap();
    assert_eq!(report.exit_code(), 0);
}

/// The whole reason `comparison` is a required field: 8 le 8 passes a fixer
/// that changed nothing, and 8 lt 8 does not.
#[test]
fn a_fixer_that_changed_nothing_fails_the_strict_comparison() {
    assert!(Comparison::Le.holds(8.0, 8.0));
    assert!(!Comparison::Lt.holds(8.0, 8.0));
}

// --- the report -----------------------------------------------------------

#[test]
fn a_report_is_non_zero_only_when_an_error_is_present() {
    let mut report = Report::new("bake", "survivor", 1);
    for severity in [Severity::Warning, Severity::Info, Severity::Skipped] {
        report
            .add(Finding {
                severity,
                ..a_finding()
            })
            .unwrap();
    }
    assert!(!report.has_errors());
    assert_eq!(report.exit_code(), 0);

    report.add(a_finding()).unwrap();
    assert!(report.has_errors());
    assert_eq!(report.exit_code(), 1);
}

#[test]
fn an_empty_report_passes() {
    let report = Report::new("bake", "survivor", 1);
    assert_eq!(report.exit_code(), 0);
    assert!(report.findings().is_empty());
    assert_eq!(report.stage(), "bake");
    assert_eq!(report.attempt(), 1);
}

#[test]
fn a_report_refuses_a_finding_from_another_attempt() {
    let error = Report::new("concept", "survivor", 2)
        .add(a_finding())
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("attempt 1") && error.contains("attempt 2"),
        "all three attempts must survive separately: {error}"
    );
}

#[test]
fn a_report_lands_in_its_own_numbered_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut report = Report::new("concept", "survivor", 2);
    report
        .add(Finding {
            attempt: 2,
            ..a_finding()
        })
        .unwrap();

    let path = report.write(dir.path()).unwrap();

    assert_eq!(
        path,
        dir.path()
            .join("art/staging/reports/concept.survivor.2.json"),
        "a retry must not overwrite the attempt before it"
    );
    let read = Report::read(&path).unwrap();
    assert_eq!(read.attempt(), 2);
    assert_eq!(read.findings(), report.findings());
}

#[test]
fn a_report_the_python_side_wrote_is_re_validated() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bake.1.json");
    // What a Blender script would emit if its space went missing.
    std::fs::write(
        &path,
        r#"{"stage":"bake","item":"survivor","attempt":1,"findings":[{"rule":"bake.pivot",
           "severity":"error","subject":"s","measured":1.0,"limit":0.0,
           "comparison":"le","unit":"px","attempt":1,"measured_on":"",
           "message":"x"}]}"#,
    )
    .unwrap();

    let error = format!("{:#}", Report::read(&path).unwrap_err());
    assert!(error.contains("measured_on"), "got: {error}");
}

#[test]
fn a_report_claiming_no_stage_is_refused_on_the_way_off_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bake.1.json");
    std::fs::write(
        &path,
        r#"{"stage":"","item":"survivor","attempt":1,"findings":[]}"#,
    )
    .unwrap();

    let error = format!("{:#}", Report::read(&path).unwrap_err());
    assert!(error.contains("stage"), "got: {error}");
}

#[test]
fn an_unreadable_report_is_reported_by_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bake.1.json");
    let error = format!("{:#}", Report::read(&path).unwrap_err());
    assert!(error.contains("bake.1.json"), "got: {error}");

    std::fs::write(&path, "not json").unwrap();
    let error = format!("{:#}", Report::read(&path).unwrap_err());
    assert!(error.contains("bake.1.json"), "got: {error}");
}

#[test]
fn extending_a_report_stops_at_the_first_refusal() {
    let mut report = Report::new("clip", "run", 1);
    let error = report
        .extend([
            a_finding(),
            Finding {
                measured: f64::NAN,
                ..a_finding()
            },
            a_finding(),
        ])
        .unwrap_err()
        .to_string();

    assert!(error.contains("finite"), "got: {error}");
    assert_eq!(report.findings().len(), 1);
}

// --- the artifacts --------------------------------------------------------

#[test]
fn every_artifact_of_one_attempt_shares_its_numbered_name() {
    let artifacts = Artifacts::new(Path::new("/repo"), "bake", "survivor", 3).unwrap();
    let reports = Path::new("/repo/art/staging/reports");

    assert_eq!(artifacts.dir(), reports);
    assert_eq!(artifacts.report(), reports.join("bake.survivor.3.json"));
    assert_eq!(
        artifacts.sentinel(),
        reports.join("bake.survivor.3.sentinel.json")
    );
    assert_eq!(artifacts.argv(), reports.join("bake.survivor.3.argv.txt"));
    assert_eq!(artifacts.log(), reports.join("bake.survivor.3.log"));
    assert_eq!(artifacts.blend(), reports.join("bake.survivor.3.blend"));
}

#[test]
fn a_report_knows_where_its_own_artifacts_go() {
    let artifacts = Report::new("fetch", "strafe_left", 1)
        .artifacts(Path::new("/repo"))
        .unwrap();
    assert_eq!(
        artifacts.report(),
        Path::new("/repo/art/staging/reports/fetch.strafe_left.1.json")
    );
}

/// One stage runs once per clip, so without the item in the name a
/// three-clip download would keep one log and lose two.
#[test]
fn two_items_of_one_stage_never_share_a_file() {
    let reports = Path::new("/repo/art/staging/reports");
    let idle = Artifacts::new(Path::new("/repo"), "download", "idle", 1).unwrap();
    let run = Artifacts::new(Path::new("/repo"), "download", "run", 1).unwrap();

    assert_eq!(idle.log(), reports.join("download.idle.1.log"));
    assert_ne!(idle.log(), run.log());
    assert_ne!(idle.report(), run.report());
    assert_ne!(idle.sentinel(), run.sentinel());
    assert_ne!(idle.argv(), run.argv());
    assert_ne!(idle.blend(), run.blend());
}

#[test]
fn a_report_with_no_stage_or_no_item_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    for (stage, item) in [("", "survivor"), ("bake", "  ")] {
        assert!(
            Report::new(stage, item, 1).write(dir.path()).is_err(),
            "stage {stage:?} item {item:?}"
        );
    }
    assert!(
        Report::new("bake", "survivor", 0)
            .write(dir.path())
            .is_err()
    );
}

/// The dot separates the parts, and the Blender scripts read the header back
/// off the name, so a part carrying one would make the name ambiguous.
#[test]
fn a_stage_or_an_item_holding_a_dot_is_refused() {
    let root = Path::new("/repo");
    for (stage, item) in [("bake.1", "survivor"), ("bake", "survivor.old")] {
        let error = Artifacts::new(root, stage, item, 1)
            .unwrap_err()
            .to_string();
        assert!(error.contains("dot"), "{stage}/{item}: {error}");
    }
    assert!(Artifacts::new(root, "bake", "survivor", 0).is_err());
    assert!(Artifacts::new(root, "", "survivor", 1).is_err());
}

// --- the two implementations ----------------------------------------------

/// `findings.py` declares the same record, and the Rust side parses what it
/// writes. Parsed rather than copied, because a copy here would be a third
/// list to keep in step. Embedded and not read, so a moved file fails the
/// build.
const FINDINGS_PY: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/blender/src/findings.py"
));

/// The annotated field names of one pydantic model, in order.
fn python_fields(class: &str) -> Vec<String> {
    body_of(class)
        .lines()
        .filter_map(|line| line.strip_prefix("    ")?.split_once(": "))
        .filter(|(name, _)| name.chars().all(|c| c.is_ascii_lowercase() || c == '_'))
        .map(|(name, _)| name.to_owned())
        .collect()
}

/// The wire values of one `StrEnum`, in order.
fn python_enum(class: &str) -> Vec<String> {
    body_of(class)
        .lines()
        .filter_map(|line| line.strip_prefix("    ")?.split_once(" = "))
        .filter_map(|(_, value)| value.trim().strip_prefix('"')?.strip_suffix('"'))
        .map(str::to_owned)
        .collect()
}

/// One class body: everything up to the next top-level definition, and up to
/// the first method or decorator inside it.
fn body_of(class: &str) -> &'static str {
    let after = FINDINGS_PY
        .split_once(&format!("\nclass {class}("))
        .unwrap_or_else(|| panic!("findings.py must declare {class}"))
        .1;
    let end = ["\nclass ", "\ndef ", "\n    @", "\n    def "]
        .iter()
        .filter_map(|marker| after.find(marker))
        .min()
        .unwrap_or(after.len());
    &after[..end]
}

#[test]
fn the_python_finding_has_exactly_the_fields_rust_parses() {
    let json = serde_json::to_value(a_finding()).unwrap();
    let rust: Vec<String> = json.as_object().unwrap().keys().cloned().collect();

    let mut python = python_fields("Finding");
    python.sort_unstable();

    assert_eq!(
        rust, python,
        "a field renamed on one side must be a red test, not a silent skip"
    );
}

#[test]
fn the_python_report_has_exactly_the_fields_rust_parses() {
    let json = serde_json::to_value(Report::new("bake", "survivor", 1)).unwrap();
    let rust: Vec<String> = json.as_object().unwrap().keys().cloned().collect();

    let mut python = python_fields("Report");
    python.sort_unstable();

    assert_eq!(rust, python);
}

/// The three Blender modules that declare a `Rule` of their own.
const BLENDER_MODULES: [&str; 3] = [
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tools/blender/src/clip.py"
    )),
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tools/blender/src/framing.py"
    )),
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tools/blender/src/source.py"
    )),
];

/// Module-level string constants, including the ones written as several
/// adjacent literals inside brackets, which is how a long space is spelled.
fn python_constants(module: &str) -> BTreeMap<String, String> {
    let mut constants = BTreeMap::new();
    let mut lines = module.lines();
    while let Some(line) = lines.next() {
        let Some((name, rest)) = line.split_once(" = ") else {
            continue;
        };
        if !name.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
            continue;
        }
        if let Some(text) = quoted(rest) {
            constants.insert(name.to_owned(), text);
        } else if rest == "(" {
            let mut joined = String::new();
            for part in lines.by_ref().take_while(|line| *line != ")") {
                joined.push_str(&quoted(part.trim()).unwrap_or_default());
            }
            constants.insert(name.to_owned(), joined);
        }
    }
    constants
}

/// The contents of the first double-quoted string in `text`.
fn quoted(text: &str) -> Option<String> {
    let rest = text.strip_prefix('"')?;
    Some(rest[..rest.find('"')?].to_owned())
}

/// Every `Rule(...)` one module declares: its id, its unit and the space it
/// says it measured on, with module-level constants resolved.
fn python_rules(module: &str) -> BTreeMap<String, (String, String)> {
    let constants = python_constants(module);
    module
        .split("= Rule(")
        .skip(1)
        .map(|block| {
            let body = &block[..block.find("\n)").expect("a closed Rule(")];
            let field = |key: &str| {
                let after = body
                    .split_once(&format!("{key}="))
                    .unwrap_or_else(|| panic!("a Rule with no {key}: {body}"))
                    .1;
                quoted(after).unwrap_or_else(|| {
                    let name: String = after
                        .chars()
                        .take_while(|c| c.is_ascii_uppercase() || *c == '_')
                        .collect();
                    constants
                        .get(&name)
                        .unwrap_or_else(|| panic!("{name} is not a module constant"))
                        .clone()
                })
            };
            (field("id"), (field("unit"), field("measured_on")))
        })
        .collect()
}

/// A script builds its Findings through a `Rule` of its own, so an id, a unit
/// or a space spelled differently there is a report `off_registry` refuses at
/// run time. This is the same disagreement, at build time, where a typo is
/// one failing test rather than a wasted Blender run.
#[test]
fn every_blender_rule_says_what_the_registry_publishes() {
    let spelled: BTreeMap<String, (String, String)> = BLENDER_MODULES
        .iter()
        .flat_map(|m| python_rules(m))
        .collect();
    let published: BTreeMap<String, (String, String)> = every_rule()
        .filter(|rule| spelled.contains_key(rule.id))
        .map(|rule| {
            (
                rule.id.to_owned(),
                (rule.unit.to_owned(), rule.space.to_owned()),
            )
        })
        .collect();

    assert_eq!(spelled, published);
    assert_eq!(
        spelled.len(),
        12,
        "4 at the retarget, 2 at the bake and 6 at the fetch"
    );
}

#[test]
fn both_sides_spell_the_severities_and_comparisons_the_same() {
    let sorted = |mut values: Vec<String>| {
        values.sort_unstable();
        values
    };
    let spelled = |values: &[serde_json::Value]| {
        sorted(
            values
                .iter()
                .map(|v| v.as_str().expect("a string").to_owned())
                .collect(),
        )
    };
    let severities = [
        Severity::Error,
        Severity::Warning,
        Severity::Info,
        Severity::Skipped,
    ]
    .map(|s| serde_json::to_value(s).unwrap());
    let comparisons = [
        Comparison::Le,
        Comparison::Lt,
        Comparison::Eq,
        Comparison::Ge,
    ]
    .map(|c| serde_json::to_value(c).unwrap());

    assert_eq!(spelled(&severities), sorted(python_enum("Severity")));
    assert_eq!(spelled(&comparisons), sorted(python_enum("Comparison")));
}

// --- what the fetch boundary hands Blender --------------------------------

/// `source.child_axis` measures each bone's own axis against the direction to
/// its mapped child, and `[profile.tails]` is what chooses which child that
/// is: `Hips` has three and only one of them continues the body.
#[test]
fn every_mapped_child_comes_off_the_profile_s_own_tails() {
    let root = repo_root();
    let profile = profile();
    let table = AimTable::of(&root, HUMANOID).unwrap();
    let children = source::mapped_children(&profile, table.bones(table.canonical()).unwrap());

    assert_eq!(
        children.len(),
        17,
        "18 tails, and `Head` points at `head_end`, which fills no role"
    );
    // The two the design records a Mixamo reading for, at 7.051 and 16.933.
    assert_eq!(children["hips"], "spine_lower");
    assert_eq!(children["neck"], "head");
    assert!(!children.contains_key("head"), "no role fills `head_end`");
}
