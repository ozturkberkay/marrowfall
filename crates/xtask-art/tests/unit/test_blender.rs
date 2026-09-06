//! The Blender invocation, with a stub standing in for Blender.
//!
//! Two things are worth pinning here. The argument order, because it is a
//! documented silent-failure mode. And the success sentinel, because Blender
//! exits 0 when a script raises anywhere but its own top level, so the exit
//! code alone would record a crash as a success.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use xtask_art::blender::{self, BLENDER_SRC, venv_site_packages};
use xtask_art::check::Artifacts;

use crate::support::EnvGuard;

/// A repo tree with a script and a virtualenv, the two things a run needs.
fn a_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("tools/blender/src")).unwrap();
    std::fs::write(dir.path().join("tools/blender/src/bake_sprites.py"), "").unwrap();
    std::fs::create_dir_all(dir.path().join(".venv/lib/python3.13/site-packages")).unwrap();
    dir
}

/// A stub in place of Blender. It dumps its environment, then does `body`.
fn a_stub(dir: &Path, body: &str) -> PathBuf {
    let stub = dir.join("blender-stub.sh");
    std::fs::write(
        &stub,
        format!(
            "#!/bin/sh\n\
             printf 'PYTHONPATH=%s\\n' \"$PYTHONPATH\" > \"$MARROWFALL_STUB_ENV\"\n\
             printf 'SENTINEL=%s\\n' \"$MARROWFALL_SENTINEL\" >> \"$MARROWFALL_STUB_ENV\"\n\
             printf 'CRASH_BLEND=%s\\n' \"$MARROWFALL_CRASH_BLEND\" >> \"$MARROWFALL_STUB_ENV\"\n\
             printf 'REPORT=%s\\n' \"$MARROWFALL_REPORT\" >> \"$MARROWFALL_STUB_ENV\"\n\
             {body}\n"
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

/// Points the runner at a stub and returns where the stub reported its
/// environment.
fn with_stub(dir: &Path, body: &str, env: &mut EnvGuard) -> PathBuf {
    let stub = a_stub(dir, body);
    let dump = dir.join("stub-env.txt");
    env.set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap())
        .set("MARROWFALL_STUB_ENV", dump.to_str().unwrap());
    dump
}

fn script(root: &Path) -> PathBuf {
    root.join("tools/blender/src/bake_sprites.py")
}

fn artifacts(root: &Path) -> Artifacts {
    Artifacts::new(root, "bake", "survivor", 1).unwrap()
}

// --- the argv -------------------------------------------------------------

#[test]
fn every_flag_is_in_the_one_documented_order() {
    let argv = blender::argv(
        Path::new("/repo/tools/blender/src/bake_sprites.py"),
        Path::new("/repo/art/staging/reports/bake.1.log"),
        &[OsString::from("--character"), OsString::from("/repo/x.glb")],
    );

    // Order is the contract. A reordering here is a silent failure at runtime,
    // so this list is written out rather than searched.
    assert_eq!(
        argv,
        [
            "--background",
            "--factory-startup",
            "--offline-mode",
            "--python-use-system-env",
            "--python-exit-code",
            "1",
            "--log-level",
            "debug",
            "--log-file",
            "/repo/art/staging/reports/bake.1.log",
            "--python",
            "/repo/tools/blender/src/bake_sprites.py",
            "--",
            "--character",
            "/repo/x.glb",
        ]
        .map(OsString::from)
    );
}

#[test]
fn the_two_load_bearing_flags_are_never_dropped() {
    let argv = blender::argv(Path::new("s.py"), Path::new("s.log"), &[]);

    assert!(
        argv.contains(&OsString::from("--python-use-system-env")),
        "without it PYTHONPATH never reaches Blender's interpreter: {argv:?}"
    );
    assert!(
        argv.contains(&OsString::from("--python-exit-code")),
        "defense in depth for a top-level raise: {argv:?}"
    );
}

#[test]
fn the_log_is_asked_for_at_debug_level() {
    let argv = blender::argv(Path::new("s.py"), Path::new("s.log"), &[]);
    let level = argv.iter().position(|a| a == "--log-level").unwrap();

    assert_eq!(
        argv[level + 1],
        OsString::from("debug"),
        "with --log-file alone Blender writes an empty file"
    );
}

#[test]
fn a_name_value_pair_is_one_argument() {
    assert_eq!(
        blender::pair("idle", "art/animations/idle.glb"),
        OsString::from("idle=art/animations/idle.glb")
    );
}

// --- the sentinel ---------------------------------------------------------

#[test]
fn a_script_that_finishes_leaves_a_sentinel_and_the_run_succeeds() {
    let dir = a_repo();
    let mut env = EnvGuard::new();
    with_stub(dir.path(), ": > \"$MARROWFALL_SENTINEL\"", &mut env);
    let artifacts = artifacts(dir.path());

    blender::run(&script(dir.path()), &[], &artifacts, dir.path()).unwrap();

    assert!(artifacts.sentinel().exists());
}

#[test]
fn blender_exiting_zero_without_a_sentinel_fails_the_run() {
    let dir = a_repo();
    let mut env = EnvGuard::new();
    with_stub(dir.path(), "exit 0", &mut env);
    let artifacts = artifacts(dir.path());

    let error = blender::run(&script(dir.path()), &[], &artifacts, dir.path())
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("outside the script's top level"),
        "a raise from a handler, a thread or atexit exits 0: {error}"
    );
}

#[test]
fn a_sentinel_from_an_earlier_run_cannot_pass_this_one() {
    let dir = a_repo();
    let mut env = EnvGuard::new();
    with_stub(dir.path(), "exit 0", &mut env);
    let artifacts = artifacts(dir.path());
    std::fs::create_dir_all(artifacts.dir()).unwrap();
    std::fs::write(artifacts.sentinel(), "{\"ok\": true}").unwrap();

    assert!(
        blender::run(&script(dir.path()), &[], &artifacts, dir.path()).is_err(),
        "a stale sentinel would record a crash as a success"
    );
    assert!(!artifacts.sentinel().exists());
}

#[test]
fn a_sentinel_that_cannot_be_cleared_stops_the_run() {
    let dir = a_repo();
    let mut env = EnvGuard::new();
    with_stub(dir.path(), ": > \"$MARROWFALL_SENTINEL\"", &mut env);
    let artifacts = artifacts(dir.path());
    // A directory where the sentinel belongs: the run must stop rather than
    // carry on with a sentinel it can neither delete nor trust.
    std::fs::create_dir_all(artifacts.sentinel()).unwrap();

    let error = format!(
        "{:#}",
        blender::run(&script(dir.path()), &[], &artifacts, dir.path()).unwrap_err()
    );

    assert!(error.contains("clearing"), "got: {error}");
}

#[test]
fn a_report_from_an_earlier_run_cannot_pass_this_one() {
    let dir = a_repo();
    let mut env = EnvGuard::new();
    with_stub(dir.path(), ": > \"$MARROWFALL_SENTINEL\"", &mut env);
    let artifacts = artifacts(dir.path());
    std::fs::create_dir_all(artifacts.dir()).unwrap();
    std::fs::write(
        artifacts.report(),
        r#"{"stage":"bake","item":"survivor","attempt":1,"findings":[{
            "rule":"bake.pivot","severity":"error","subject":"idle_s_00",
            "measured":20.0,"limit":1.0,"comparison":"le","unit":"px",
            "attempt":1,"measured_on":"pixels, cropped frame",
            "message":"the ground line drifts 20 px"}]}"#,
    )
    .unwrap();

    let report = blender::run(&script(dir.path()), &[], &artifacts, dir.path()).unwrap();

    assert!(
        report.is_none(),
        "a run that measured nothing must not inherit the last run's findings"
    );
    assert!(!artifacts.report().exists());
}

