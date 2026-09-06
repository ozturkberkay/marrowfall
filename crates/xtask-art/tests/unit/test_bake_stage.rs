//! The bake stage, with a stub standing in for Blender.
//!
//! The real bake is a Blender render, which no test should depend on. What is
//! worth testing is everything around it: the environment handed over, the
//! arguments assembled, the staging directory cleared, and the failure
//! reported. A shell stub that writes the frames Blender would have written
//! covers all of that.

use std::path::Path;

use xtask_art::check::Artifacts;
use xtask_art::library::{Animation, MotionSource};
use xtask_art::spec::Paths;
use xtask_art::stages;

use crate::support::{
    EnvGuard, a_bake_finding, a_bake_report, a_bake_report_of, a_library, a_spec, bake_subjects,
    install_library, install_skeleton,
};

/// A repo tree with the script, a virtualenv and the animation library in place.
fn a_baked_repo(with_animations: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("tools/blender/src")).unwrap();
    std::fs::write(root.join("tools/blender/src/bake_sprites.py"), "").unwrap();
    std::fs::create_dir_all(root.join(".venv/lib/python3.13/site-packages")).unwrap();
    install_skeleton(root);

    let paths = Paths::new(root, "survivor");
    std::fs::create_dir_all(paths.dir()).unwrap();
    std::fs::write(paths.character_glb(), b"glTF").unwrap();
    if with_animations {
        install_library(root);
    }
    dir
}

/// A stub that parses `--out`, writes one PNG there, and finishes the way a
/// real script does: with the success sentinel and a report on every clip.
fn a_blender_stub(dir: &Path) -> std::path::PathBuf {
    a_stub_reporting(dir, &a_bake_report("survivor", &["idle", "run"]))
}

/// The same stub, over a report the caller chose.
fn a_stub_reporting(dir: &Path, report: &str) -> std::path::PathBuf {
    let prepared = dir.join("prepared.json");
    std::fs::write(&prepared, report).unwrap();
    let stub = dir.join("blender-stub.sh");
    std::fs::write(
        &stub,
        format!(
            r#"#!/bin/sh
out=""
while [ $# -gt 0 ]; do
  if [ "$1" = "--out" ]; then out="$2"; fi
  shift
done
mkdir -p "$out"
: > "$out/idle_s_00.png"
: > "$out/idle_s_01.png"
: > "$out/notes.txt"
cat {prepared:?} > "$MARROWFALL_REPORT"
: > "$MARROWFALL_SENTINEL"
exit 0
"#,
            prepared = prepared.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(
        &stub,
        <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
    )
    .unwrap();
    stub
}

#[test]
fn bake_counts_only_the_png_frames_it_produced() {
    let library = a_library();
    let dir = a_baked_repo(true);
    let stub = a_blender_stub(dir.path());
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap());

    let paths = Paths::new(dir.path(), "survivor");
    let record = stages::bake(&a_spec("survivor"), &library, &paths, dir.path()).unwrap();

    assert_eq!(
        record.note.unwrap(),
        "2 frames",
        "the stray .txt must not be counted as a frame"
    );
}

#[test]
fn bake_passes_the_character_once_and_every_animation_by_name() {
    let library = a_library();
    let dir = a_baked_repo(true);
    let stub = a_blender_stub(dir.path());
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap());

    let mut spec = a_spec("survivor");
    spec.animations.push("run".to_owned());
    stages::bake(
        &spec,
        &library,
        &Paths::new(dir.path(), "survivor"),
        dir.path(),
    )
    .unwrap();

    let seen = std::fs::read_to_string(
        Artifacts::new(dir.path(), "bake", "survivor", 1)
            .unwrap()
            .argv(),
    )
    .unwrap();
    assert_eq!(
        seen.matches("--character").count(),
        1,
        "the mesh is loaded once and shared across animations"
    );
    assert!(seen.contains("idle="), "labeled by our name: {seen}");
    assert!(seen.contains("run="), "labeled by our name: {seen}");
    assert!(
        seen.contains("art/animations/idle.glb"),
        "from the shared library: {seen}"
    );
    assert!(
        seen.contains("art/animations/run.glb"),
        "from the shared library: {seen}"
    );
    assert!(seen.contains("--directions\n8"), "got: {seen}");
    assert!(
        seen.contains("--fps\nrun=24"),
        "a rate per animation: {seen}"
    );
}

#[test]
fn stale_frames_from_a_previous_shape_are_cleared_first() {
    let library = a_library();
    let dir = a_baked_repo(true);
    let stub = a_blender_stub(dir.path());
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap());

    let paths = Paths::new(dir.path(), "survivor");
    std::fs::create_dir_all(paths.staging()).unwrap();
    std::fs::write(paths.staging().join("idle_s_99.png"), b"stale").unwrap();

    stages::bake(&a_spec("survivor"), &library, &paths, dir.path()).unwrap();

    assert!(
        !paths.staging().join("idle_s_99.png").exists(),
        "a leftover frame would be picked up by packing"
    );
}

