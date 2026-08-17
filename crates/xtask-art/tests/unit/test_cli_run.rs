//! A complete `cargo art run`, with both providers and Blender stubbed.
//!
//! This is the only place the whole pipeline executes end to end, so it is
//! what covers the driver's decisions: what is cached, what is skipped, what
//! re-prompts, and what happens when a paid stage fails partway.

use base64::Engine as _;
use serde_json::json;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};
use xtask_art::cli::{RunOptions, run};
use xtask_art::library::AnimationLibrary;
use xtask_art::lock::{Lock, Stage};
use xtask_art::spec::Paths;

use crate::support::{EnvGuard, a_library, a_png, a_spec, install_library};

fn options(from: Option<Stage>, only: Option<Stage>, retry: bool) -> RunOptions {
    RunOptions { from, only, retry }
}

/// A repo where every external call succeeds: both APIs, and Blender.
async fn a_working_repo(server: &MockServer) -> tempfile::TempDir {
    let png = base64::engine::general_purpose::STANDARD.encode(a_png());
    for route in ["/images/generations", "/images/edits"] {
        Mock::given(method("POST"))
            .and(path(route))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"data": [{"b64_json": png}]})),
            )
            .mount(server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path("/v1/balance"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"balance": 500})))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path_regex(r"/v1/(multi-image-to-3d|rigging|animations)$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": "t1"})))
        .mount(server)
        .await;
    let glb = format!("{}/files/x.glb", server.uri());
    Mock::given(method("GET"))
        .and(path_regex(
            r"/v1/(multi-image-to-3d|rigging|animations)/t1$",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "t1", "status": "SUCCEEDED", "progress": 100, "consumed_credits": 5,
            "model_urls": {"glb": glb},
            "result": {"rigged_character_glb_url": glb, "animation_glb_url": glb},
            "thumbnail_url": format!("{}/files/t.png", server.uri())
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"/files/.+$"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(a_png()))
        .mount(server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("crates")).unwrap();
    std::fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
    std::fs::create_dir_all(root.join(".venv/lib/python3.13/site-packages")).unwrap();
    std::fs::create_dir_all(root.join("tools/blender/src")).unwrap();
    std::fs::write(root.join("tools/blender/src/bake_sprites.py"), "").unwrap();
    a_spec("survivor")
        .save(&Paths::new(root, "survivor").spec())
        .unwrap();
    install_library(root);
    dir
}

/// A stub that writes the frames the packer expects, so the bake "succeeds".
fn install_blender_stub(root: &std::path::Path, env: &mut EnvGuard) {
    let stub = root.join("blender-stub.sh");
    std::fs::write(
        &stub,
        r#"#!/bin/sh
out=""
while [ $# -gt 0 ]; do
  if [ "$1" = "--out" ]; then out="$2"; fi
  shift
done
mkdir -p "$out"
for d in s se e ne n nw w sw; do
  for i in 00 01; do
    cp "$MARROWFALL_STUB_FRAME" "$out/idle_${d}_${i}.png"
  done
done
exit 0
"#,
    )
    .unwrap();
    std::fs::set_permissions(
        &stub,
        <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
    )
    .unwrap();

    // A frame with an opaque block inset from the border, so cropping finds
    // content and nothing is judged clipped.
    let mut frame = image::RgbaImage::new(64, 64);
    for y in 8..48 {
        for x in 24..40 {
            frame.put_pixel(x, y, image::Rgba([200, 180, 160, 255]));
        }
    }
    let frame_path = root.join("frame.png");
    frame.save(&frame_path).unwrap();

    env.set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap())
        .set("MARROWFALL_STUB_FRAME", frame_path.to_str().unwrap());
}

#[tokio::test]
async fn a_full_run_completes_every_stage_and_records_each_one() {
    let server = MockServer::start().await;
    let dir = a_working_repo(&server).await;
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());
    install_blender_stub(dir.path(), &mut env);

    run(
        dir.path(),
        "survivor",
        options(None, None, false),
        true,
        &mut std::io::empty(),
    )
    .await
    .unwrap();

    let paths = Paths::new(dir.path(), "survivor");
    let lock = Lock::load(&paths.lock()).unwrap();
    for stage in Stage::all() {
        assert!(lock.stages.contains_key(&stage), "{stage} was not recorded");
    }
    assert!(paths.assets().join("character.ron").exists());
    assert!(paths.assets().join("idle.png").exists());
}

#[tokio::test]
async fn a_second_run_reports_every_stage_as_cached_and_calls_nothing() {
    let server = MockServer::start().await;
    let dir = a_working_repo(&server).await;
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());
    install_blender_stub(dir.path(), &mut env);

    run(
        dir.path(),
        "survivor",
        options(None, None, false),
        true,
        &mut std::io::empty(),
    )
    .await
    .unwrap();
    // Point the providers at a dead host: a cached run must not reach them.
    env.set("MARROWFALL_MESHY_BASE_URL", "http://127.0.0.1:1")
        .set("MARROWFALL_OPENAI_BASE_URL", "http://127.0.0.1:1");

    run(
        dir.path(),
        "survivor",
        options(None, None, false),
        true,
        &mut std::io::empty(),
    )
    .await
    .expect("a fully cached run must not make a single request");
}

