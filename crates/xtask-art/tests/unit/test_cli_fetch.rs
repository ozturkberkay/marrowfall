//! `cargo art fetch`: what it decides to do, and what it does.
//!
//! The provider is served locally and Blender is a shell stub, so the whole
//! path runs with no network, no browser and no Blender.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use xtask_art::check::profile::Profile;
use xtask_art::check::{Finding, Report, Rule, Severity, clip, foot, source};
use xtask_art::cli::{FetchStep, OnDisk, fetch, fetch_plan};
use xtask_art::library::{
    Animation, AnimationLibrary, ClipFiles, HUMANOID, LibraryLock, MotionSource, Verdict,
};
use xtask_art::lock::blender_inputs;
use xtask_art::providers::mixamo::client::CHARACTER_ID;

use crate::support::{BLENDER, EnvGuard, a_tree, edit_an_aim_row};

const PRODUCT: &str = "c9ccc468-b96c-11e4-a802-0aaa78deedf9";

/// The template library plus one clip that has to be fetched.
///
/// `source_fps` and `loops` describe the synthetic cross-rig pair the Blender
/// stub hands back rather than a real Mixamo clip: it runs 8 frames at 24 fps
/// off an open curve, so its two ends do not meet.
fn a_library() -> AnimationLibrary {
    let mut library = AnimationLibrary::template();
    library.animations.insert(
        "walk_back".to_owned(),
        Animation {
            skeleton: HUMANOID.to_owned(),
            loops: false,
            fps: 20,
            source_fps: 24,
            travels: true,
            source: MotionSource::Mixamo {
                product_id: PRODUCT.to_owned(),
            },
        },
    );
    library
}

fn an_fbx() -> Vec<u8> {
    let mut fbx = b"Kaydara FBX Binary  \x00".to_vec();
    fbx.resize(60_000, 0x42);
    fbx
}