/// The mesh sent to rigging lives beside the frames, and the paid rig record
/// fingerprints it, so clearing it would report that stage stale after every
/// bake.
#[test]
fn the_mesh_sent_to_rigging_survives_a_bake() {
    let library = a_library();
    let dir = a_baked_repo(true);
    let stub = a_blender_stub(dir.path());
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap());

    let paths = Paths::new(dir.path(), "survivor");
    std::fs::create_dir_all(paths.staging()).unwrap();
    std::fs::write(paths.bare_glb(), b"glTF bare").unwrap();
    std::fs::write(paths.clean_glb(), b"glTF clean").unwrap();

    stages::bake(&a_spec("survivor"), &library, &paths, dir.path()).unwrap();

    assert_eq!(std::fs::read(paths.bare_glb()).unwrap(), b"glTF bare");
    assert_eq!(std::fs::read(paths.clean_glb()).unwrap(), b"glTF clean");
}

/// A bake that measured nothing is a bake whose two rules nobody read, and a
/// gate that goes quiet cannot be told from one that never ran.
#[test]
fn a_bake_that_writes_no_report_at_all_stops() {
    let error = a_bake_reporting(None);
    assert!(error.contains("wrote no report"), "got: {error}");
}

/// And one that wrote a report but left an axis out of it. This is what
/// deleting the bake's own `measure_root_travel` call looks like from here.
#[test]
fn a_bake_that_leaves_a_subject_unreported_stops() {
    let error = a_bake_reporting(Some(without("idle z")));

    assert!(error.contains("idle z"), "got: {error}");
    assert!(error.contains("nothing was read"), "got: {error}");
}

/// The runner publishes every limit the script reports against, so a finding
/// naming a rule the list does not carry has no published limit at all.
#[test]
fn a_bake_finding_off_the_rule_list_is_named_back() {
    let error = a_bake_reporting(Some(edited("clip.root_travel", "clip.made_up")));
    assert!(error.contains("clip.made_up"), "got: {error}");
}

/// And a real defect stops the stage rather than being counted and dropped.
///
/// Both numbers are what pinning the root's own channels 0 and 1 leaves on a
/// left strafe, which is the defect these two rules exist to catch.
#[test]
fn a_root_that_still_slides_sideways_stops_the_bake() {
    let error = a_bake_reporting(Some(measuring("idle x", 0.0428)));
    assert!(error.contains("1 defect(s)"), "got: {error}");
}

/// And a root that sank, which is the other half: the up axis has a limit of
/// its own, because the bob it keeps is animation.
#[test]
fn a_root_that_sank_a_third_of_a_meter_stops_the_bake() {
    let error = a_bake_reporting(Some(measuring("idle z", 0.2911)));
    assert!(error.contains("1 defect(s)"), "got: {error}");
}

/// The clean report with one subject taken out of it.
fn without(dropped: &str) -> String {
    let findings = bake_subjects(&["idle"])
        .iter()
        .filter(|subject| *subject != dropped)
        .map(|subject| a_bake_finding(subject, 0.0))
        .collect();
    a_bake_report_of("survivor", findings)
}

