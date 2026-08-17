//! The pipeline stages end to end, against a local server and a temp tree.
//!
//! These are the stages that spend money in production, so every path here is
//! exercised without a network: the providers are served locally and the
//! filesystem is a temporary directory.

use base64::Engine as _;
use serde_json::json;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};
use xtask_art::library::AnimationLibrary;
use xtask_art::lock::TaskRef;
use xtask_art::meshy::Endpoint;
use xtask_art::spec::View;
use xtask_art::spec::{CharacterType, Paths};
use xtask_art::stages;

use crate::support::{EnvGuard, a_library, a_png, a_spec};

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Serves both image endpoints with the same 2x2 PNG.
async fn serve_images(server: &MockServer) {
    let body = json!({"data": [{"b64_json": b64(&a_png())}]});
    for route in ["/images/generations", "/images/edits"] {
        Mock::given(method("POST"))
            .and(path(route))
            .respond_with(ResponseTemplate::new(200).set_body_json(body.clone()))
            .mount(server)
            .await;
    }
}

// --- concept --------------------------------------------------------------

#[tokio::test]
async fn concept_generates_every_view_and_writes_a_preview() {
    let server = MockServer::start().await;
    serve_images(&server).await;
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "survivor");
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());

    let record = stages::concept(&a_spec("survivor"), &paths, false)
        .await
        .unwrap();

    for view in View::ALL {
        assert!(paths.concept(view).exists(), "{view} was not written");
    }
    assert!(record.note.unwrap().contains("4 views"));
    assert!(paths.preview().join("concept.png").exists());
}

#[tokio::test]
async fn concept_reuses_views_already_on_disk() {
    let server = MockServer::start().await;
    serve_images(&server).await;
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "survivor");
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());
    let spec = a_spec("survivor");

    stages::concept(&spec, &paths, false).await.unwrap();
    let record = stages::concept(&spec, &paths, false).await.unwrap();

    assert!(
        record.note.unwrap().contains("0 newly generated"),
        "an existing view must not be paid for twice"
    );
}

#[tokio::test]
async fn retry_regenerates_views_that_already_exist() {
    let server = MockServer::start().await;
    serve_images(&server).await;
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "survivor");
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());
    let spec = a_spec("survivor");

    stages::concept(&spec, &paths, false).await.unwrap();
    let record = stages::concept(&spec, &paths, true).await.unwrap();

    assert!(record.note.unwrap().contains("3 newly generated"));
}

// --- model ----------------------------------------------------------------

#[tokio::test]
async fn model_uploads_every_concept_view_and_records_the_task() {
    let server = MockServer::start().await;
    serve_images(&server).await;
    Mock::given(method("POST"))
        .and(path(Endpoint::MultiImageTo3d.path()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": "m1"})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"/v1/multi-image-to-3d/m1$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "m1", "status": "SUCCEEDED", "progress": 100,
            "consumed_credits": 20,
            "model_urls": {"glb": "https://example.test/m.glb"},
            "thumbnail_url": "https://example.test/t.png"
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "survivor");
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());
    let spec = a_spec("survivor");
    stages::concept(&spec, &paths, false).await.unwrap();

    let record = stages::model(&spec, &paths).await.unwrap();

    assert_eq!(record.credits, Some(20));
    assert!(matches!(record.tasks.as_slice(), [TaskRef::Model { id }] if id == "m1"));
}

#[tokio::test]
async fn model_refuses_to_run_before_the_concepts_exist() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "survivor");
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());

    let error = stages::model(&a_spec("survivor"), &paths)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("run the concept stage first"),
        "got: {error}"
    );
}

// --- rig ------------------------------------------------------------------

#[tokio::test]
async fn rig_creates_a_rig_then_one_animation_per_entry() {
    let library = a_library();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(Endpoint::Rigging.path()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": "r1"})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(Endpoint::Animation.path()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": "a1"})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"/v1/(rigging|animations)/[ra]1$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "SUCCEEDED", "progress": 100
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let _paths = Paths::new(dir.path(), "survivor");
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());
    let mut persisted = Vec::new();
    let record = stages::rig(
        &a_spec("survivor"),
        &library,
        dir.path(),
        &[TaskRef::Model {
            id: "m1".to_owned(),
        }],
        |task| persisted.push(task),
    )
    .await
    .unwrap();

    assert_eq!(record.tasks.len(), 2, "one rig plus one animation");
    assert_eq!(
        persisted.len(),
        2,
        "each paid task is persisted as it completes, not at the end"
    );
}

#[tokio::test]
async fn rig_refuses_to_run_before_the_model_stage() {
    let library = a_library();
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let _paths = Paths::new(dir.path(), "survivor");
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());

    let error = stages::rig(&a_spec("survivor"), &library, dir.path(), &[], |_| {})
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("run the model stage first"), "got: {error}");
}