/// Why the plan wants one clip fetched, which is what its step has to say.
fn why(steps: &[FetchStep], name: &str) -> String {
    steps
        .iter()
        .find_map(|step| match step {
            FetchStep::Fetch { name: got, why, .. } if got == name => Some((*why).to_owned()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{name} is not fetched: {steps:?}"))
}

fn wants(steps: &[FetchStep], name: &str) -> bool {
    steps
        .iter()
        .any(|step| matches!(step, FetchStep::Fetch { name: fetched, .. } if fetched == name))
}

/// What the plan reads off disk: the clips that are there, and the rig every
/// one of them would be fitted by.
fn on_disk(entries: &[(&str, &str)]) -> OnDisk {
    OnDisk {
        glb: entries
            .iter()
            .map(|(name, digest)| ((*name).to_owned(), (*digest).to_owned()))
            .collect(),
        retarget: BTreeMap::from([(HUMANOID.to_owned(), FITTED_BY.to_owned())]),
    }
}

/// The rig and tooling fingerprint a recorded fit was made with.
const FITTED_BY: &str = "fedcba9876543210";

/// What a passing retarget records.
fn a_verdict() -> Verdict {
    Verdict {
        report: "retarget.walk_back.1".to_owned(),
        worst: Severity::Info,
        rules: vec![clip::INTERPOLATION.id.to_owned()],
    }
}

/// One recorded fetch, as a passing retarget leaves it.
fn a_record(glb: &[u8]) -> LibraryLock {
    let mut lock = LibraryLock::default();
    lock.record(
        "walk_back",
        MotionSource::Mixamo {
            product_id: PRODUCT.to_owned(),
        },
        ClipFiles {
            download: b"the download",
            glb,
        },
        FITTED_BY,
        a_verdict(),
    );
    lock
}

// --- Planning -------------------------------------------------------------

#[test]
fn with_no_names_everything_missing_is_fetched() {
    let steps = fetch_plan(
        &a_library(),
        &LibraryLock::default(),
        &[],
        &on_disk(&[]),
        false,
    )
    .unwrap();

    assert!(wants(&steps, "walk_back"));
    assert_eq!(steps.len(), 3, "one step per library entry");
}

#[test]
fn motion_that_arrives_another_way_is_skipped_with_the_reason() {
    let mut library = a_library();
    library.animations.insert(
        "wave".to_owned(),
        Animation {
            skeleton: HUMANOID.to_owned(),
            loops: false,
            fps: 12,
            source_fps: 24,
            travels: false,
            source: MotionSource::Authored,
        },
    );

    let steps = fetch_plan(&library, &LibraryLock::default(), &[], &on_disk(&[]), false).unwrap();
    let reason = |name: &str| {
        steps
            .iter()
            .find_map(|step| match step {
                FetchStep::Skipped(skipped, why) if skipped == name => Some(*why),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{name} was not skipped: {steps:?}"))
    };

    assert!(reason("run").contains("rig stage"), "bought, not fetched");
    assert!(reason("wave").contains("authored"));
}

#[test]
fn a_clip_already_fetched_is_left_alone() {
    let lock = a_record(b"the glb");
    let digest = lock.fetched["walk_back"].glb.clone();

    let steps = fetch_plan(
        &a_library(),
        &lock,
        &["walk_back".to_owned()],
        &on_disk(&[("walk_back", &digest)]),
        false,
    )
    .unwrap();

    assert_eq!(steps, vec![FetchStep::Cached("walk_back".to_owned())]);
}

/// A record with no verdict never passed a gate anyone kept, so the file on
/// disk is not evidence of anything.
#[test]
fn a_clip_recorded_without_a_verdict_is_fetched_again() {
    let mut lock = a_record(b"the glb");
    let digest = lock.fetched["walk_back"].glb.clone();
    lock.fetched.get_mut("walk_back").unwrap().verdict = None;

    let steps = fetch_plan(
        &a_library(),
        &lock,
        &["walk_back".to_owned()],
        &on_disk(&[("walk_back", &digest)]),
        false,
    )
    .unwrap();

    assert!(why(&steps, "walk_back").contains("verdict"), "{steps:?}");
}

#[test]
fn a_clip_whose_recorded_verdict_failed_is_fetched_again() {
    let mut lock = a_record(b"the glb");
    let digest = lock.fetched["walk_back"].glb.clone();
    if let Some(verdict) = &mut lock.fetched.get_mut("walk_back").unwrap().verdict {
        verdict.worst = Severity::Error;
    }

    let steps = fetch_plan(
        &a_library(),
        &lock,
        &["walk_back".to_owned()],
        &on_disk(&[("walk_back", &digest)]),
        false,
    )
    .unwrap();

    assert!(why(&steps, "walk_back").contains("failed"), "{steps:?}");
}

/// Replacing the canonical rig, its profile, a script or Blender itself means
/// every fit on disk was made by something else.
#[test]
fn a_clip_fitted_by_another_rig_is_fetched_again() {
    let mut lock = a_record(b"the glb");
    let digest = lock.fetched["walk_back"].glb.clone();
    lock.fetched.get_mut("walk_back").unwrap().fingerprint = "0000000000000000".to_owned();

    let steps = fetch_plan(
        &a_library(),
        &lock,
        &["walk_back".to_owned()],
        &on_disk(&[("walk_back", &digest)]),
        false,
    )
    .unwrap();

    assert!(why(&steps, "walk_back").contains("fitted"), "{steps:?}");
}

/// The design's criterion, end to end: an `[aim_table]` row is what the
/// transfer reads, so editing one sends every recorded clip back to be
/// fitted again.
#[test]
fn an_aim_table_row_sends_every_recorded_clip_back_to_be_fitted() {
    let tree = a_tree();
    let root = tree.path();
    let library = a_library();
    let before = blender_inputs(root, HUMANOID, BLENDER).unwrap();

    let mut lock = LibraryLock::default();
    lock.record(
        "walk_back",
        MotionSource::Mixamo {
            product_id: PRODUCT.to_owned(),
        },
        ClipFiles {
            download: b"the download",
            glb: b"the glb",
        },
        &before,
        a_verdict(),
    );
    let digest = lock.fetched["walk_back"].glb.clone();
    let disk = |fitted_by: &str| OnDisk {
        glb: BTreeMap::from([("walk_back".to_owned(), digest.clone())]),
        retarget: BTreeMap::from([(HUMANOID.to_owned(), fitted_by.to_owned())]),
    };
    let plan = |fitted_by: &str| {
        fetch_plan(
            &library,
            &lock,
            &["walk_back".to_owned()],
            &disk(fitted_by),
            false,
        )
        .unwrap()
    };

    assert_eq!(
        plan(&before),
        vec![FetchStep::Cached("walk_back".to_owned())],
        "nothing moved yet"
    );

    edit_an_aim_row(root);

    let after = blender_inputs(root, HUMANOID, BLENDER).unwrap();
    assert_ne!(after, before, "one [aim_table] row moved what fits a clip");
    let steps = plan(&after);
    assert!(why(&steps, "walk_back").contains("fitted"), "{steps:?}");
}

/// Nothing measured the rig, so nothing can tell a finished fetch from one
/// made by a different one. Refused rather than guessed at.
#[test]
fn a_skeleton_nobody_fingerprinted_is_an_error() {
    let error = fetch_plan(
        &a_library(),
        &a_record(b"the glb"),
        &["walk_back".to_owned()],
        &OnDisk::default(),
        false,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("humanoid"), "got: {error}");
}

#[test]
fn a_file_nobody_recorded_is_reported_rather_than_overwritten() {
    let steps = fetch_plan(
        &a_library(),
        &LibraryLock::default(),
        &["walk_back".to_owned()],
        &on_disk(&[("walk_back", "0123456789abcdef")]),
        false,
    )
    .unwrap();

    assert_eq!(steps, vec![FetchStep::Changed("walk_back".to_owned())]);
}

#[test]
fn force_fetches_again_whatever_is_on_disk() {
    let steps = fetch_plan(
        &a_library(),
        &LibraryLock::default(),
        &["walk_back".to_owned()],
        &on_disk(&[("walk_back", "0123456789abcdef")]),
        true,
    )
    .unwrap();

    assert!(wants(&steps, "walk_back"));
}

#[test]
fn a_name_the_library_does_not_declare_lists_what_it_does() {
    let error = fetch_plan(
        &a_library(),
        &LibraryLock::default(),
        &["moonwalk".to_owned()],
        &on_disk(&[]),
        false,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("moonwalk"), "got: {error}");
    assert!(
        error.contains("walk_back"),
        "lists the alternatives: {error}"
    );
}

// --- Fetching -------------------------------------------------------------

/// A repo with the retarget script, a virtualenv, a canonical rig and the
/// skeleton file beside it. The real one: the runner reads `[profile]` out of
/// it to hold the script's report to the published rule list.
fn a_repo(library: &AnimationLibrary) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("tools/blender/src")).unwrap();
    for script in ["check_source.py", "retarget_animation.py"] {
        std::fs::write(root.join("tools/blender/src").join(script), "").unwrap();
    }
    std::fs::create_dir_all(root.join(".venv/lib/python3.13/site-packages")).unwrap();

    // A real rig, because `clip.object_transform` reads the clip's own object
    // nodes against this file's.
    let rig = AnimationLibrary::reference_rig(root, HUMANOID);
    std::fs::create_dir_all(rig.parent().unwrap()).unwrap();
    std::fs::write(&rig, crate::rigs::SyntheticRig::conformant().to_gltf()).unwrap();
    let skeleton = rig.with_extension("toml");
    std::fs::copy(
        crate::support::repo_root().join("art/skeletons/humanoid.toml"),
        skeleton,
    )
    .unwrap();
    library.save(root).unwrap();
    dir
}

/// The report writing, as the real script looks like from outside: the
/// header comes off the path the runner set, and one finding per rule stands
/// in for the 69 the retarget and the 33 the source check really write.
///
/// Each stage defaults to the clean set of every rule it owns, kept on a
/// file, because the runner refuses either script when it leaves one of its
/// own rules unread. A test overrides one stage's whole list.
fn writes_a_report(dir: &Path) -> String {
    let fitted = dir.join("retarget-findings.json");
    let vendor = dir.join("fetch-findings.json");
    std::fs::write(&fitted, a_retarget_report(0.0)).unwrap();
    std::fs::write(&vendor, as_findings(&a_source_report())).unwrap();
    format!(
        r#"
name=$(basename "$MARROWFALL_REPORT" .json)
stage=${{name%%.*}}; rest=${{name#*.}}; item=${{rest%.*}}; attempt=${{rest##*.}}
if [ "$stage" = "fetch" ]; then
  if [ -n "$MARROWFALL_STUB_SOURCE_FINDINGS" ]; then
    findings="$MARROWFALL_STUB_SOURCE_FINDINGS"
  else findings=$(cat "{vendor}"); fi
elif [ -n "$MARROWFALL_STUB_FINDINGS" ]; then findings="$MARROWFALL_STUB_FINDINGS"
else findings=$(cat "{fitted}"); fi
printf '{{"stage":"%s","item":"%s","attempt":%s,"findings":[%s]}}' \
  "$stage" "$item" "$attempt" "$findings" > "$MARROWFALL_REPORT"
"#,
        fitted = fitted.display(),
        vendor = vendor.display()
    )
}

/// The fetch path runs two scripts, and a stub testing the second one still
/// has to let the first through with a clean report of its own.
fn passes_the_source_check(dir: &Path) -> String {
    let vendor = dir.join("passing-fetch-findings.json");
    std::fs::write(&vendor, as_findings(&a_source_report())).unwrap();
    format!(
        r#"
case "${{MARROWFALL_REPORT##*/}}" in
  fetch.*)
    printf '{{"stage":"fetch","item":"walk_back","attempt":1,"findings":[%s]}}' \
      "$(cat "{vendor}")" > "$MARROWFALL_REPORT"
    : > "$MARROWFALL_SENTINEL"
    exit 0
    ;;
esac
"#,
        vendor = vendor.display()
    )
}

/// One executable stub, named after what it does wrong.
///
/// Every one answers `--version` first, because a fetch reads the Blender
/// build before it fits anything.
fn a_stub(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
    let stub = dir.join(name);
    std::fs::write(
        &stub,
        format!("#!/bin/sh\n{}{body}", crate::support::answers_its_version()),
    )
    .unwrap();
    std::fs::set_permissions(
        &stub,
        <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
    )
    .unwrap();
    stub
}

/// Blender as the fetch path uses it: it writes the clip, the source motion
/// beside it, the report and the sentinel.
///
/// The clip and the sidecar are the synthetic cross-rig pair, standing, so
/// every file-side `clip.*` rule measures a real fit rather than a stub
/// string and the three foot contact ones have a foot that plants.
fn a_blender_stub(dir: &Path) -> std::path::PathBuf {
    a_blender_stub_of(dir, &crate::clips::CrossRig::new(a_convention()).standing())
}

/// The same stub handing back a pair a test chose.
fn a_blender_stub_of(dir: &Path, pair: &crate::clips::CrossRig) -> std::path::PathBuf {
    std::fs::write(dir.join("fitted.glb"), pair.output_glb()).unwrap();
    std::fs::write(dir.join("source.json"), pair.source_motion()).unwrap();
    a_stub(
        dir,
        "blender-stub.sh",
        &format!(
            r#"printf '%s\n' "$@" >> "$MARROWFALL_STUB_ARGV"
out=""; motion=""
while [ $# -gt 0 ]; do
  if [ "$1" = "--out" ]; then out="$2"; fi
  if [ "$1" = "--source-motion" ]; then motion="$2"; fi
  shift
done
if [ -n "$out" ]; then
  mkdir -p "$(dirname "$out")" "$(dirname "$motion")"
  cp "{clip}" "$out"
  cp "{source}" "$motion"
fi
{report}
: > "$MARROWFALL_SENTINEL"
exit 0
"#,
            clip = dir.join("fitted.glb").display(),
            source = dir.join("source.json").display(),
            report = writes_a_report(dir),
        ),
    )
}

/// The canonical role to bone map, out of the committed skeleton file.
fn a_convention() -> std::collections::BTreeMap<String, String> {
    let table =
        xtask_art::check::aim::AimTable::of(&crate::support::repo_root(), HUMANOID).unwrap();
    table.bones(table.canonical()).unwrap().clone()
}

/// What the retarget reports, as JSON: one clean finding per rule it owns,
/// with `clip.interpolation` reading `measured`.
///
/// Built through the rule registry, so the stub cannot report a limit, a unit
/// or a space the published list does not carry, and never decides its own
/// severity.
fn a_retarget_report(measured: f64) -> String {
    as_findings(&reporting(clip::RETARGET_RULES.iter().copied(), |rule| {
        match rule.id {
            // A clean reading inside every published limit. The one ratio of
            // the seven sits near 1, and everything else reads none of what
            // it measures.
            id if id == clip::INTERPOLATION.id => measured,
            // The one ratio, and the one foot that has to plant at least once.
            id if id == clip::STRIDE_RATIO.id || id == foot::PLANTS.id => 1.0,
            _ => 0.0,
        }
    }))
}

/// The same report with one rule left out of it.
fn without(rule: &str) -> String {
    let mut reported = reporting(clip::RETARGET_RULES.iter().copied(), |_| 0.0);
    reported.remove(rule);
    as_findings(&reported)
}

/// What the source check reports: one clean finding per rule it owns, keyed
/// by rule id so a test can swap one for the defect it is about.
fn a_source_report() -> BTreeMap<&'static str, Finding> {
    let travel = a_profile().source.travel_meters;
    reporting(source::RULES.iter().copied(), |rule| {
        // `source.traveling` is the one `ge` rule of the six: a clip that
        // travels reads at least the threshold, and every other rule reads
        // none of what it is measuring.
        if rule.id == source::TRAVELING.id {
            travel
        } else {
            0.0
        }
    })
}

/// One finding per rule, each reading whatever `reads` says, so the registry
/// and not the stub decides the severity.
fn reporting(
    rules: impl Iterator<Item = &'static Rule>,
    reads: impl Fn(&Rule) -> f64,
) -> BTreeMap<&'static str, Finding> {
    let profile = a_profile();
    rules
        .map(|rule| {
            (
                rule.id,
                rule.measured(&profile, "Hips", reads(rule), 1, "stub".to_owned()),
            )
        })
        .collect()
}

/// A findings list as the body of a JSON array, which is how the stub pastes
/// it into a report.
fn as_findings(reported: &BTreeMap<&str, Finding>) -> String {
    reported
        .values()
        .map(|finding| serde_json::to_string(finding).unwrap())
        .collect::<Vec<String>>()
        .join(",")
}

fn a_profile() -> Profile {
    Profile::of(&crate::support::repo_root(), HUMANOID).unwrap()
}

/// Serves one product, its export, and the file that export produces.
async fn serve_mixamo(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path(format!("/products/{PRODUCT}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": "Walking Backward",
            "details": {"gms_hash": {"model-id": 123_530_901}},
        })))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/animations/export"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status": "ok"})))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/characters/{CHARACTER_ID}/monitor")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "completed",
            "job_result": format!("{}/files/motion.fbx", server.uri()),
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/files/motion.fbx"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(an_fbx()))
        .mount(server)
        .await;
}

