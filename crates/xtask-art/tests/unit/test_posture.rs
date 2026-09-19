//! `cargo art posture`: which file it opens, in whose naming, and what it
//! files the reading under.
//!
//! The angles themselves are `tools/blender/tests/unit/test_posture.py`'s,
//! where they run with no Blender. What is tested here is the wiring: a
//! fitted clip is read on today's canonical rig, a source is read on the
//! vendor's own, and the two do not overwrite each other's artifacts.

use std::path::Path;

use clap::Parser as _;
use xtask_art::cli::{Cli, Command};
use xtask_art::library::AnimationLibrary;
use xtask_art::posture::{self, Asked};

use crate::stubs::a_stub;
use crate::support::{EnvGuard, committed_glb, install_library, install_scripts, install_skeleton};

/// A repo-shaped temp tree holding the library, the skeleton and the scripts.
fn a_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("crates")).unwrap();
    install_library(dir.path());
    install_skeleton(dir.path());
    install_scripts(dir.path());
    // What Blender's own interpreter imports our modules through.
    std::fs::create_dir_all(dir.path().join(".venv/lib/python3.13/site-packages")).unwrap();
    dir
}

/// Blender as this command uses it: it writes the text at `--out` and the
/// sentinel, and no report.
fn a_reading_stub(dir: &Path) -> std::path::PathBuf {
    a_stub(
        dir,
        "blender-posture.sh",
        r#"printf '%s\n' "$@" >> "$MARROWFALL_STUB_ARGV"
out=""
while [ $# -gt 0 ]; do
  if [ "$1" = "--out" ]; then out="$2"; fi
  shift
done
mkdir -p "$(dirname "$out")"
printf 'skull pitch 38.2\n' > "$out"
: > "$MARROWFALL_SENTINEL"
exit 0
"#,
    )
}

/// The tree, the stub, and the file the stub's argv is recorded in.
fn a_repo_that_reads() -> (tempfile::TempDir, EnvGuard) {
    let dir = a_repo();
    let mut env = EnvGuard::new();
    env.set(
        "MARROWFALL_BLENDER_BIN",
        a_reading_stub(dir.path()).to_str().unwrap(),
    )
    .set(
        "MARROWFALL_STUB_ARGV",
        dir.path().join("argv.txt").to_str().unwrap(),
    );
    (dir, env)
}

fn argv_of(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("argv.txt")).unwrap()
}

fn asked(clip: Option<&str>, source: Option<&str>, rest: Option<&Path>) -> Asked {
    Asked {
        clip: clip.map(str::to_owned),
        source: source.map(str::to_owned),
        rest: rest.map(Path::to_path_buf),
        frames: None,
    }
}

// --- What the command line accepts ----------------------------------------

#[test]
fn one_subject_is_required_and_only_one() {
    let parsed = Cli::try_parse_from(["cargo art", "posture", "idle"]).unwrap();
    let Command::Posture(asked) = parsed.command else {
        panic!("a posture command");
    };
    assert_eq!(asked.clip.as_deref(), Some("idle"));

    for argv in [
        vec!["cargo art", "posture"],
        vec!["cargo art", "posture", "idle", "--source", "idle"],
        vec!["cargo art", "posture", "--rest", "rig.glb", "--frames", "0"],
    ] {
        assert!(
            Cli::try_parse_from(&argv).is_err(),
            "{argv:?} names no one subject"
        );
    }
}

#[test]
fn the_frames_are_taken_as_written_and_handed_on() {
    let parsed =
        Cli::try_parse_from(["cargo art", "posture", "idle", "--frames", "0,12,24"]).unwrap();
    let Command::Posture(asked) = parsed.command else {
        panic!("a posture command");
    };

    assert_eq!(asked.frames.as_deref(), Some("0,12,24"));
}

// --- A fitted clip --------------------------------------------------------