#[tokio::test]
async fn rig_reuses_a_recorded_task_when_the_inputs_still_match() {
    let library = a_library();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex(r"/v1/(rigging|animations)/.+$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "SUCCEEDED", "progress": 100
        })))
        .mount(&server)
        .await;

    let mut env = EnvGuard::new();
    env.with_api(&server.uri());
    let spec = a_spec("survivor");
    let already = vec![
        TaskRef::Model {
            id: "m1".to_owned(),
        },
        TaskRef::Rig {
            id: "r1".to_owned(),
            height_meters: spec.subject.height_meters,
        },
        TaskRef::Animation {
            id: "a1".to_owned(),
            name: "idle".to_owned(),
            action_id: 251,
        },
    ];

    let dir = tempfile::tempdir().unwrap();
    let _paths = Paths::new(dir.path(), "survivor");
    let mut persisted = Vec::new();
    let record = stages::rig(&spec, &library, dir.path(), &already, |task| {
        persisted.push(task)
    })
    .await
    .unwrap();

    assert!(
        persisted.is_empty(),
        "nothing new was submitted, so nothing was charged"
    );
    assert_eq!(record.tasks.len(), 2);
}

#[tokio::test]
async fn changing_the_height_forces_a_new_rig_rather_than_reusing_the_old_one() {
    let library = a_library();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(Endpoint::Rigging.path()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": "r2"})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(Endpoint::Animation.path()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": "a2"})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"/v1/(rigging|animations)/.+$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "SUCCEEDED", "progress": 100
        })))
        .mount(&server)
        .await;

    let mut env = EnvGuard::new();
    env.with_api(&server.uri());
    let mut spec = a_spec("survivor");
    spec.subject.height_meters = 2.1;
    let already = vec![
        TaskRef::Model {
            id: "m1".to_owned(),
        },
        // Recorded against the *old* height.
        TaskRef::Rig {
            id: "r1".to_owned(),
            height_meters: 1.7,
        },
    ];

    let dir = tempfile::tempdir().unwrap();
    let _paths = Paths::new(dir.path(), "survivor");
    let record = stages::rig(&spec, &library, dir.path(), &already, |_| {})
        .await
        .unwrap();
    let rig_id = record
        .tasks
        .iter()
        .find_map(|task| match task {
            TaskRef::Rig { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .unwrap();
    assert_eq!(rig_id, "r2", "the stale rig must not be reused");
}

// --- download -------------------------------------------------------------

#[tokio::test]
async fn download_writes_the_character_and_one_file_per_animation() {
    let server = MockServer::start().await;
    let rig_url = format!("{}/files/rig.glb", server.uri());
    let anim_url = format!("{}/files/anim.glb", server.uri());
    Mock::given(method("GET"))
        .and(path_regex(r"/v1/rigging/r1$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "SUCCEEDED",
            "result": {"rigged_character_glb_url": rig_url}
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"/v1/animations/a1$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "SUCCEEDED",
            "result": {"animation_glb_url": anim_url}
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"/files/.+$"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"glTF".to_vec()))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "survivor");
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());
    install_strip_stub(dir.path(), &mut env);

    let record = stages::download(
        &paths,
        dir.path(),
        &[
            TaskRef::Rig {
                id: "r1".to_owned(),
                height_meters: 1.7,
            },
            TaskRef::Animation {
                id: "a1".to_owned(),
                name: "idle".to_owned(),
                action_id: 251,
            },
        ],
    )
    .await
    .unwrap();

    assert!(paths.character_glb().exists());
    assert!(AnimationLibrary::glb(dir.path(), "idle").exists());
    assert!(record.note.unwrap().contains("2 GLB"));
}

#[tokio::test]
async fn a_rigged_character_supersedes_the_bare_mesh() {
    let server = MockServer::start().await;
    let rig_url = format!("{}/files/rig.glb", server.uri());
    Mock::given(method("GET"))
        .and(path_regex(r"/v1/rigging/r1$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "SUCCEEDED",
            "result": {"rigged_character_glb_url": rig_url}
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"/files/.+$"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"rigged".to_vec()))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "survivor");
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());
    install_strip_stub(dir.path(), &mut env);

    let record = stages::download(
        &paths,
        dir.path(),
        &[
            TaskRef::Model {
                id: "m1".to_owned(),
            },
            TaskRef::Rig {
                id: "r1".to_owned(),
                height_meters: 1.7,
            },
        ],
    )
    .await
    .unwrap();

    assert_eq!(
        std::fs::read(paths.character_glb()).unwrap(),
        b"rigged",
        "the unrigged mesh must not overwrite the rigged one"
    );
    assert!(record.note.unwrap().contains("1 GLB"));
}

#[tokio::test]
async fn download_with_nothing_recorded_says_which_stage_to_run() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "survivor");
    let server = MockServer::start().await;
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());

    let error = stages::download(&paths, dir.path(), &[])
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("model and rig stages"), "got: {error}");
}

#[tokio::test]
async fn a_task_that_exposes_no_glb_is_reported_with_its_status() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex(r"/v1/rigging/r1$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status": "IN_PROGRESS"})))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "survivor");
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());

    let error = stages::download(
        &paths,
        dir.path(),
        &[TaskRef::Rig {
            id: "r1".to_owned(),
            height_meters: 1.7,
        }],
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(error.contains("no GLB url"), "got: {error}");
}