#[tokio::test]
async fn a_run_with_nothing_to_fetch_never_asks_for_a_credential() {
    // No token, no base URL: if this touched the network or the browser it
    // could not pass.
    let library = AnimationLibrary::template();
    let dir = a_repo(&library);
    let mut env = EnvGuard::new();
    env.remove("MARROWFALL_MIXAMO_TOKEN");

    fetch(dir.path(), &[], false).await.unwrap();
}

#[tokio::test]
async fn a_fetched_clip_is_staged_fitted_and_recorded() {
    let server = MockServer::start().await;
    serve_mixamo(&server).await;
    let library = a_library();
    let dir = a_repo(&library);
    let stub = a_blender_stub(dir.path());
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_MIXAMO_TOKEN", "a-bearer-token")
        .set("MARROWFALL_MIXAMO_BASE_URL", &server.uri())
        .set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap())
        .set(
            "MARROWFALL_STUB_ARGV",
            dir.path().join("argv.txt").to_str().unwrap(),
        )
        .set("MARROWFALL_STUB_FINDINGS", &a_retarget_report(0.0));

    fetch(dir.path(), &["walk_back".to_owned()], false)
        .await
        .unwrap();

    let staged = AnimationLibrary::staged_download(dir.path(), "walk_back");
    assert_eq!(std::fs::read(staged).unwrap(), an_fbx(), "kept for a look");
    let glb = library.glb(dir.path(), "walk_back");
    assert!(
        glb.ends_with("art/animations/local/walk_back.glb"),
        "a clip we may not redistribute stays out of the committed art: {glb:?}"
    );
    assert!(glb.exists(), "the retarget's output");

    let argv = std::fs::read_to_string(dir.path().join("argv.txt")).unwrap();
    // The source check runs first, on the file as it arrived.
    assert!(argv.contains("check_source.py"), "got: {argv}");
    assert!(
        argv.find("check_source.py") < argv.find("retarget_animation.py"),
        "measured before anything was fitted to it: {argv}"
    );
    // Both scripts are told: the source check gates the vendor file's own
    // travel and `clip.stride` reads the fit against it.
    assert_eq!(argv.matches("--travels\ntrue").count(), 2, "got: {argv}");
    assert!(
        argv.contains("--children\nhips=spine_lower,"),
        "the mapped child per role, off `[profile.tails]`: {argv}"
    );
    assert!(argv.contains("--child-axis\n0,1,0"), "got: {argv}");
    // Every published limit is profile data Rust reads once, so no script
    // opens a skeleton file to find one.
    for limit in [
        "source.traveling=0.02",
        "source.child_axis=180",
        "clip.fps_grid=0.0001",
        "clip.floor_snap=0.005",
        "clip.stride=2",
        "clip.stride_ratio=100",
        "clip.foot_contact.plants=1",
        "clip.foot_contact.skate=0.025",
        "clip.foot_contact.penetration=0.005",
    ] {
        assert!(argv.contains(limit), "{limit} is not published: {argv}");
    }
    assert!(argv.contains("retarget_animation.py"), "got: {argv}");
    assert!(
        argv.contains("--source-fps\n24"),
        "the library's own rate: {argv}"
    );
    assert!(argv.contains("humanoid.glb"), "fitted to the rig: {argv}");
    assert!(
        argv.contains("--convention\nmixamo"),
        "the provider says how its bones are named: {argv}"
    );
    assert!(argv.contains("--name\nwalk_back"), "got: {argv}");

    let lock = LibraryLock::load(dir.path()).unwrap();
    let fetched = &lock.fetched["walk_back"];
    assert_eq!(
        fetched.source,
        MotionSource::Mixamo {
            product_id: PRODUCT.to_owned()
        }
    );
    assert_ne!(fetched.download, fetched.glb);
    // The vendor FBX is not committed, so this record is the only thing that
    // says the clip on disk was ever measured.
    let verdict = fetched.verdict.clone().expect("a recorded verdict");
    assert_eq!(verdict.report, "retarget.walk_back.1");
    assert_eq!(verdict.worst, Severity::Info);
    let mut owed: Vec<String> = clip::RETARGET_RULES
        .iter()
        .chain(clip::FILE_RULES.iter())
        .map(|rule| rule.id.to_owned())
        .collect();
    owed.sort_unstable();
    owed.dedup();
    assert_eq!(
        verdict.rules, owed,
        "every rule the retarget owns, both the ones Blender counts and the \
         ones Rust reads off the file it wrote"
    );
    assert_eq!(
        fetched.fingerprint,
        xtask_art::lock::blender_inputs(dir.path(), HUMANOID, crate::support::BLENDER).unwrap(),
        "the rig and the tooling that fitted it"
    );

    // And it is a small, readable record rather than a copy of the report.
    let text = std::fs::read_to_string(LibraryLock::path(dir.path())).unwrap();
    assert!(text.contains("worst: info"), "got: {text}");
    assert!(
        text.contains("report: \"retarget.walk_back.1\""),
        "got: {text}"
    );
}

