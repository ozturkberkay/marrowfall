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

/// A stub that parses `--out`, writes one PNG there, and finishes the way a
/// real script does: with the success sentinel.
fn a_blender_stub(dir: &Path) -> std::path::PathBuf {
    let stub = dir.join("blender-stub.sh");
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
: > "$out/idle_s_01.png"
: > "$out/notes.txt"
: > "$MARROWFALL_SENTINEL"
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

/// The bake reads no findings yet, so a report it produced would vanish. The
/// tripwire goes when T14 wires the `bake.*` rules in.
#[test]
fn a_bake_that_reports_findings_stops_rather_than_dropping_them() {
    let library = a_library();
    let dir = a_baked_repo(true);
    let stub = dir.path().join("reporting.sh");
    std::fs::write(
        &stub,
        r#"#!/bin/sh
: > "$MARROWFALL_SENTINEL"
printf '%s' '{"stage":"bake","item":"survivor","attempt":1,"findings":[]}'   > "$MARROWFALL_REPORT"
exit 0
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

    let error = stages::bake(
        &a_spec("survivor"),
        &library,
        &Paths::new(dir.path(), "survivor"),
        dir.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("nobody reads yet"), "got: {error}");
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