#[test]
fn a_report_whose_header_disagrees_with_its_path_is_refused() {
    let dir = a_repo();
    let mut env = EnvGuard::new();
    with_stub(
        dir.path(),
        ": > \"$MARROWFALL_SENTINEL\"\n\
         printf '%s' \"$MARROWFALL_REPORT_BODY\" > \"$MARROWFALL_REPORT\"",
        &mut env,
    );
    // Written into bake.survivor.1.json while claiming to be a clip report.
    env.set(
        "MARROWFALL_REPORT_BODY",
        r#"{"stage":"clip","item":"idle","attempt":1,"findings":[]}"#,
    );

    let error = blender::run(&script(dir.path()), &[], &artifacts(dir.path()), dir.path())
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("clip.idle.1.json"),
        "a mislabeled report is attributed to the wrong stage: {error}"
    );
}

#[test]
fn the_script_is_told_where_to_write_the_sentinel_and_the_crash_blend() {
    let dir = a_repo();
    let mut env = EnvGuard::new();
    let dump = with_stub(dir.path(), ": > \"$MARROWFALL_SENTINEL\"", &mut env);
    let artifacts = artifacts(dir.path());

    blender::run(&script(dir.path()), &[], &artifacts, dir.path()).unwrap();

    let seen = std::fs::read_to_string(dump).unwrap();
    assert!(
        seen.contains(&format!("SENTINEL={}", artifacts.sentinel().display())),
        "got: {seen}"
    );
    assert!(
        seen.contains(&format!("CRASH_BLEND={}", artifacts.blend().display())),
        "got: {seen}"
    );
    assert!(
        seen.contains(&format!("REPORT={}", artifacts.report().display())),
        "the runner owns every report name, so no script derives one: {seen}"
    );
}