#[test]
fn a_fitted_clip_is_read_on_todays_canonical_rig() {
    let (dir, _env) = a_repo_that_reads();
    let root = dir.path();

    posture::run(root, &asked(Some("idle"), None, None)).unwrap();

    let argv = argv_of(root);
    let rig = AnimationLibrary::reference_rig(root, "humanoid");
    let clip = AnimationLibrary::load(root).unwrap().glb(root, "idle");
    assert!(argv.contains(rig.to_str().unwrap()), "{argv}");
    assert!(argv.contains(clip.to_str().unwrap()), "{argv}");
    assert!(argv.contains("--convention\nstandard\n"), "{argv}");
    // The clip's own rate, which is what the glTF importer turns key times
    // in seconds into frames with.
    assert!(argv.contains("--source-fps\n24\n"), "{argv}");
}

#[test]
fn the_reading_is_printed_and_kept_under_the_clips_own_name() {
    let (dir, _env) = a_repo_that_reads();
    let root = dir.path();

    posture::run(root, &asked(Some("idle"), None, None)).unwrap();

    let text = root.join("art/staging/reports/posture.idle.1.txt");
    assert_eq!(std::fs::read_to_string(text).unwrap(), "skull pitch 38.2\n");
}

#[test]
fn only_the_frames_asked_for_are_passed_on() {
    let (dir, _env) = a_repo_that_reads();
    let mut wanted = asked(Some("idle"), None, None);
    wanted.frames = Some("0,12,24".to_owned());

    posture::run(dir.path(), &wanted).unwrap();

    assert!(argv_of(dir.path()).contains("--frames\n0,12,24\n"));
}

#[test]
fn a_clip_with_no_fit_on_this_machine_names_the_file_and_the_fetch() {
    let (dir, _env) = a_repo_that_reads();
    let glb = AnimationLibrary::load(dir.path())
        .unwrap()
        .glb(dir.path(), "walk_back");
    std::fs::remove_file(&glb).unwrap();

    let error = format!(
        "{:#}",
        posture::run(dir.path(), &asked(Some("walk_back"), None, None)).unwrap_err()
    );

    assert!(
        error.contains("art/animations/local/walk_back.glb"),
        "{error}"
    );
    assert!(error.contains("cargo art fetch walk_back"), "{error}");
}

#[test]
fn a_name_the_library_does_not_declare_is_refused() {
    let (dir, _env) = a_repo_that_reads();

    let error = format!(
        "{:#}",
        posture::run(dir.path(), &asked(Some("moonwalk"), None, None)).unwrap_err()
    );

    assert!(error.contains("moonwalk"), "{error}");
}

// --- A vendor file --------------------------------------------------------

#[test]
fn a_source_is_read_on_the_vendors_own_rig_and_in_the_vendors_own_naming() {
    let (dir, _env) = a_repo_that_reads();
    let root = dir.path();
    let fbx = AnimationLibrary::staged_download(root, "walk_back", "fbx");
    std::fs::create_dir_all(fbx.parent().unwrap()).unwrap();
    std::fs::write(&fbx, b"an fbx").unwrap();

    posture::run(root, &asked(None, Some("walk_back"), None)).unwrap();

    let argv = argv_of(root);
    assert!(argv.contains(fbx.to_str().unwrap()), "{argv}");
    assert!(argv.contains("--convention\nmixamo\n"), "{argv}");
    // No rig of ours: a vendor file carries its own.
    assert!(!argv.contains("--rig\n"), "{argv}");
}

#[test]
fn a_fitted_clip_and_its_source_are_filed_apart() {
    let (dir, _env) = a_repo_that_reads();
    let root = dir.path();
    let source = AnimationLibrary::source_clip(root, "idle");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(&source, b"glTF a source").unwrap();

    posture::run(root, &asked(Some("idle"), None, None)).unwrap();
    posture::run(root, &asked(None, Some("idle"), None)).unwrap();

    let reports = root.join("art/staging/reports");
    assert!(reports.join("posture.idle.1.txt").exists());
    assert!(reports.join("posture.idle_source.1.txt").exists());
}

