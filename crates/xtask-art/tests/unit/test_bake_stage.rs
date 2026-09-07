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

use crate::frames;
use crate::support::{
    EnvGuard, a_bake_finding, a_bake_findings, a_bake_report, a_bake_report_of, a_library, a_ring,
    a_spec, install_library, install_skeleton,
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

/// How many frames per direction the stub renders. Two, so a rendered set is
/// a rectangle with something in it and a test still reads in milliseconds.
const STUB_FRAMES: u32 = 2;

/// The clips the stub renders and reports on.
const STUB_CLIPS: [&str; 2] = ["idle", "run"];

/// A stub that parses `--out`, renders the frames a real bake would, and
/// finishes the way a real script does: with the success sentinel and a
/// report on every clip.
fn a_blender_stub(dir: &Path) -> std::path::PathBuf {
    a_stub_reporting(dir, &a_bake_report("survivor", &STUB_CLIPS))
}

/// The same stub, over a report the caller chose.
fn a_stub_reporting(dir: &Path, report: &str) -> std::path::PathBuf {
    a_stub_rendering(dir, report, &frames::a_frame())
}

/// And the same again, where one frame of `idle` is drawn some other way:
/// what a bake that cut a pose off or drew nothing leaves behind.
fn a_stub_rendering(dir: &Path, report: &str, odd: &image::RgbaImage) -> std::path::PathBuf {
    let prepared = dir.join("prepared.json");
    std::fs::write(&prepared, report).unwrap();
    let good = dir.join("frame.png");
    frames::a_frame().save(&good).unwrap();
    let unusual = dir.join("odd.png");
    odd.save(&unusual).unwrap();

    let mut renders = String::new();
    for clip in STUB_CLIPS {
        for direction in a_ring() {
            for index in 0..STUB_FRAMES {
                let source = if (clip, *direction, index) == ("idle", a_ring()[0], 0) {
                    &unusual
                } else {
                    &good
                };
                renders.push_str(&format!(
                    "cp {source:?} \"$out/{clip}_{direction}_{index:02}.png\"\n",
                    source = source.display()
                ));
            }
        }
    }

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
{renders}: > "$out/notes.txt"
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
        "32 frames",
        "two clips of eight directions by two frames, and the stray .txt is \
         not one of them"
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
    let findings = a_bake_findings(&["idle"])
        .into_iter()
        .filter(|finding| finding.subject != dropped)
        .collect();
    a_bake_report_of("survivor", findings)
}

/// And with every finding of one rule taken out, which is what deleting the
/// call that measures it looks like from here.
fn unread(rule: &str) -> String {
    let findings = a_bake_findings(&["idle"])
        .into_iter()
        .filter(|finding| finding.rule != rule)
        .collect();
    a_bake_report_of("survivor", findings)
}

/// The clean report with one subject read at `meters`, through the rule, so
/// the severity is the one the number gives.
fn measuring(subject: &str, meters: f64) -> String {
    let findings = a_bake_findings(&["idle"])
        .into_iter()
        .map(|finding| {
            if finding.subject == subject {
                a_bake_finding(subject, meters)
            } else {
                finding
            }
        })
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

/// What the script does with a defect it can see before it renders: writes
/// the whole report and returns, so a moved golden costs seconds. From here
/// that is a report with an error and an empty staging directory, and both
/// have to reach the person who ran it.
#[test]
fn a_bake_that_stopped_before_rendering_names_the_defect_and_the_frames_it_owes() {
    let library = a_library();
    let dir = a_baked_repo(true);
    let stub = a_reporting_stub_that_never_renders(dir.path(), &measuring("idle x", 0.0428));
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap());
    let paths = Paths::new(dir.path(), "survivor");

    let error = stages::bake(&a_spec("survivor"), &library, &paths, dir.path())
        .unwrap_err()
        .to_string();

    assert!(error.contains("defect(s)"), "got: {error}");
    assert_eq!(
        std::fs::read_dir(paths.staging())
            .map(|entries| entries.count())
            .unwrap_or(0),
        0,
        "the stub rendered nothing, so nothing may be left behind"
    );
}

/// A run that wrote its report and returned before rendering a frame.
fn a_reporting_stub_that_never_renders(dir: &Path, report: &str) -> std::path::PathBuf {
    let prepared = dir.join("early.json");
    std::fs::write(&prepared, report).unwrap();
    let stub = dir.join("early.sh");
    std::fs::write(
        &stub,
        format!(
            "#!/bin/sh\ncat {prepared:?} > \"$MARROWFALL_REPORT\"\n\
             : > \"$MARROWFALL_SENTINEL\"\nexit 0\n",
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

/// And one that dropped every finding of one rule, which is what deleting
/// the call that measures it looks like from here.
///
/// `bake.sampled_frames_are_keys` names its clip, which the four rules this
/// stage reads off the PNGs name too, so what is missing here is the rule and
/// never the subject.
#[test]
fn a_bake_that_never_read_a_rule_stops() {
    let error = a_bake_reporting(Some(unread("bake.sampled_frames_are_keys")));

    assert!(
        error.contains("bake.sampled_frames_are_keys"),
        "got: {error}"
    );
    assert!(error.contains("never read"), "got: {error}");
}

/// The goldens the script reads, the two directions it takes them in, and
/// the limits it reports them against all arrive on argv: a script names no
/// path of its own and no limit of its own.
#[test]
fn the_goldens_and_their_two_directions_reach_blender() {
    let seen = an_argv_from(None);

    assert!(
        seen.contains("--goldens\n") && seen.contains("art/goldens/survivor"),
        "got: {seen}"
    );
    assert!(seen.contains("--golden-direction\ns\n"), "got: {seen}");
    assert!(seen.contains("--golden-direction\ne\n"), "got: {seen}");
    assert!(
        seen.contains("--limit\nbake.landmark_golden=1\n"),
        "got: {seen}"
    );
    assert!(
        seen.contains("--limit\nbake.sampled_frames_are_keys=0\n"),
        "got: {seen}"
    );
    assert!(
        !seen.contains("--update-goldens"),
        "a golden is read, not rewritten, unless something asks: {seen}"
    );
}

/// And the one thing that rewrites a golden is that variable, which CI
/// asserts is unset.
#[test]
fn only_the_update_variable_turns_a_golden_read_into_a_rewrite() {
    assert_eq!(stages::UPDATE_GOLDENS_ENV, "MARROWFALL_UPDATE_GOLDENS");

    let seen = an_argv_from(Some("1"));
    assert!(seen.contains("--update-goldens"), "got: {seen}");

    let empty = an_argv_from(Some(""));
    assert!(
        !empty.contains("--update-goldens"),
        "an empty variable asks for nothing: {empty}"
    );
}

/// And nothing set it in this run, which is the local half of what CI
/// asserts: a suite that measures goldens must not be measuring goldens it
/// just rewrote.
#[test]
fn nothing_set_the_golden_update_switch_in_this_run() {
    // Through the guard, so this cannot read the variable the test above sets.
    let _env = EnvGuard::new();

    assert!(
        std::env::var_os(stages::UPDATE_GOLDENS_ENV).is_none(),
        "{} is set, so every golden this run reads is one it wrote",
        stages::UPDATE_GOLDENS_ENV
    );
}

/// And the CI half: the workflow refuses the variable before it runs a test,
/// and reads the contact sheet it redraws against the committed copy before
/// uploading it.
#[test]
fn ci_refuses_a_rewritten_golden_before_it_runs_anything() {
    let workflow = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../.github/workflows/rust.yml"
    ));

    let refusal = workflow
        .find("run: test -z \"${MARROWFALL_UPDATE_GOLDENS:-}\"")
        .expect("the workflow asserts the variable is unset");
    let tests = workflow
        .find("run: cargo nextest run --workspace")
        .expect("the workflow runs the workspace tests");
    assert!(
        refusal < tests,
        "the refusal has to come before anything reads a golden"
    );
    let drawn = workflow
        .find("run: cargo run --package xtask-art -- check --sheet")
        .expect("CI draws the contact sheet from the committed atlases");
    let compared = workflow
        .find("run: git diff --exit-code -- 'project/assets/characters/*/sheet.png'")
        .expect("and reads the redraw against the copy in the diff");
    assert!(
        drawn < compared,
        "a comparison before the redraw would pass on a stale sheet"
    );
    assert!(
        workflow.contains("uses: actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a"),
        "and uploads it, pinned by commit"
    );
}

/// The bake's own argv, with `MARROWFALL_UPDATE_GOLDENS` set to `value`.
fn an_argv_from(value: Option<&str>) -> String {
    let library = a_library();
    let dir = a_baked_repo(true);
    let stub = a_blender_stub(dir.path());
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap());
    match value {
        Some(value) => env.set(stages::UPDATE_GOLDENS_ENV, value),
        None => env.remove(stages::UPDATE_GOLDENS_ENV),
    };

    stages::bake(
        &a_spec("survivor"),
        &library,
        &Paths::new(dir.path(), "survivor"),
        dir.path(),
    )
    .unwrap();

    std::fs::read_to_string(
        Artifacts::new(dir.path(), "bake", "survivor", 1)
            .unwrap()
            .argv(),
    )
    .unwrap()
}

/// The rules this stage reads off the PNGs are wired into it, so a pose the
/// camera cut off stops the bake even though the script said nothing about
/// it.
#[test]
fn a_pose_the_camera_cut_off_stops_the_bake() {
    let library = a_library();
    let dir = a_baked_repo(true);
    let stub = a_stub_rendering(
        dir.path(),
        &a_bake_report("survivor", &STUB_CLIPS),
        &frames::a_clipped_frame(),
    );
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

    // Both, because content pushed against one border is no longer centered
    // on the axis the ring turns about either.
    assert!(
        error.contains("left 2 defect(s) on bake.in_frame, bake.pivot"),
        "got: {error}"
    );
}

/// And the report on disk carries both halves, so the file a reader opens is
/// the whole measurement rather than the Blender half of it.
#[test]
fn the_written_report_carries_what_blender_measured_and_what_rust_did() {
    let library = a_library();
    let dir = a_baked_repo(true);
    let stub = a_blender_stub(dir.path());
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap());

    stages::bake(
        &a_spec("survivor"),
        &library,
        &Paths::new(dir.path(), "survivor"),
        dir.path(),
    )
    .unwrap();

    let written = xtask_art::check::Report::read(
        &Artifacts::new(dir.path(), "bake", "survivor", 1)
            .unwrap()
            .report(),
    )
    .unwrap();
    let rules: std::collections::BTreeSet<&str> = written
        .findings()
        .iter()
        .map(|finding| finding.rule.as_str())
        .collect();
    assert_eq!(
        rules,
        [
            "bake.frame_count",
            "bake.in_frame",
            "bake.landmark_golden",
            "bake.non_empty",
            "bake.pivot",
            "bake.sampled_frames_are_keys",
            "clip.root_bob",
            "clip.root_travel",
        ]
        .into_iter()
        .collect()
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
