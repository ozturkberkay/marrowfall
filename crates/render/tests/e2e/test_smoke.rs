//! The gate: the committed project, imported and booted headless.
//!
//! Loads only, never pixels. What the player sees is `test_visual`, which
//! needs a window and is therefore a local verb rather than a gate.

use std::time::Duration;

use crate::{engine, fixture};

/// Long enough for the sim thread to reach its first tick and for the first
/// resident window of chunks to be painted, both of which are a worker thread
/// racing the frame loop. Measured here: both land by frame 10 and 600 take
/// 4.4 s, so the margin is what a slower runner gets to spend.
const BOOT_FRAMES: &str = "600";

/// Enough for a run that only has to reach `_ready` and exit again. The leak
/// line is printed at exit, so it does have to exit.
const READY_FRAMES: &str = "5";

/// What the watchdog is given for the run that never exits. Short, because
/// the test is over the moment the kill lands.
const SHORT_WATCHDOG: Duration = Duration::from_secs(5);

#[test]
fn the_committed_project_boots_with_every_atlas_the_game_reads() {
    let Some(godot) = engine::godot_or_skip() else {
        return;
    };
    let root = fixture::root();
    engine::build_extension(&root);
    let scratch = fixture::scratch("boots");
    let project = fixture::project();

    // The atlases are loadable only through what this writes. A checkout has
    // no `.godot/`, so without it the run fails on the first texture.
    engine::import(&godot, &scratch.join("import.log"), &project).expect_clean();

    // The sidecars are the pack stage's output and are committed. An import
    // that rewrites one means the pipeline shipped a stub, so what the game
    // loads is not what was reviewed.
    let diff = std::process::Command::new("git")
        .args(["diff", "--exit-code", "--", "project/**/*.import"])
        .current_dir(&root)
        .status()
        .expect("running git diff");
    assert!(
        diff.success(),
        "the import rewrote a committed .import sidecar"
    );

    let run = engine::run(
        &godot,
        &scratch.join("boot.log"),
        &[
            "--headless",
            "--path",
            engine::text(&project),
            "--quit-after",
            BOOT_FRAMES,
        ],
    );
    run.expect_clean();

    // A quiet run is not a passing one: it is also what a Godot that loaded
    // no extension looks like. So every step says so out loud.
    run.expect_printed("Initialize godot-rust");
    run.expect_printed("[marrowfall] sim thread live at tick");
    run.expect_printed("[marrowfall] world streaming:");
    for (manifest, assets) in fixture::manifests() {
        let count = assets.animations.len();
        run.expect_printed(&format!(
            "{}: {count} of {count} atlases loaded",
            fixture::resource_path(&manifest)
        ));
    }
}

#[test]
fn a_manifest_whose_frame_count_is_wrong_fails_the_gate() {
    let Some(godot) = engine::godot_or_skip() else {
        return;
    };
    let root = fixture::root();
    engine::build_extension(&root);
    let scratch = fixture::scratch("corrupt");
    let project = fixture::project_copy(&scratch);

    // One animation now claims one frame more than it has rects for, which is
    // the invariant `sprites::parse` refuses a manifest on.
    let manifest = project.join("assets/characters/survivor/character.ron");
    let text = std::fs::read_to_string(&manifest).expect("reading the copied manifest");
    let broken = text.replacen("frames: 15,", "frames: 16,", 1);
    assert_ne!(text, broken, "the manifest no longer holds a 15 frame clip");
    std::fs::write(&manifest, broken).expect("writing the broken manifest");

    engine::import(&godot, &scratch.join("import.log"), &project).expect_clean();
    let run = engine::run(
        &godot,
        &scratch.join("boot.log"),
        &[
            "--headless",
            "--path",
            engine::text(&project),
            "--quit-after",
            READY_FRAMES,
        ],
    );

    let problem = run.problem().expect("the broken manifest to fail the gate");
    assert!(
        problem.contains("rects must hold one entry per direction per frame"),
        "the gate failed on something else: {problem}"
    );
}

#[test]
fn a_script_error_and_a_leaked_object_both_fail_the_gate() {
    let Some(godot) = engine::godot_or_skip() else {
        return;
    };
    engine::build_extension(&fixture::root());
    let scratch = fixture::scratch("script_error");
    let project = fixture::project_copy(&scratch);
    let probe = fixture::probe_scene(&project);
    engine::import(&godot, &scratch.join("import.log"), &project).expect_clean();

    let run = engine::run(
        &godot,
        &scratch.join("probe.log"),
        &[
            "--headless",
            "--path",
            engine::text(&project),
            probe,
            "--quit-after",
            READY_FRAMES,
        ],
    );

    // Godot leaves both of these at exit 0, which is why the log is the gate.
    let problem = run.problem().expect("the probe scene to fail the gate");
    assert!(
        problem.contains("SCRIPT ERROR"),
        "the script error is missing: {problem}"
    );
    assert!(
        problem.contains("ObjectDB instance"),
        "the leaked object is missing: {problem}"
    );
}

#[test]
fn a_run_that_never_exits_is_killed_by_the_watchdog() {
    let Some(godot) = engine::godot_or_skip() else {
        return;
    };
    engine::build_extension(&fixture::root());
    let scratch = fixture::scratch("watchdog");
    let project = fixture::project_copy(&scratch);
    let probe = fixture::probe_scene(&project);

    let run = engine::run_within(
        &godot,
        &scratch.join("hang.log"),
        &[
            "--headless",
            "--path",
            engine::text(&project),
            probe,
            "--",
            "--hang",
        ],
        SHORT_WATCHDOG,
    );

    let problem = run.problem().expect("the hanging run to fail the gate");
    assert!(
        problem.contains("the watchdog killed it"),
        "something other than the watchdog ended it: {problem}"
    );
}