#[test]
fn the_findings_a_script_wrote_come_back_from_the_run() {
    let dir = a_repo();
    let mut env = EnvGuard::new();
    with_stub(
        dir.path(),
        ": > \"$MARROWFALL_SENTINEL\"\n\
         printf '%s' \"$MARROWFALL_REPORT_BODY\" > \"$MARROWFALL_REPORT\"",
        &mut env,
    );
    let artifacts = artifacts(dir.path());
    env.set(
        "MARROWFALL_REPORT_BODY",
        r#"{"stage":"bake","item":"survivor","attempt":1,"findings":[{
            "rule":"bake.pivot","severity":"error","subject":"idle_s_00",
            "measured":20.0,"limit":1.0,"comparison":"le","unit":"px",
            "attempt":1,"measured_on":"pixels, cropped frame",
            "message":"the ground line drifts 20 px"}]}"#,
    );

    let report = blender::run(&script(dir.path()), &[], &artifacts, dir.path())
        .unwrap()
        .expect("the script wrote a report");

    assert!(
        report.has_errors(),
        "an Err means the run broke, not the art"
    );
    assert_eq!(report.item(), "survivor");
}

#[test]
fn a_script_that_reports_nothing_is_still_a_finished_run() {
    let dir = a_repo();
    let mut env = EnvGuard::new();
    with_stub(dir.path(), ": > \"$MARROWFALL_SENTINEL\"", &mut env);

    let report =
        blender::run(&script(dir.path()), &[], &artifacts(dir.path()), dir.path()).unwrap();

    assert!(report.is_none(), "the bake measures nothing until T14");
}

#[test]
fn a_report_the_script_wrote_wrongly_fails_the_run() {
    let dir = a_repo();
    let mut env = EnvGuard::new();
    with_stub(
        dir.path(),
        ": > \"$MARROWFALL_SENTINEL\"\nprintf 'not json' > \"$MARROWFALL_REPORT\"",
        &mut env,
    );

    assert!(
        blender::run(&script(dir.path()), &[], &artifacts(dir.path()), dir.path()).is_err(),
        "a report nobody can parse is not a measurement"
    );
}

#[test]
fn blender_gets_both_the_virtualenv_and_the_scripts_own_directory() {
    let dir = a_repo();
    let mut env = EnvGuard::new();
    let dump = with_stub(dir.path(), ": > \"$MARROWFALL_SENTINEL\"", &mut env);
    let artifacts = artifacts(dir.path());

    blender::run(&script(dir.path()), &[], &artifacts, dir.path()).unwrap();

    let seen = std::fs::read_to_string(dump).unwrap();
    assert!(
        seen.contains("site-packages"),
        "pydantic must be importable inside Blender: {seen}"
    );
    assert!(
        seen.contains("tools/blender/src"),
        "the script's own modules must be importable: {seen}"
    );
}

// --- the diagnostics ------------------------------------------------------

#[test]
fn a_failed_run_leaves_the_argv_verbatim_and_names_where_to_look() {
    let dir = a_repo();
    let mut env = EnvGuard::new();
    with_stub(
        dir.path(),
        "echo 'to stdout'\necho 'to stderr' >&2\nexit 1",
        &mut env,
    );
    let artifacts = artifacts(dir.path());

    let error = blender::run(
        &script(dir.path()),
        &[OsString::from("--character")],
        &artifacts,
        dir.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("to stdout"), "got: {error}");
    assert!(
        error.contains("to stderr"),
        "showing one stream routinely hides the cause: {error}"
    );
    assert!(
        error.contains(&artifacts.dir().display().to_string()),
        "the message must say where the diagnostics are: {error}"
    );

    let argv = std::fs::read_to_string(artifacts.argv()).unwrap();
    let lines: Vec<&str> = argv.lines().collect();
    assert!(lines[0].ends_with("blender-stub.sh"), "got: {argv}");
    assert_eq!(lines[1], "--background", "got: {argv}");
    assert_eq!(lines.last().unwrap(), &"--character", "got: {argv}");
    let log = artifacts.log().display().to_string();
    assert!(
        lines.contains(&log.as_str()),
        "Blender must log to the file the diagnostics name, got: {argv}"
    );
}