/// The clip audition, on record after an unattended run. The vendor FBX is
/// not committed and nothing downstream still carries its posture, so this
/// report is the only thing that says a hunched purchase was measured before
/// the pipeline spent anything on it. Nothing prints and vanishes.
#[tokio::test]
async fn the_audition_survives_an_unattended_fetch_as_a_report_on_disk() {
    let server = MockServer::start().await;
    serve_mixamo(&server).await;
    let dir = a_repo(&a_library());
    let stub = a_blender_stub(dir.path());
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_MIXAMO_TOKEN", "a-bearer-token")
        .set("MARROWFALL_MIXAMO_BASE_URL", &server.uri())
        .set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap())
        .set(
            "MARROWFALL_STUB_ARGV",
            dir.path().join("argv.txt").to_str().unwrap(),
        );

    fetch(dir.path(), &["walk_back".to_owned()], false)
        .await
        .unwrap();

    let written = xtask_art::check::Artifacts::new(dir.path(), source::STAGE, "walk_back", 1)
        .unwrap()
        .report();
    assert!(
        written.ends_with("art/staging/reports/fetch.walk_back.1.json"),
        "got: {}",
        written.display()
    );
    let report = Report::read(&written).unwrap();
    assert_eq!((report.stage(), report.item()), ("fetch", "walk_back"));
    let read: Vec<&str> = report
        .findings()
        .iter()
        .map(|finding| finding.rule.as_str())
        .collect();
    for rule in source::RULES {
        assert!(
            read.contains(&rule.id),
            "{} is not on record: {read:?}",
            rule.id
        );
    }
    assert!(
        read.contains(&source::POSTURE.id),
        "the posture the audition is about: {read:?}"
    );
}