/// The clean report with one subject read at `meters`, through the rule, so
/// the severity is the one the number gives.
fn measuring(subject: &str, meters: f64) -> String {
    let findings = bake_subjects(&["idle"])
        .iter()
        .map(|each| a_bake_finding(each, if each == subject { meters } else { 0.0 }))
        .collect();
    a_bake_report_of("survivor", findings)
}

/// The clean report with one substring rewritten.
fn edited(from: &str, to: &str) -> String {
    a_bake_report("survivor", &["idle"]).replace(from, to)
}

/// Runs the bake against a stub that hands `report` back, or writes no report
/// at all, and returns why the stage refused it.
fn a_bake_reporting(report: Option<String>) -> String {
    let library = a_library();
    let dir = a_baked_repo(true);
    let stub = match report {
        Some(report) => a_stub_reporting(dir.path(), &report),
        None => a_silent_stub(dir.path()),
    };
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap());

    stages::bake(
        &a_spec("survivor"),
        &library,
        &Paths::new(dir.path(), "survivor"),
        dir.path(),
    )
    .unwrap_err()
    .to_string()
}

/// A run that finished and wrote nothing.
fn a_silent_stub(dir: &Path) -> std::path::PathBuf {
    let stub = dir.join("silent.sh");
    std::fs::write(&stub, "#!/bin/sh\n: > \"$MARROWFALL_SENTINEL\"\nexit 0\n").unwrap();
    std::fs::set_permissions(
        &stub,
        <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
    )
    .unwrap();
    stub
}

/// A failed bake is a diagnostic, so its half-written frames stay on disk.
#[test]
fn a_failed_bake_keeps_the_frames_it_did_produce() {
    let library = a_library();
    let dir = a_baked_repo(true);
    let stub = dir.path().join("half-way.sh");
    std::fs::write(
        &stub,
        r#"#!/bin/sh
out=""
while [ $# -gt 0 ]; do
  if [ "$1" = "--out" ]; then out="$2"; fi
  shift
done
mkdir -p "$out"
: > "$out/idle_s_00.png"
exit 1
"#,
    )
    .unwrap();
    std::fs::set_permissions(
        &stub,
        <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
    )
    .unwrap();
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap());

    let paths = Paths::new(dir.path(), "survivor");
    assert!(stages::bake(&a_spec("survivor"), &library, &paths, dir.path()).is_err());

    assert!(
        paths.staging().join("idle_s_00.png").exists(),
        "the partial render is evidence, not rubbish"
    );
}

#[test]
fn a_missing_animation_glb_says_to_download_first() {
    let library = a_library();
    // The character is present but its animation was never fetched.
    let dir = a_baked_repo(false);
    let stub = a_blender_stub(dir.path());
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap());

    let error = stages::bake(
        &a_spec("survivor"),
        &library,
        &Paths::new(dir.path(), "survivor"),
        dir.path(),
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("run the download stage first"),
        "got: {error}"
    );
}

/// Motion nobody bought arrives another way, so the message has to say which.
#[test]
fn a_missing_fetched_animation_names_the_command_that_gets_it() {
    let mut library = a_library();
    library.animations.insert(
        "strafe_left".to_owned(),
        Animation {
            skeleton: xtask_art::library::HUMANOID.to_owned(),
            loops: true,
            fps: 24,
            source_fps: 30,
            travels: true,
            source: MotionSource::Mixamo {
                product_id: "c9c97b90-b96c-11e4-a802-0aaa78deedf9".to_owned(),
            },
        },
    );
    let dir = a_baked_repo(true);
    let stub = a_blender_stub(dir.path());
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap());
    let mut spec = a_spec("survivor");
    spec.animations = vec!["strafe_left".to_owned()];

    let error = stages::bake(
        &spec,
        &library,
        &Paths::new(dir.path(), "survivor"),
        dir.path(),
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("cargo art fetch strafe_left"),
        "got: {error}"
    );
}

#[test]
fn a_missing_bake_script_is_reported_by_path() {
    let library = a_library();
    let dir = tempfile::tempdir().unwrap();
    let error = stages::bake(
        &a_spec("survivor"),
        &library,
        &Paths::new(dir.path(), "survivor"),
        dir.path(),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("missing bake script"), "got: {error}");
}