#[test]
fn a_missing_blender_says_to_check_the_path() {
    let dir = a_repo();
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_BLENDER_BIN", "definitely-not-installed-blender");
    let artifacts = artifacts(dir.path());

    let error = format!(
        "{:#}",
        blender::run(&script(dir.path()), &[], &artifacts, dir.path()).unwrap_err()
    );
    assert!(error.contains("on PATH"), "got: {error}");
}

// --- the virtualenv lookup ------------------------------------------------

#[test]
fn the_virtualenv_is_found_by_python_version_rather_than_hardcoded() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".venv/lib/python3.13/site-packages")).unwrap();

    let found = venv_site_packages(dir.path()).unwrap();
    assert!(found.ends_with("python3.13/site-packages"), "{found:?}");
}

#[test]
fn the_newest_python_wins_when_several_are_present() {
    let dir = tempfile::tempdir().unwrap();
    for version in ["python3.9", "python3.13"] {
        std::fs::create_dir_all(
            dir.path()
                .join(".venv/lib")
                .join(version)
                .join("site-packages"),
        )
        .unwrap();
    }

    let found = venv_site_packages(dir.path()).unwrap();
    assert!(found.ends_with("python3.13/site-packages"), "{found:?}");
}

#[test]
fn no_virtualenv_says_to_run_uv_sync() {
    let dir = tempfile::tempdir().unwrap();
    let error = venv_site_packages(dir.path()).unwrap_err().to_string();
    assert!(error.contains("uv sync"), "got: {error}");
}

#[test]
fn a_virtualenv_with_no_site_packages_also_says_to_run_uv_sync() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".venv/lib/python3.13")).unwrap();
    let error = venv_site_packages(dir.path()).unwrap_err().to_string();
    assert!(error.contains("uv sync"), "got: {error}");
}

// --- the one thing a script may never do ---------------------------------

/// `transform_apply` on a rig that owns an action rescales the rest geometry
/// and leaves every location key byte identical, so their meaning changes by
/// the object's scale and 2.316 m of travel reads as 231.599 m. World
/// matrices are composed instead, so no script needs it and none may have it.
///
/// A lint rather than a review note: the call was in `align_to_world` until
/// the new transfer deleted the last caller, and nothing else would notice it
/// coming back.
const FORBIDDEN: &str = "transform_apply";

/// Every line of a Python file that is code, with comments and docstrings
/// dropped so the rule can still be explained in prose where it is enforced.
fn code_lines(source: &str) -> Vec<&str> {
    let mut inside = false;
    let mut code = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        let comment = !inside && trimmed.starts_with('#');
        let quoted = comment || (!inside && trimmed.starts_with("\"\"\""));
        if !inside && !quoted {
            code.push(line);
        }
        // A stray triple quote inside a comment must not open a docstring.
        if !comment && line.matches("\"\"\"").count() % 2 == 1 {
            inside = !inside;
        }
    }
    code
}

#[test]
fn a_stray_triple_quote_in_a_comment_hides_nothing() {
    let source = "# prose with a stray \"\"\" in it\nbpy.ops.object.transform_apply()\n";
    assert_eq!(code_lines(source), vec!["bpy.ops.object.transform_apply()"]);
}

/// Every `.py` under the scripts directory, by name.
fn blender_scripts() -> Vec<(String, String)> {
    let dir = crate::support::repo_root().join(BLENDER_SRC);
    let mut scripts: Vec<(String, String)> = std::fs::read_dir(&dir)
        .expect("the Blender scripts directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|kind| kind == "py"))
        .map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            (
                name,
                std::fs::read_to_string(&path).expect("a readable script"),
            )
        })
        .collect();
    scripts.sort();
    scripts
}