#[test]
fn a_clip_with_no_vendor_file_on_this_machine_says_so() {
    let (dir, _env) = a_repo_that_reads();

    let error = format!(
        "{:#}",
        posture::run(dir.path(), &asked(None, Some("idle"), None)).unwrap_err()
    );

    assert!(error.contains("no vendor file on this machine"), "{error}");
}

// --- A bind pose ----------------------------------------------------------

#[test]
fn a_rig_at_rest_is_read_in_whichever_convention_its_own_bones_name() {
    let (dir, _env) = a_repo_that_reads();
    let rig = committed_glb("art/skeletons/humanoid.glb");

    posture::run(dir.path(), &asked(None, None, Some(&rig))).unwrap();

    let argv = argv_of(dir.path());
    assert!(argv.contains("--convention\nstandard\n"), "{argv}");
    // No clip, so the bones themselves are the pose and no action can stand
    // in for a rest one.
    assert!(!argv.contains("--clip\n"), "{argv}");
    assert!(
        dir.path()
            .join("art/staging/reports/posture.humanoid_rest.1.txt")
            .exists()
    );
}

#[test]
fn a_rest_path_that_holds_no_skeleton_names_the_file() {
    let (dir, _env) = a_repo_that_reads();
    let not_a_rig = dir.path().join("notes.txt");
    std::fs::write(&not_a_rig, "no bones here").unwrap();

    let error = format!(
        "{:#}",
        posture::run(dir.path(), &asked(None, None, Some(&not_a_rig))).unwrap_err()
    );

    assert!(error.contains("notes.txt"), "{error}");
}

// --- What is refused before Blender is asked ------------------------------

#[test]
fn a_tree_with_no_posture_script_says_which_file_is_missing() {
    let (dir, _env) = a_repo_that_reads();
    let script = dir.path().join("tools/blender/src/read_posture.py");
    std::fs::remove_file(&script).unwrap();

    let error = format!(
        "{:#}",
        posture::run(dir.path(), &asked(Some("idle"), None, None)).unwrap_err()
    );

    assert!(error.contains("read_posture.py"), "{error}");
}

#[test]
fn a_missing_canonical_rig_says_a_fit_is_read_on_the_rig_it_was_fitted_to() {
    let (dir, _env) = a_repo_that_reads();
    std::fs::remove_file(AnimationLibrary::reference_rig(dir.path(), "humanoid")).unwrap();

    let error = format!(
        "{:#}",
        posture::run(dir.path(), &asked(Some("idle"), None, None)).unwrap_err()
    );

    assert!(error.contains("art/skeletons/humanoid.glb"), "{error}");
}

#[test]
fn a_run_that_wrote_no_reading_is_refused_rather_than_printed_empty() {
    let dir = a_repo();
    let mut env = EnvGuard::new();
    let quiet = a_stub(
        dir.path(),
        "blender-quiet.sh",
        ": > \"$MARROWFALL_SENTINEL\"\nexit 0\n",
    );
    env.set("MARROWFALL_BLENDER_BIN", quiet.to_str().unwrap());

    let error = format!(
        "{:#}",
        posture::run(dir.path(), &asked(Some("idle"), None, None)).unwrap_err()
    );

    assert!(error.contains("wrote no"), "{error}");
}

#[test]
fn a_bare_path_needs_the_repository_to_declare_exactly_one_skeleton() {
    let (dir, _env) = a_repo_that_reads();
    let rig = committed_glb("art/skeletons/humanoid.glb");
    let skeletons = dir.path().join("art/skeletons");
    std::fs::copy(
        skeletons.join("humanoid.toml"),
        skeletons.join("quadruped.toml"),
    )
    .unwrap();

    let two = format!(
        "{:#}",
        posture::run(dir.path(), &asked(None, None, Some(&rig))).unwrap_err()
    );
    std::fs::remove_dir_all(&skeletons).unwrap();
    let none = format!(
        "{:#}",
        posture::run(dir.path(), &asked(None, None, Some(&rig))).unwrap_err()
    );

    assert!(two.contains("quadruped"), "{two}");
    assert!(none.contains("no skeleton in"), "{none}");
}