#[tokio::test]
async fn a_second_run_skips_what_the_first_one_fetched() {
    let server = MockServer::start().await;
    serve_mixamo(&server).await;
    let dir = a_repo(&a_library());
    let stub = a_blender_stub(dir.path());
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_MIXAMO_TOKEN", "a-bearer-token")
        .set("MARROWFALL_MIXAMO_BASE_URL", &server.uri())
        .set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap())
        .set(
            "MARROWFALL_STUB_ARGV",
            dir.path().join("argv.txt").to_str().unwrap(),
        );
    fetch(dir.path(), &[], false).await.unwrap();

    // Nothing left to do, so this one needs neither the server nor Blender.
    std::fs::remove_file(dir.path().join("argv.txt")).unwrap();
    fetch(dir.path(), &[], false).await.unwrap();

    assert!(
        !dir.path().join("argv.txt").exists(),
        "a cached clip must not be fitted again"
    );
}

#[tokio::test]
async fn a_file_nobody_recorded_is_reported_and_left_where_it_is() {
    let library = a_library();
    let dir = a_repo(&library);
    let glb = library.glb(dir.path(), "walk_back");
    std::fs::create_dir_all(glb.parent().unwrap()).unwrap();
    std::fs::write(&glb, b"put here by hand").unwrap();
    let mut env = EnvGuard::new();
    env.remove("MARROWFALL_MIXAMO_TOKEN");

    // No token and no server: reaching either would fail this.
    fetch(dir.path(), &[], false).await.unwrap();

    assert_eq!(std::fs::read(&glb).unwrap(), b"put here by hand");
}