#[test]
fn no_blender_script_applies_an_object_transform() {
    let scripts = blender_scripts();

    // Every module, not only the entry points: `actions.py` holds the
    // F-curve edits both the retarget and the bake make.
    assert!(scripts.len() >= 12, "found {} scripts", scripts.len());
    for (name, source) in &scripts {
        for line in code_lines(source) {
            assert!(
                !line.contains(FORBIDDEN),
                "{name} calls {FORBIDDEN}: {}",
                line.trim()
            );
        }
    }
}

/// Every size the fixer acts at is published to it on argv, so no second
/// copy of one can drift from the profile that states it.
#[test]
fn the_mesh_fixer_holds_no_size_of_its_own() {
    let scripts = blender_scripts();
    let fixer = |wanted: &str| {
        let (_, source) = scripts
            .iter()
            .find(|(name, _)| name == wanted)
            .unwrap_or_else(|| panic!("{wanted}"));
        code_lines(source).join("\n")
    };
    let shell = fixer("mesh_clean.py");

    for read in [
        "args.weld",
        "args.island_volume",
        "args.symmetry_threshold",
        "args.symmetry",
        "args.meshes",
    ] {
        assert!(shell.contains(read), "mesh_clean.py never reads {read}");
    }
    // Both halves of the fixer: the shell that runs it and the module that
    // decides what it does. The weld distance, the island floor, the mirror
    // threshold and the one mesh name the profile allows, in every spelling
    // Python would print.
    for (name, code) in [
        ("mesh_clean.py", shell),
        ("cleanup.py", fixer("cleanup.py")),
    ] {
        for published in [
            "1e-5", "1e-05", "0.00001", "1e-6", "1e-06", "0.000001", "0.001", "char1",
        ] {
            assert!(
                !code.contains(published),
                "{name} holds {published}, which the runner already publishes"
            );
        }
    }
}

/// The one thing every glTF export that carries an armature must ask for.
///
/// `clip.twist` reads its rest term off the joints of the file it is handed,
/// so the armature has to leave Blender at its rest position. That is the
/// exporter's default, and a default can move: with the flag off, a correct
/// `strafe_left` fit reads 113.884 degrees where it should read 11.411, and
/// 20 of its 22 roles go red.
const REST_POSITION: &str = "export_rest_position_armature=True";

const EXPORT: &str = "export_scene.gltf(";

/// An export with no armature in it has no pose to get wrong, which is the
/// mesh fixer: it writes geometry and drops every skin.
const WITH_ARMATURE: &str = "export_skins=True";

#[test]
fn every_gltf_export_that_carries_an_armature_asks_for_its_rest_position() {
    let mut written = (0, 0);
    for (name, source) in blender_scripts() {
        let code = code_lines(&source).join("\n");
        let (skinned, at_rest) = (
            code.matches(WITH_ARMATURE).count(),
            code.matches(REST_POSITION).count(),
        );
        assert_eq!(
            skinned, at_rest,
            "{name} exports {skinned} armature(s) and asks for the rest \
             position {at_rest} time(s)"
        );
        written = (
            written.0 + code.matches(EXPORT).count(),
            written.1 + skinned,
        );
    }

    // A rename that left nothing exporting would pass without proving
    // anything: three scripts write a GLB, and the retarget and the strip
    // are the two that put an armature in one.
    assert_eq!(written, (3, 2));
}

/// The lint's own negative: an export that leaves the flag to the default.
#[test]
fn the_lint_catches_an_export_that_leaves_the_rest_position_out() {
    let source = "bpy.ops.export_scene.gltf(\n    filepath=str(out),\n)\n";

    let code = code_lines(source).join("\n");

    assert_eq!(code.matches(EXPORT).count(), 1);
    assert_eq!(code.matches(REST_POSITION).count(), 0);
}

/// The lint's own negative: the same scan over a script that has it back.
#[test]
fn the_lint_catches_the_call_coming_back() {
    let reinserted = "\
def align_to_world(rig):
    \"\"\"Forbidden: transform_apply, in prose, is not a call.\"\"\"
    # transform_apply in a comment is not a call either.
    bpy.ops.object.transform_apply(rotation=True)
";

    let code: Vec<&str> = code_lines(reinserted);

    let caught: Vec<&str> = code
        .iter()
        .copied()
        .filter(|line| line.contains(FORBIDDEN))
        .collect();

    assert_eq!(
        caught,
        ["    bpy.ops.object.transform_apply(rotation=True)"],
        "prose and comments are not calls, and the call is: {code:#?}"
    );
}
