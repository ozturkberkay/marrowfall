//! The bake stage, with a stub standing in for Blender.
//!
//! The real bake is a Blender render, which no test should depend on. What is
//! worth testing is everything around it: the environment handed over, the
//! arguments assembled, the staging directory cleared, and the failure
//! reported. A shell stub that writes the frames Blender would have written
//! covers all of that.

use std::path::Path;

use xtask_art::spec::Paths;
use xtask_art::stages::{self, venv_site_packages};

use crate::support::{EnvGuard, a_library, a_spec, install_library};

/// A repo tree with the script, a virtualenv and the animation library in place.
fn a_baked_repo(with_animations: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("tools/blender/src")).unwrap();
    std::fs::write(root.join("tools/blender/src/bake_sprites.py"), "").unwrap();
    std::fs::create_dir_all(root.join(".venv/lib/python3.13/site-packages")).unwrap();

    let paths = Paths::new(root, "survivor");
    std::fs::create_dir_all(paths.dir()).unwrap();
    std::fs::write(paths.character_glb(), b"glTF").unwrap();
    if with_animations {
        install_library(root);
    }
    dir
}

/// A stub that parses `--out`, writes one PNG there, and reports its argv.
fn a_blender_stub(dir: &Path) -> std::path::PathBuf {
    let stub = dir.join("blender-stub.sh");
    std::fs::write(
        &stub,
        r#"#!/bin/sh
printf '%s\n' "$@" > "$MARROWFALL_STUB_ARGV"
printf '%s\n' "PYTHONPATH=$PYTHONPATH" >> "$MARROWFALL_STUB_ARGV"
out=""
while [ $# -gt 0 ]; do
  if [ "$1" = "--out" ]; then out="$2"; fi
  shift
done
mkdir -p "$out"
: > "$out/idle_s_00.png"
: > "$out/idle_s_01.png"
: > "$out/notes.txt"
exit 0
"#,
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
    let argv = dir.path().join("argv.txt");
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap())
        .set("MARROWFALL_STUB_ARGV", argv.to_str().unwrap());

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
    let argv = dir.path().join("argv.txt");
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap())
        .set("MARROWFALL_STUB_ARGV", argv.to_str().unwrap());

    let mut spec = a_spec("survivor");
    spec.animations.push("run".to_owned());
    stages::bake(
        &spec,
        &library,
        &Paths::new(dir.path(), "survivor"),
        dir.path(),
    )
    .unwrap();

    let seen = std::fs::read_to_string(&argv).unwrap();
    assert_eq!(
        seen.matches("--character").count(),
        1,
        "the mesh is loaded once and shared across animations"
    );
    assert!(seen.contains("idle="), "labelled by our name: {seen}");
    assert!(seen.contains("run="), "labelled by our name: {seen}");
    assert!(
        seen.contains("art/animations/idle.glb"),
        "from the shared library: {seen}"
    );
    assert!(
        seen.contains("art/animations/run.glb"),
        "from the shared library: {seen}"
    );
    assert!(seen.contains("--python-use-system-env"), "got: {seen}");
    assert!(seen.contains("--directions\n8"), "got: {seen}");
    assert!(
        seen.contains("--fps\nrun=24"),
        "a rate per animation: {seen}"
    );
}

#[test]
fn bake_hands_blender_both_the_virtualenv_and_the_scripts_own_directory() {
    let library = a_library();
    let dir = a_baked_repo(true);
    let stub = a_blender_stub(dir.path());
    let argv = dir.path().join("argv.txt");
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap())
        .set("MARROWFALL_STUB_ARGV", argv.to_str().unwrap());

    stages::bake(
        &a_spec("survivor"),
        &library,
        &Paths::new(dir.path(), "survivor"),
        dir.path(),
    )
    .unwrap();

    let seen = std::fs::read_to_string(&argv).unwrap();
    assert!(
        seen.contains("site-packages"),
        "pydantic must be importable inside Blender: {seen}"
    );
    assert!(
        seen.contains("tools/blender/src"),
        "the script's own modules must be importable: {seen}"
    );
}

#[test]
fn stale_frames_from_a_previous_shape_are_cleared_first() {
    let library = a_library();
    let dir = a_baked_repo(true);
    let stub = a_blender_stub(dir.path());
    let argv = dir.path().join("argv.txt");
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap())
        .set("MARROWFALL_STUB_ARGV", argv.to_str().unwrap());

    let paths = Paths::new(dir.path(), "survivor");
    std::fs::create_dir_all(paths.staging()).unwrap();
    std::fs::write(paths.staging().join("idle_s_99.png"), b"stale").unwrap();

    stages::bake(&a_spec("survivor"), &library, &paths, dir.path()).unwrap();

    assert!(
        !paths.staging().join("idle_s_99.png").exists(),
        "a leftover frame would be picked up by packing"
    );
}

#[test]
fn a_failing_blender_reports_both_output_streams() {
    let library = a_library();
    let dir = a_baked_repo(true);
    let stub = dir.path().join("failing.sh");
    std::fs::write(
        &stub,
        "#!/bin/sh\necho 'to stdout'\necho 'to stderr' >&2\nexit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(
        &stub,
        <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
    )
    .unwrap();
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
    assert!(error.contains("to stdout"), "got: {error}");
    assert!(
        error.contains("to stderr"),
        "showing one stream routinely hides the cause: {error}"
    );
}

#[test]
fn a_missing_blender_says_to_check_the_path() {
    let library = a_library();
    let dir = a_baked_repo(true);
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_BLENDER_BIN", "definitely-not-installed-blender");

    let error = format!(
        "{:#}",
        stages::bake(
            &a_spec("survivor"),
            &library,
            &Paths::new(dir.path(), "survivor"),
            dir.path()
        )
        .unwrap_err()
    );
    assert!(error.contains("on PATH"), "got: {error}");
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
