//! `cargo art fetch`: what it decides to do, and what it does.
//!
//! The provider is served locally and Blender is a shell stub, so the whole
//! path runs with no network, no browser and no Blender.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use xtask_art::cli::{FetchStep, fetch, fetch_plan};
use xtask_art::library::{Animation, AnimationLibrary, HUMANOID, LibraryLock, MotionSource};
use xtask_art::providers::mixamo::client::CHARACTER_ID;

use crate::support::EnvGuard;

const PRODUCT: &str = "c9ccc468-b96c-11e4-a802-0aaa78deedf9";

/// The template library plus one clip that has to be fetched.
fn a_library() -> AnimationLibrary {
    let mut library = AnimationLibrary::template();
    library.animations.insert(
        "walk_back".to_owned(),
        Animation {
            skeleton: HUMANOID.to_owned(),
            loops: true,
            fps: 20,
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

fn wants(steps: &[FetchStep], name: &str) -> bool {
    steps
        .iter()
        .any(|step| matches!(step, FetchStep::Fetch { name: fetched, .. } if fetched == name))
}

fn on_disk(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries
        .iter()
        .map(|(name, digest)| ((*name).to_owned(), (*digest).to_owned()))
        .collect()
}

// --- Planning -------------------------------------------------------------

#[test]
fn with_no_names_everything_missing_is_fetched() {
    let steps = fetch_plan(
        &a_library(),
        &LibraryLock::default(),
        &[],
        &BTreeMap::new(),
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
            source: MotionSource::Authored,
        },
    );

    let steps = fetch_plan(
        &library,
        &LibraryLock::default(),
        &[],
        &BTreeMap::new(),
        false,
    )
    .unwrap();
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
    let mut lock = LibraryLock::default();
    lock.record(
        "walk_back",
        MotionSource::Mixamo {
            product_id: PRODUCT.to_owned(),
        },
        b"the download",
        b"the glb",
    );
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
        &BTreeMap::new(),
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

/// A repo with the retarget script, a virtualenv and a canonical rig.
fn a_repo(library: &AnimationLibrary) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("tools/blender/src")).unwrap();
    std::fs::write(root.join("tools/blender/src/retarget_animation.py"), "").unwrap();
    std::fs::create_dir_all(root.join(".venv/lib/python3.13/site-packages")).unwrap();

    let rig = AnimationLibrary::reference_rig(root, HUMANOID);
    std::fs::create_dir_all(rig.parent().unwrap()).unwrap();
    std::fs::write(rig, b"glTF").unwrap();
    library.save(root).unwrap();
    dir
}

/// A stub that writes the GLB the retarget would have written.
fn a_blender_stub(dir: &Path) -> std::path::PathBuf {
    let stub = dir.join("blender-stub.sh");
    std::fs::write(
        &stub,
        r#"#!/bin/sh
printf '%s\n' "$@" > "$MARROWFALL_STUB_ARGV"
out=""
while [ $# -gt 0 ]; do
  if [ "$1" = "--out" ]; then out="$2"; fi
  shift
done
mkdir -p "$(dirname "$out")"
printf 'glTF fitted' > "$out"
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
        );

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
    assert!(argv.contains("retarget_animation.py"), "got: {argv}");
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
    // Finishes cleanly, sentinel and all, but writes no clip.
    let stub = dir.path().join("silent-stub.sh");
    std::fs::write(&stub, "#!/bin/sh\n: > \"$MARROWFALL_SENTINEL\"\nexit 0\n").unwrap();
    std::fs::set_permissions(
        &stub,
        <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
    )
    .unwrap();
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_MIXAMO_TOKEN", "a-bearer-token")
        .set("MARROWFALL_MIXAMO_BASE_URL", &server.uri())
        .set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap());

    let error = fetch(dir.path(), &["walk_back".to_owned()], false)
        .await
        .unwrap_err()
        .to_string();

    assert!(error.contains("walk_back.glb"), "got: {error}");
    assert!(
        LibraryLock::load(dir.path()).unwrap().fetched.is_empty(),
        "nothing arrived, so nothing is recorded"
    );
}