// --- bake -----------------------------------------------------------------

#[tokio::test]
async fn bake_refuses_a_character_with_no_animations() {
    let library = a_library();
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "beast");
    let mut spec = a_spec("beast");
    spec.subject.kind = CharacterType::Quadruped;
    spec.animations.clear();

    // The script must exist, so the animation check is what fails.
    let script = dir.path().join("tools/blender/src");
    std::fs::create_dir_all(&script).unwrap();
    std::fs::write(script.join("bake_sprites.py"), "").unwrap();

    let error = stages::bake(&spec, &library, &paths, dir.path())
        .unwrap_err()
        .to_string();
    assert!(error.contains("no animations to bake"), "got: {error}");
    assert!(error.contains("Quadruped"), "got: {error}");
}

#[tokio::test]
async fn bake_says_to_download_first_when_the_character_glb_is_absent() {
    let library = a_library();
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "survivor");
    let script = dir.path().join("tools/blender/src");
    std::fs::create_dir_all(&script).unwrap();
    std::fs::write(script.join("bake_sprites.py"), "").unwrap();

    let error = stages::bake(&a_spec("survivor"), &library, &paths, dir.path())
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("run the download stage first"),
        "got: {error}"
    );
}

#[tokio::test]
async fn an_animation_already_in_the_library_is_not_bought_again() {
    let library = a_library();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(Endpoint::Rigging.path()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": "r1"})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"/v1/rigging/r1$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "SUCCEEDED", "progress": 100
        })))
        .mount(&server)
        .await;
    // No mock for POST /v1/animations at all: reaching it is the failure.

    let dir = tempfile::tempdir().unwrap();
    let _paths = Paths::new(dir.path(), "skeleton");
    let spec = a_spec("skeleton");
    let shared = AnimationLibrary::glb(dir.path(), &spec.animations[0]);
    std::fs::create_dir_all(shared.parent().unwrap()).unwrap();
    std::fs::write(&shared, b"glTF").unwrap();

    let mut env = EnvGuard::new();
    env.with_api(&server.uri());
    let mut persisted = Vec::new();
    let record = stages::rig(
        &spec,
        &library,
        dir.path(),
        &[TaskRef::Model {
            id: "m1".to_owned(),
        }],
        |task| persisted.push(task),
    )
    .await
    .unwrap();

    assert_eq!(
        record.tasks.len(),
        1,
        "only the rig was bought; the animation came free from the library"
    );
    assert!(
        persisted
            .iter()
            .all(|t| !matches!(t, TaskRef::Animation { .. })),
        "no animation was charged for"
    );
}

/// Downloading an animation strips it in Blender, so tests need a stand-in.
fn install_strip_stub(root: &std::path::Path, env: &mut EnvGuard) {
    let src = root.join("tools/blender/src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("strip_animation.py"), "").unwrap();
    std::fs::create_dir_all(root.join(".venv/lib/python3.13/site-packages")).unwrap();

    let stub = root.join("strip-stub.sh");
    std::fs::write(&stub, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(
        &stub,
        <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
    )
    .unwrap();
    env.set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap());
}

#[tokio::test]
async fn a_downloaded_animation_is_stripped_but_the_character_is_not() {
    let server = MockServer::start().await;
    let glb = format!("{}/files/x.glb", server.uri());
    Mock::given(method("GET"))
        .and(path_regex(r"/v1/(rigging|animations)/[ra]1$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "SUCCEEDED",
            "result": {"rigged_character_glb_url": glb, "animation_glb_url": glb}
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"/files/.+$"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"glTF".to_vec()))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "survivor");
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());

    // A stub that records every file it was asked to strip.
    let src = dir.path().join("tools/blender/src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("strip_animation.py"), "").unwrap();
    std::fs::create_dir_all(dir.path().join(".venv/lib/python3.13/site-packages")).unwrap();
    let log = dir.path().join("stripped.txt");
    let stub = dir.path().join("stub.sh");
    std::fs::write(
        &stub,
        "#!/bin/sh\nwhile [ $# -gt 0 ]; do\n  if [ \"$1\" = \"--glb\" ]; then \
         echo \"$2\" >> \"$MARROWFALL_STRIP_LOG\"; fi\n  shift\ndone\nexit 0\n",
    )
    .unwrap();
    std::fs::set_permissions(
        &stub,
        <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
    )
    .unwrap();
    env.set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap())
        .set("MARROWFALL_STRIP_LOG", log.to_str().unwrap());

    stages::download(
        &paths,
        dir.path(),
        &[
            TaskRef::Rig {
                id: "r1".to_owned(),
                height_meters: 1.7,
            },
            TaskRef::Animation {
                id: "a1".to_owned(),
                name: "idle".to_owned(),
                action_id: 251,
            },
        ],
    )
    .await
    .unwrap();

    let stripped = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        stripped.contains("animations/idle.glb"),
        "the animation must be stripped: {stripped:?}"
    );
    assert!(
        !stripped.contains("model.glb"),
        "the character keeps its mesh — it is what gets rendered: {stripped:?}"
    );
}