#[tokio::test]
async fn a_retarget_that_writes_nothing_is_not_recorded_as_fetched() {
    let server = MockServer::start().await;
    serve_mixamo(&server).await;
    let dir = a_repo(&a_library());
    // Finishes cleanly, sentinel and all, and measures nothing at all.
    let stub = a_stub(
        dir.path(),
        "silent-stub.sh",
        &format!(
            "{passes}: > \"$MARROWFALL_SENTINEL\"\nexit 0\n",
            passes = passes_the_source_check(dir.path())
        ),
    );
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_MIXAMO_TOKEN", "a-bearer-token")
        .set("MARROWFALL_MIXAMO_BASE_URL", &server.uri())
        .set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap());

    let error = fetch(dir.path(), &["walk_back".to_owned()], false)
        .await
        .unwrap_err()
        .to_string();

    assert!(error.contains("wrote no report"), "got: {error}");
    assert!(
        LibraryLock::load(dir.path()).unwrap().fetched.is_empty(),
        "nothing arrived, so nothing is recorded"
    );
}

/// The report is written and the clip is not, which is the shape a failed
/// export leaves behind.
#[tokio::test]
async fn a_retarget_that_reports_but_writes_no_clip_is_refused() {
    let server = MockServer::start().await;
    serve_mixamo(&server).await;
    let dir = a_repo(&a_library());
    let stub = a_stub(
        dir.path(),
        "no-clip-stub.sh",
        &format!(
            "{passes}{report}\n: > \"$MARROWFALL_SENTINEL\"\nexit 0\n",
            passes = passes_the_source_check(dir.path()),
            report = writes_a_report(dir.path())
        ),
    );
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_MIXAMO_TOKEN", "a-bearer-token")
        .set("MARROWFALL_MIXAMO_BASE_URL", &server.uri())
        .set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap())
        .set("MARROWFALL_STUB_FINDINGS", &a_retarget_report(0.0));

    let error = format!(
        "{:#}",
        fetch(dir.path(), &["walk_back".to_owned()], false)
            .await
            .unwrap_err()
    );

    // The clip gates are what report it: a missing input is an error with a
    // stated reason, never a skip.
    assert!(error.contains("left 11 defect(s)"), "got: {error}");
    let report = std::fs::read_to_string(
        dir.path()
            .join("art/staging/reports/retarget.walk_back.1.json"),
    )
    .unwrap();
    assert!(report.contains("walk_back.glb"), "got: {report}");
    assert!(LibraryLock::load(dir.path()).unwrap().fetched.is_empty());
}

/// A clip that broke a `clip.*` rule is not recorded, however complete the
/// file looks. The report stays on disk for a human.
#[tokio::test]
async fn a_retarget_that_breaks_a_rule_is_not_recorded_as_fetched() {
    let server = MockServer::start().await;
    serve_mixamo(&server).await;
    let dir = a_repo(&a_library());
    let stub = a_blender_stub(dir.path());
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_MIXAMO_TOKEN", "a-bearer-token")
        .set("MARROWFALL_MIXAMO_BASE_URL", &server.uri())
        .set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap())
        .set(
            "MARROWFALL_STUB_ARGV",
            dir.path().join("argv.txt").to_str().unwrap(),
        )
        // Four channels left on Bezier. The registry decides the severity,
        // so this is an error without the stub claiming to be one.
        .set("MARROWFALL_STUB_FINDINGS", &a_retarget_report(4.0));

    let error = fetch(dir.path(), &["walk_back".to_owned()], false)
        .await
        .unwrap_err()
        .to_string();

    assert!(error.contains("left 1 defect(s)"), "got: {error}");
    assert!(
        dir.path()
            .join("art/staging/reports/retarget.walk_back.1.json")
            .exists(),
        "the report is the diagnostic, so it stays"
    );
    assert!(LibraryLock::load(dir.path()).unwrap().fetched.is_empty());
}

/// `[synth]` a fit whose feet ride along with its root, which breaks two
/// rules on two feet each. The refusal names each rule once, sorted, so a
/// reader knows what to open the report for.
#[tokio::test]
async fn the_refusal_names_each_failing_rule_once() {
    let server = MockServer::start().await;
    serve_mixamo(&server).await;
    let dir = a_repo(&a_library());
    let stub = a_blender_stub_of(dir.path(), &crate::clips::CrossRig::new(a_convention()));
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_MIXAMO_TOKEN", "a-bearer-token")
        .set("MARROWFALL_MIXAMO_BASE_URL", &server.uri())
        .set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap())
        .set(
            "MARROWFALL_STUB_ARGV",
            dir.path().join("argv.txt").to_str().unwrap(),
        )
        .set("MARROWFALL_STUB_FINDINGS", &a_retarget_report(0.0));

    let error = fetch(dir.path(), &["walk_back".to_owned()], false)
        .await
        .unwrap_err()
        .to_string();

    assert!(
        error.contains(
            "left 4 defect(s) on clip.foot_contact.plants, clip.foot_contact.skate, listed in"
        ),
        "got: {error}"
    );
}

