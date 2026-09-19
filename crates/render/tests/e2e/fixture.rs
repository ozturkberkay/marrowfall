//! The project this tier drives, and private copies of it.

use std::path::{Path, PathBuf};
use std::process::Command;

use sprites::CharacterAssets;

/// The import cache. Gitignored, and Godot rewrites it on every run.
const CACHE: &str = ".godot";

/// The worktree root. This crate's manifest sits two levels under it.
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("resolving the worktree root")
}

/// The committed Godot project.
pub fn project() -> PathBuf {
    root().join("project")
}

/// An empty directory of one test's own, under the gitignored `target/`.
///
/// A [`Scratch`] rather than a path, because a project copy is tens of
/// megabytes and nothing else would ever remove it.
pub fn scratch(label: &str) -> Scratch {
    let dir = root().join("target/e2e").join(label);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("creating the scratch directory");
    Scratch(dir)
}

/// A [`scratch`] directory, removed when the test that asked for it ends.
///
/// A failing test keeps its own: the logs under it are the only account of
/// what Godot did, and the panic that failed the test names them.
pub struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

impl std::ops::Deref for Scratch {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

/// A private copy of the project inside `scratch`, needing one import.
///
/// [`CACHE`] is left behind rather than copied, because a test that drives
/// the engine must neither read a cache another test is rewriting nor write
/// the committed one.
///
/// `rust.gdextension` names the library as `res://../target/debug/`, so the
/// one file it needs is copied beside the project.
pub fn project_copy(scratch: &Path) -> PathBuf {
    let copy = scratch.join("project");
    std::fs::create_dir_all(&copy).expect("creating the project copy");
    for entry in std::fs::read_dir(project()).expect("reading the project") {
        let path = entry.expect("a project entry").path();
        if path.file_name().is_some_and(|name| name == CACHE) {
            continue;
        }
        let status = Command::new("cp")
            .arg("-R")
            .arg(&path)
            .arg(&copy)
            .status()
            .expect("copying a project entry");
        assert!(status.success(), "copying {} failed", path.display());
    }

    let name = format!(
        "{}render{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    );
    let debug = scratch.join("target/debug");
    std::fs::create_dir_all(&debug).expect("creating the library directory");
    std::fs::copy(root().join("target/debug").join(&name), debug.join(&name))
        .expect("copying the extension library");
    copy
}

/// Puts the probe scene in a project copy, and answers where it is.
///
/// It provokes, on demand, the two log lines the gate greps for besides
/// `ERROR:`. A copy only, because the game ships no script of its own and a
/// deliberately broken one has no business in the project.
pub fn probe_scene(project: &Path) -> &'static str {
    for (name, text) in [
        ("probe.gd", include_str!("fixtures/probe.gd")),
        ("probe.tscn", include_str!("fixtures/probe.tscn")),
    ] {
        std::fs::write(project.join(name), text)
            .unwrap_or_else(|error| panic!("writing {name}: {error}"));
    }
    "res://probe.tscn"
}

/// Every character manifest the project ships, read with `sprites::parse`,
/// which is the reader the game loads it with.
///
/// A directory under `assets/characters/` with no `character.ron` is a hole
/// rather than a character, so this refuses to be quiet about one.
pub fn manifests() -> Vec<(PathBuf, CharacterAssets)> {
    let characters = project().join("assets/characters");
    let mut found: Vec<PathBuf> = std::fs::read_dir(&characters)
        .expect("reading the characters directory")
        .map(|entry| entry.expect("a characters directory entry").path())
        .filter(|path| path.is_dir())
        .map(|path| path.join("character.ron"))
        .collect();
    found.sort();
    assert!(
        !found.is_empty(),
        "no character under {}",
        characters.display()
    );
    found
        .into_iter()
        .map(|path| {
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
            let assets =
                sprites::parse(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            (path, assets)
        })
        .collect()
}

/// The `res://` path the game reads a character's manifest through, which is
/// what its own log line names it by.
pub fn resource_path(manifest: &Path) -> String {
    let inside = manifest
        .strip_prefix(project())
        .expect("a manifest inside the project");
    format!("res://{}", inside.display())
}

#[test]
fn a_scratch_directory_does_not_outlive_the_test_that_asked_for_it() {
    let path = {
        let scratch = scratch("dropped");
        assert!(scratch.is_dir(), "{} was not created", scratch.display());
        scratch.to_path_buf()
    };
    assert!(!path.exists(), "{} outlived its test", path.display());
}