#[tokio::test]
async fn from_bake_reruns_the_free_tail_without_touching_the_paid_stages() {
    let server = MockServer::start().await;
    let dir = a_working_repo(&server).await;
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());
    install_blender_stub(dir.path(), &mut env);

    run(
        dir.path(),
        "survivor",
        options(None, None, false),
        true,
        &mut std::io::empty(),
    )
    .await
    .unwrap();
    env.set("MARROWFALL_MESHY_BASE_URL", "http://127.0.0.1:1")
        .set("MARROWFALL_OPENAI_BASE_URL", "http://127.0.0.1:1");

    run(
        dir.path(),
        "survivor",
        options(Some(Stage::Bake), None, false),
        true,
        &mut std::io::empty(),
    )
    .await
    .expect("re-running the bake must never re-spend credits");
}

#[tokio::test]
async fn editing_a_sprite_setting_does_not_invalidate_the_paid_stages() {
    let library = a_library();
    let server = MockServer::start().await;
    let dir = a_working_repo(&server).await;
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());
    install_blender_stub(dir.path(), &mut env);
    let paths = Paths::new(dir.path(), "survivor");

    run(
        dir.path(),
        "survivor",
        options(None, None, false),
        true,
        &mut std::io::empty(),
    )
    .await
    .unwrap();

    let mut spec = a_spec("survivor");
    spec.bake.sprite_height = 128;
    spec.save(&paths.spec()).unwrap();

    let lock = Lock::load(&paths.lock()).unwrap();
    assert!(
        lock.is_current(Stage::Concept, &spec, &library),
        "concept costs money"
    );
    assert!(
        lock.is_current(Stage::Model, &spec, &library),
        "model costs money"
    );
    assert!(
        lock.is_current(Stage::Rig, &spec, &library),
        "rigging costs money"
    );
    assert!(
        !lock.is_current(Stage::Pack, &spec, &library),
        "packing consumes sprite_height, so it must re-run"
    );
}

#[tokio::test]
async fn a_non_humanoid_skips_rigging_with_a_reason() {
    let server = MockServer::start().await;
    let dir = a_working_repo(&server).await;
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());
    install_blender_stub(dir.path(), &mut env);

    let paths = Paths::new(dir.path(), "hound");
    let mut spec = a_spec("hound");
    spec.subject.kind = xtask_art::spec::CharacterType::Quadruped;
    spec.animations.clear();
    spec.save(&paths.spec()).unwrap();

    // Rigging is skipped, so the bake has nothing to do and says so.
    let error = run(
        dir.path(),
        "hound",
        options(None, None, false),
        true,
        &mut std::io::empty(),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(error.contains("no animations to bake"), "got: {error}");

    let lock = Lock::load(&paths.lock()).unwrap();
    assert!(
        !lock.stages.contains_key(&Stage::Rig),
        "an unriggable body plan must not be charged for rigging"
    );
}

#[tokio::test]
async fn a_paid_stage_failing_partway_still_records_what_was_charged() {
    let server = MockServer::start().await;
    let dir = a_working_repo(&server).await;
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());
    let paths = Paths::new(dir.path(), "survivor");

    // Get as far as the model, then make animation submissions fail.
    run(
        dir.path(),
        "survivor",
        options(None, Some(Stage::Concept), false),
        true,
        &mut std::io::empty(),
    )
    .await
    .unwrap();
    run(
        dir.path(),
        "survivor",
        options(None, Some(Stage::Model), false),
        true,
        &mut std::io::empty(),
    )
    .await
    .unwrap();

    // The animation must actually be bought for this to be a partial failure,
    // so take it back out of the shared library first.
    std::fs::remove_file(AnimationLibrary::glb(dir.path(), "idle")).unwrap();

    server.reset().await;
    Mock::given(method("GET"))
        .and(path("/v1/balance"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"balance": 5})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/rigging"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": "r1"})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"/v1/rigging/r1$"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"status": "SUCCEEDED", "progress": 100})),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/animations"))
        .respond_with(ResponseTemplate::new(500).set_body_string("animation queue full"))
        .mount(&server)
        .await;

    let error = run(
        dir.path(),
        "survivor",
        options(None, Some(Stage::Rig), false),
        true,
        &mut std::io::empty(),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(error.contains("animating"), "got: {error}");

    let lock = Lock::load(&paths.lock()).unwrap();
    let rig_tasks = &lock.stages[&Stage::Rig].tasks;
    assert!(
        !rig_tasks.is_empty(),
        "the rig was paid for before the animation failed, so it must be kept"
    );
}