/// `[synth]` the same one defect on a clip the library declares in place,
/// where both stance rules file themselves as `skipped`. A skip is a rule
/// that chose not to measure, so it is not a defect, and counting one would
/// make the refusal name a number no reader can find in the report.
#[tokio::test]
async fn a_skipped_rule_is_not_counted_among_the_defects() {
    let server = MockServer::start().await;
    serve_mixamo(&server).await;
    let mut library = a_library();
    library.animations.get_mut("walk_back").unwrap().travels = false;
    let dir = a_repo(&library);
    let stub = a_blender_stub(dir.path());
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_MIXAMO_TOKEN", "a-bearer-token")
        .set("MARROWFALL_MIXAMO_BASE_URL", &server.uri())
        .set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap())
        .set(
            "MARROWFALL_STUB_ARGV",
            dir.path().join("argv.txt").to_str().unwrap(),
        )
        .set("MARROWFALL_STUB_FINDINGS", &a_retarget_report(4.0));

    let error = fetch(dir.path(), &["walk_back".to_owned()], false)
        .await
        .unwrap_err()
        .to_string();

    let report = Report::read(
        &dir.path()
            .join("art/staging/reports/retarget.walk_back.1.json"),
    )
    .unwrap();
    assert_eq!(
        report
            .findings()
            .iter()
            .filter(|finding| finding.severity == Severity::Skipped && !finding.holds())
            .count(),
        2,
        "one skipped plant per foot, each reading 0 against a limit of 1"
    );
    assert!(
        error.contains("left 1 defect(s) on clip.interpolation, listed in"),
        "got: {error}"
    );
}

/// A retarget that reported nothing about one of its own rules. This is what
/// deleting the retarget's own `placed` call looks like from here: a gate
/// that goes quiet cannot be told from one that never ran.
#[tokio::test]
async fn a_retarget_that_leaves_one_of_its_rules_unread_is_refused() {
    let server = MockServer::start().await;
    serve_mixamo(&server).await;
    let dir = a_repo(&a_library());
    let stub = a_blender_stub(dir.path());
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_MIXAMO_TOKEN", "a-bearer-token")
        .set("MARROWFALL_MIXAMO_BASE_URL", &server.uri())
        .set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap())
        .set(
            "MARROWFALL_STUB_ARGV",
            dir.path().join("argv.txt").to_str().unwrap(),
        )
        .set("MARROWFALL_STUB_FINDINGS", &without("clip.floor_snap"));

    let error = fetch(dir.path(), &["walk_back".to_owned()], false)
        .await
        .unwrap_err()
        .to_string();

    assert!(error.contains("clip.floor_snap"), "got: {error}");
    assert!(error.contains("never read"), "got: {error}");
    assert!(LibraryLock::load(dir.path()).unwrap().fetched.is_empty());
}

/// And the same for the plant, which is the newest of the retarget's rules
/// and the one whose measurement is furthest from where the report is built.
#[tokio::test]
async fn a_retarget_that_never_reads_a_foot_is_refused() {
    let server = MockServer::start().await;
    serve_mixamo(&server).await;
    let dir = a_repo(&a_library());
    let stub = a_blender_stub(dir.path());
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_MIXAMO_TOKEN", "a-bearer-token")
        .set("MARROWFALL_MIXAMO_BASE_URL", &server.uri())
        .set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap())
        .set(
            "MARROWFALL_STUB_ARGV",
            dir.path().join("argv.txt").to_str().unwrap(),
        )
        .set("MARROWFALL_STUB_FINDINGS", &without(foot::PLANTS.id));

    let error = fetch(dir.path(), &["walk_back".to_owned()], false)
        .await
        .unwrap_err()
        .to_string();

    assert!(error.contains(foot::PLANTS.id), "got: {error}");
    assert!(error.contains("never read"), "got: {error}");
    assert!(LibraryLock::load(dir.path()).unwrap().fetched.is_empty());
}

/// A defect filed as information. The runner's own gate reads severity, so
/// without the registry check this clip would be recorded as fetched with
/// four broken channels sitting in its report.
#[tokio::test]
async fn a_retarget_that_calls_a_defect_information_is_refused() {
    let server = MockServer::start().await;
    serve_mixamo(&server).await;
    let dir = a_repo(&a_library());
    let stub = a_blender_stub(dir.path());
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_MIXAMO_TOKEN", "a-bearer-token")
        .set("MARROWFALL_MIXAMO_BASE_URL", &server.uri())
        .set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap())
        .set(
            "MARROWFALL_STUB_ARGV",
            dir.path().join("argv.txt").to_str().unwrap(),
        )
        .set(
            "MARROWFALL_STUB_FINDINGS",
            &a_retarget_report(4.0).replace("\"error\"", "\"info\""),
        );

    let error = fetch(dir.path(), &["walk_back".to_owned()], false)
        .await
        .unwrap_err()
        .to_string();

    assert!(error.contains("severity"), "got: {error}");
    assert!(LibraryLock::load(dir.path()).unwrap().fetched.is_empty());
}

/// A clip that is not what the library declares stops before the retarget,
/// because the whole point of measuring the vendor file first is not to
/// spend anything fitting the wrong one.
#[tokio::test]
async fn a_source_check_that_finds_a_defect_stops_before_the_retarget() {
    let server = MockServer::start().await;
    serve_mixamo(&server).await;
    let dir = a_repo(&a_library());
    let stub = a_blender_stub(dir.path());
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_MIXAMO_TOKEN", "a-bearer-token")
        .set("MARROWFALL_MIXAMO_BASE_URL", &server.uri())
        .set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap())
        .set(
            "MARROWFALL_STUB_ARGV",
            dir.path().join("argv.txt").to_str().unwrap(),
        )
        // An in-place export of a clip the library calls traveling. The
        // other five rules still report, so this is the reading and not the
        // shape of the report.
        .set(
            "MARROWFALL_STUB_SOURCE_FINDINGS",
            &a_source_report_where(&source::TRAVELING, 0.0),
        );

    let error = fetch(dir.path(), &["walk_back".to_owned()], false)
        .await
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("not the clip the library declares"),
        "got: {error}"
    );
    let argv = std::fs::read_to_string(dir.path().join("argv.txt")).unwrap();
    assert!(
        !argv.contains("retarget_animation.py"),
        "nothing was fitted to it: {argv}"
    );
    assert!(LibraryLock::load(dir.path()).unwrap().fetched.is_empty());
}

/// And the same registry check the retarget gets, on the fetch report.
#[tokio::test]
async fn a_source_check_reporting_a_rule_nobody_publishes_is_refused() {
    let server = MockServer::start().await;
    serve_mixamo(&server).await;
    let dir = a_repo(&a_library());
    let stub = a_blender_stub(dir.path());
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_MIXAMO_TOKEN", "a-bearer-token")
        .set("MARROWFALL_MIXAMO_BASE_URL", &server.uri())
        .set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap())
        .set(
            "MARROWFALL_STUB_ARGV",
            dir.path().join("argv.txt").to_str().unwrap(),
        )
        .set(
            "MARROWFALL_STUB_SOURCE_FINDINGS",
            &a_source_report_where(&source::TRAVELING, 2.31)
                .replace("source.traveling", "source.made_up"),
        );

    let error = fetch(dir.path(), &["walk_back".to_owned()], false)
        .await
        .unwrap_err()
        .to_string();

    assert!(error.contains("source.made_up"), "got: {error}");
}

/// A source check that reported nothing about one of its own six rules.
/// This is what deleting one of `check_source.py`'s own measurements looks
/// like from here, and the guard is the same one the retarget gets.
#[tokio::test]
async fn a_source_check_that_leaves_one_of_its_rules_unread_is_refused() {
    let server = MockServer::start().await;
    serve_mixamo(&server).await;
    let dir = a_repo(&a_library());
    let stub = a_blender_stub(dir.path());
    let mut reported = a_source_report();
    reported.remove(source::WANDER.id);
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_MIXAMO_TOKEN", "a-bearer-token")
        .set("MARROWFALL_MIXAMO_BASE_URL", &server.uri())
        .set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap())
        .set(
            "MARROWFALL_STUB_ARGV",
            dir.path().join("argv.txt").to_str().unwrap(),
        )
        .set("MARROWFALL_STUB_SOURCE_FINDINGS", &as_findings(&reported));

    let error = fetch(dir.path(), &["walk_back".to_owned()], false)
        .await
        .unwrap_err()
        .to_string();

    assert!(error.contains("source.wander"), "got: {error}");
    assert!(error.contains("never read"), "got: {error}");
    let argv = std::fs::read_to_string(dir.path().join("argv.txt")).unwrap();
    assert!(
        !argv.contains("retarget_animation.py"),
        "nothing was fitted to it: {argv}"
    );
    assert!(LibraryLock::load(dir.path()).unwrap().fetched.is_empty());
}

/// A vendor file with two coincident joints has no angle to report, and the
/// undefined record that says so carries neither the rule's unit nor its
/// limit. The fetch has to surface that defect, not abort on the shape of it.
#[tokio::test]
async fn a_source_check_that_could_not_take_a_measurement_reports_the_defect() {
    let server = MockServer::start().await;
    serve_mixamo(&server).await;
    let dir = a_repo(&a_library());
    let stub = a_blender_stub(dir.path());
    let undefined = xtask_art::check::source::CHILD_AXIS.undefined(
        "neck",
        1,
        "neck has no direction to measure, so no angle exists".to_owned(),
    );
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_MIXAMO_TOKEN", "a-bearer-token")
        .set("MARROWFALL_MIXAMO_BASE_URL", &server.uri())
        .set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap())
        .set(
            "MARROWFALL_STUB_ARGV",
            dir.path().join("argv.txt").to_str().unwrap(),
        )
        .set("MARROWFALL_STUB_SOURCE_FINDINGS", &{
            let mut reported = a_source_report();
            reported.insert(source::CHILD_AXIS.id, undefined);
            as_findings(&reported)
        });

    let error = fetch(dir.path(), &["walk_back".to_owned()], false)
        .await
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("not the clip the library declares"),
        "got: {error}"
    );
}

/// The clean source report with one rule reading something else, which is
/// the shape a real defect arrives in.
fn a_source_report_where(rule: &Rule, measured: f64) -> String {
    let mut reported = a_source_report();
    reported.insert(
        rule.id,
        rule.measured(
            &a_profile(),
            "the whole clip",
            measured,
            1,
            "stub".to_owned(),
        ),
    );
    as_findings(&reported)
}

/// A rule `--list-rules` does not print has no published limit, so a report
/// naming one is refused rather than filed.
#[tokio::test]
async fn a_retarget_reporting_a_rule_nobody_publishes_is_refused() {
    let server = MockServer::start().await;
    serve_mixamo(&server).await;
    let dir = a_repo(&a_library());
    let stub = a_blender_stub(dir.path());
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_MIXAMO_TOKEN", "a-bearer-token")
        .set("MARROWFALL_MIXAMO_BASE_URL", &server.uri())
        .set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap())
        .set(
            "MARROWFALL_STUB_ARGV",
            dir.path().join("argv.txt").to_str().unwrap(),
        )
        .set(
            "MARROWFALL_STUB_FINDINGS",
            &a_retarget_report(0.0).replace("clip.interpolation", "clip.made_up"),
        );

    let error = fetch(dir.path(), &["walk_back".to_owned()], false)
        .await
        .unwrap_err()
        .to_string();

    assert!(error.contains("clip.made_up"), "got: {error}");
    assert!(LibraryLock::load(dir.path()).unwrap().fetched.is_empty());
}
