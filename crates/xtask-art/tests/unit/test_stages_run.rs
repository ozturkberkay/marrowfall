//! The pipeline stages end to end, against a local server and a temp tree.
//!
//! These are the stages that spend money in production, so every path here is
//! exercised without a network: the providers are served locally and the
//! filesystem is a temporary directory.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde_json::json;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};
use xtask_art::check::{Artifacts, Report, Severity, mesh};
use xtask_art::cli::Asked;
use xtask_art::library::{Animation, AnimationLibrary, HUMANOID, MotionSource};
use xtask_art::lock::TaskRef;
use xtask_art::providers::meshy::{self, Endpoint};
use xtask_art::spec::View;
use xtask_art::spec::{CharacterType, Paths};
use xtask_art::stages;

use crate::stubs::a_blender_stub;
use crate::support::{
    EnvGuard, a_bare_mesh, a_cleaned_mesh, a_concept_view, a_library, a_png, a_rigged_character,
    a_spec, install_skeleton,
};

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

    let record = stages::concept(&a_spec("survivor"), &paths).await.unwrap();

    for view in View::ALL {
        assert!(paths.concept(view).exists(), "{view} was not written");
    }
    assert_eq!(record.note.unwrap(), "4 views generated");
    assert!(paths.preview().join("concept.png").exists());
}

/// The stage reuses nothing. A set already on disk is either one a previous
/// run left failing or one this run was asked to replace, so both attempt one
/// and every retry pay for four fresh views.
#[tokio::test]
async fn concept_regenerates_views_already_on_disk_rather_than_reusing_them() {
    let server = MockServer::start().await;
    serve_images(&server).await;
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "survivor");
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());
    let spec = a_spec("survivor");
    stages::concept(&spec, &paths).await.unwrap();
    // A different image, so a reused file is one this assertion can name. The
    // server serves `a_png` for every view.
    for view in View::ALL {
        std::fs::write(paths.concept(view), a_concept_view()).unwrap();
    }

    stages::concept(&spec, &paths).await.unwrap();

    for view in View::ALL {
        assert!(
            std::fs::read(paths.concept(view)).unwrap() == a_png(),
            "{view} was reused instead of regenerated"
        );
    }
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
            "model_urls": {"glb": format!("{}/files/m.glb", server.uri())},
            "thumbnail_url": format!("{}/files/t.png", server.uri())
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"/files/m\.glb$"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(a_bare_mesh()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"/files/t\.png$"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(a_png()))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "survivor");
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());
    let spec = a_spec("survivor");
    stages::concept(&spec, &paths).await.unwrap();

    let record = stages::model(&spec, &paths).await.unwrap();

    assert_eq!(record.credits, Some(20));
    assert!(matches!(record.tasks.as_slice(), [TaskRef::Model { id }] if id == "m1"));
    // The mesh gates and the fixer both run on this file, so the stage that
    // generated it is the stage that has to fetch it.
    assert_eq!(
        std::fs::read(paths.bare_glb()).unwrap(),
        a_bare_mesh(),
        "the bare mesh was not downloaded"
    );
    assert!(
        record
            .note
            .unwrap()
            .contains("art/staging/survivor/bare.glb")
    );
}

/// A finished task with no GLB behind it is a stage that would report success
/// having downloaded nothing.
#[tokio::test]
async fn model_refuses_a_finished_task_that_exposes_no_mesh() {
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
            "id": "m1", "status": "SUCCEEDED", "progress": 100
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "survivor");
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());
    let spec = a_spec("survivor");
    stages::concept(&spec, &paths).await.unwrap();

    let error = stages::model(&spec, &paths).await.unwrap_err().to_string();

    assert!(error.contains("exposes no GLB url"), "got: {error}");
    assert!(!paths.bare_glb().exists());
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
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());
    install_bare_mesh(dir.path(), "survivor", &mut env);
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
    // The spec's own height, because the mesh gates the rig stage runs first
    // read the mesh against it and this fixture is 1.70 m tall.
    let spec = a_spec("survivor");
    let already = vec![
        TaskRef::Model {
            id: "m1".to_owned(),
        },
        // Recorded against the *old* height.
        TaskRef::Rig {
            id: "r1".to_owned(),
            height_meters: 1.6,
        },
    ];

    let dir = tempfile::tempdir().unwrap();
    install_bare_mesh(dir.path(), "survivor", &mut env);
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

/// The one bought clip these tests fit.
const CLIP: &str = "walk_back";

/// A library of one Meshy clip, described the way the synthetic cross-rig
/// pair the Blender stub hands back really is: 8 frames at 24 fps off an open
/// curve, traveling.
fn a_bought_clip_library() -> AnimationLibrary {
    AnimationLibrary {
        animations: BTreeMap::from([(
            CLIP.to_owned(),
            Animation {
                skeleton: HUMANOID.to_owned(),
                loops: false,
                fps: 20,
                source_fps: 24,
                travels: true,
                source: MotionSource::Meshy { action_id: 544 },
            },
        )]),
    }
}

/// What Meshy delivers for an animation: the rig it sold, under its own bone
/// names, with the motion inside.
fn a_vendor_clip() -> Vec<u8> {
    a_vendor_rig()
}

/// The two tasks a rigged character with one bought clip leaves behind.
fn a_rig_and_a_clip() -> Vec<TaskRef> {
    vec![
        TaskRef::Rig {
            id: "r1".to_owned(),
            height_meters: 1.7,
        },
        TaskRef::Animation {
            id: "a1".to_owned(),
            name: CLIP.to_owned(),
            action_id: 544,
        },
    ]
}

/// Meshy answering for both tasks, and serving both files.
async fn serve_a_rig_and_a_clip(server: &MockServer, clip: &[u8]) {
    let rig_url = format!("{}/files/rig.glb", server.uri());
    let clip_url = format!("{}/files/clip.glb", server.uri());
    Mock::given(method("GET"))
        .and(path_regex(r"/v1/rigging/r1$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "SUCCEEDED",
            "result": {"rigged_character_glb_url": rig_url}
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"/v1/animations/a1$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "SUCCEEDED",
            "result": {"animation_glb_url": clip_url}
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"/files/rig\.glb$"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(a_rigged_character()))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"/files/clip\.glb$"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(clip.to_vec()))
        .mount(server)
        .await;
}

/// Everything the fit needs: the skeleton the rename and the conform read,
/// the canonical rig the clip gates measure a fit against, the two scripts,
/// and Blender as a stub. Returns where that stub logs its argv.
fn install_the_fit(root: &Path, env: &mut EnvGuard) -> PathBuf {
    install_skeleton(root);
    // The rig the stub's own pair was built on, in place of the committed
    // one: `clip.object_transform` reads the fit against this file.
    std::fs::write(
        AnimationLibrary::reference_rig(root, HUMANOID),
        crate::rigs::SyntheticRig::conformant().to_gltf(),
    )
    .unwrap();
    let src = root.join("tools/blender/src");
    std::fs::create_dir_all(&src).unwrap();
    for script in ["check_source.py", "retarget_animation.py"] {
        std::fs::write(src.join(script), "").unwrap();
    }
    std::fs::create_dir_all(root.join(".venv/lib/python3.13/site-packages")).unwrap();

    let stub = a_blender_stub(root);
    let argv = root.join("argv.txt");
    env.set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap())
        .set("MARROWFALL_STUB_ARGV", argv.to_str().unwrap());
    argv
}

#[tokio::test]
async fn download_writes_the_character_and_one_file_per_animation() {
    let server = MockServer::start().await;
    serve_a_rig_and_a_clip(&server, &a_vendor_clip()).await;

    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "survivor");
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());
    install_the_fit(dir.path(), &mut env);

    let record = stages::download(
        &a_spec("survivor"),
        &a_bought_clip_library(),
        &paths,
        dir.path(),
        &a_rig_and_a_clip(),
    )
    .await
    .unwrap();

    // The vendor's file in staging, and the conformed character beside it.
    assert!(paths.rigged_glb().exists());
    assert!(paths.character_glb().exists());
    assert!(
        AnimationLibrary::staged_download(dir.path(), CLIP, "glb").exists(),
        "the vendor's own clip is kept for a look"
    );
    assert!(a_bought_clip_library().glb(dir.path(), CLIP).exists());
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
        .respond_with(ResponseTemplate::new(200).set_body_bytes(a_rigged_character()))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "survivor");
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());
    // The rename and the conform read the skeleton the spec names, and no
    // clip means no Blender.
    install_skeleton(dir.path());

    let record = stages::download(
        &a_spec("survivor"),
        &a_library(),
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

    assert!(
        !paths.bare_glb().exists(),
        "the unrigged mesh must not be fetched over the rigged one"
    );
    assert!(paths.character_glb().exists());
    assert!(record.note.unwrap().contains("1 GLB"));
}

#[tokio::test]
async fn download_with_nothing_recorded_says_which_stage_to_run() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "survivor");
    let server = MockServer::start().await;
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());

    let error = stages::download(&a_spec("survivor"), &a_library(), &paths, dir.path(), &[])
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
        &a_spec("survivor"),
        &a_library(),
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
    let spec = a_spec("skeleton");
    let shared = a_library().glb(dir.path(), &spec.animations[0]);
    std::fs::create_dir_all(shared.parent().unwrap()).unwrap();
    std::fs::write(&shared, b"glTF").unwrap();

    let mut env = EnvGuard::new();
    env.with_api(&server.uri());
    install_bare_mesh(dir.path(), "skeleton", &mut env);
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

/// What the rig stage needs before it can spend anything: the mesh the model
/// stage downloaded, the profile its gates read, and a fixer that writes the
/// cleaned mesh Blender would have written.
fn install_bare_mesh(root: &std::path::Path, name: &str, env: &mut EnvGuard) {
    let cleaned = root.join("cleaned.glb");
    std::fs::write(&cleaned, a_cleaned_mesh()).unwrap();
    install_fixer(
        root,
        name,
        env,
        &format!("cp {:?} \"$out\"\n", cleaned.display()),
    );
}

/// The same, with the fixer's last act replaced. `$out` is whatever the
/// runner passed as `--out`, and the argv lands in `$MARROWFALL_STUB_ARGV`.
fn install_fixer(root: &std::path::Path, name: &str, env: &mut EnvGuard, last: &str) {
    let paths = Paths::new(root, name);
    std::fs::create_dir_all(paths.staging()).unwrap();
    std::fs::write(paths.bare_glb(), a_bare_mesh()).unwrap();
    install_skeleton(root);

    let src = root.join("tools/blender/src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("mesh_clean.py"), "").unwrap();
    std::fs::create_dir_all(root.join(".venv/lib/python3.13/site-packages")).unwrap();

    let stub = root.join("fixer-stub.sh");
    std::fs::write(
        &stub,
        format!(
            r#"#!/bin/sh
echo "$@" > "$MARROWFALL_STUB_ARGV"
out=""
while [ $# -gt 0 ]; do
  if [ "$1" = "--out" ]; then out="$2"; fi
  shift
done
{last}
: > "$MARROWFALL_SENTINEL"
exit 0
"#
        ),
    )
    .unwrap();
    std::fs::set_permissions(
        &stub,
        <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
    )
    .unwrap();
    env.set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap())
        .set("MARROWFALL_STUB_ARGV", argv_dump(root).to_str().unwrap());
}

/// Where the fixer stub writes the arguments it was given.
fn argv_dump(root: &std::path::Path) -> PathBuf {
    root.join("fixer-argv.txt")
}

// --- the fixer, between the mesh arriving and the credits being spent -----

/// Every number the fixer runs on is published to it, so no script here
/// holds a size of its own.
#[test]
fn the_fixer_is_handed_every_number_it_runs_on() {
    let dir = tempfile::tempdir().unwrap();
    let mut env = EnvGuard::new();
    install_bare_mesh(dir.path(), "survivor", &mut env);
    let paths = Paths::new(dir.path(), "survivor");

    let fed = stages::clean_mesh(&a_spec("survivor"), &paths, dir.path()).unwrap();

    assert_eq!(
        fed,
        paths.clean_glb(),
        "the mesh sent to rigging is what the fixer wrote"
    );
    let argv = std::fs::read_to_string(argv_dump(dir.path())).unwrap();
    for published in [
        "--meshes node 0,Mesh_0,char1",
        "--weld 0.00001",
        "--island-volume 0.000001",
        "--symmetry-threshold 0.001",
        "--symmetry true",
    ] {
        assert!(argv.contains(published), "{published} is not in {argv}");
    }
}

/// And every mesh rule reports there, so the step cannot go quiet on one.
/// The file rules read **both** meshes, the cleaned one less the two it
/// cannot answer: the file sent to rigging has to have passed them too.
#[test]
fn the_fixer_step_reports_every_file_rule_on_both_meshes() {
    let dir = tempfile::tempdir().unwrap();
    let mut env = EnvGuard::new();
    install_bare_mesh(dir.path(), "survivor", &mut env);
    let paths = Paths::new(dir.path(), "survivor");

    stages::clean_mesh(&a_spec("survivor"), &paths, dir.path()).unwrap();

    // One report per file, because four of the thirteen name the object they
    // read and both files hold an object called `char1`. The hole count is
    // the discriminator: the fixture the fixer wrote is whole.
    for (stage, file, holes, rules) in [
        (mesh::STAGE, paths.bare_glb(), 3.0, &mesh::FILE_RULES[..]),
        (
            mesh::CLEANED_STAGE,
            paths.clean_glb(),
            0.0,
            &mesh::CLEANED_RULES[..],
        ),
    ] {
        let report = a_report(dir.path(), stage);
        for rule in rules {
            assert!(
                report
                    .findings()
                    .iter()
                    .any(|finding| finding.rule == rule.id),
                "{} reported nothing under {stage}",
                rule.id
            );
        }
        assert_eq!(report.findings().len(), rules.len(), "{stage}");
        let boundary = report
            .findings()
            .iter()
            .find(|finding| finding.rule == "mesh.holes")
            .expect("the hole count");
        assert_eq!(
            (boundary.subject.clone(), boundary.measured),
            (paths.relative(&file), holes)
        );
    }

    let pair = a_report(dir.path(), mesh::CLEANUP_STAGE);
    for rule in mesh::CLEANUP_RULES {
        assert!(
            pair.findings()
                .iter()
                .any(|finding| finding.rule == rule.id),
            "{} reported nothing at all",
            rule.id
        );
    }
    assert_eq!(pair.findings().len(), 2);
}

/// Two producers write these reports: the fixer step inside `rig`, and
/// `cargo art check` by hand. Each stage name has to carry the same rule set
/// from both, or whichever ran last overwrites the other's report with a
/// different shape under the same path.
#[test]
fn checking_by_hand_writes_the_same_reports_the_fixer_step_did() {
    let dir = tempfile::tempdir().unwrap();
    let mut env = EnvGuard::new();
    install_bare_mesh(dir.path(), "survivor", &mut env);
    let paths = Paths::new(dir.path(), "survivor");
    let spec = a_spec("survivor");
    spec.save(&paths.spec()).unwrap();
    let reports = [mesh::STAGE, mesh::CLEANED_STAGE, mesh::CLEANUP_STAGE];
    stages::clean_mesh(&spec, &paths, dir.path()).unwrap();
    let by_the_step = reports.map(|stage| rules_of(dir.path(), stage));
    // Removed, so a producer that writes nothing reads as a missing report
    // rather than as the other producer's file.
    std::fs::remove_dir_all(
        Artifacts::new(dir.path(), mesh::STAGE, "survivor", 1)
            .unwrap()
            .dir(),
    )
    .unwrap();

    xtask_art::cli::check(dir.path(), None, Asked::Measure).unwrap();

    assert_eq!(
        reports.map(|stage| rules_of(dir.path(), stage)),
        by_the_step
    );
}

/// `symmetry: false` reaches the fixer as well as the mirror rules, because
/// mirroring a character that is asymmetric on purpose is the damage.
#[test]
fn a_spec_that_declines_symmetry_asks_the_fixer_not_to_mirror() {
    let dir = tempfile::tempdir().unwrap();
    let mut env = EnvGuard::new();
    install_bare_mesh(dir.path(), "survivor", &mut env);
    let mut spec = a_spec("survivor");
    spec.subject.symmetry = false;

    stages::clean_mesh(&spec, &Paths::new(dir.path(), "survivor"), dir.path()).unwrap();

    let argv = std::fs::read_to_string(argv_dump(dir.path())).unwrap();
    assert!(argv.contains("--symmetry false"), "got {argv}");
    for stage in [mesh::STAGE, mesh::CLEANED_STAGE] {
        let report = a_report(dir.path(), stage);
        let mirror = report
            .findings()
            .iter()
            .find(|finding| finding.rule == "mesh.mirror")
            .expect("the mirror rule still reports");
        assert_eq!(mirror.severity, Severity::Skipped, "{stage}: {mirror:#?}");
    }
}

/// `cleanup: false` runs no Blender at all, and the mesh sent to rigging is
/// the one that arrived.
#[test]
fn a_spec_that_declines_the_cleanup_runs_no_fixer() {
    let dir = tempfile::tempdir().unwrap();
    let mut env = EnvGuard::new();
    install_bare_mesh(dir.path(), "survivor", &mut env);
    let mut spec = a_spec("survivor");
    spec.subject.cleanup = false;
    let paths = Paths::new(dir.path(), "survivor");

    let fed = stages::clean_mesh(&spec, &paths, dir.path()).unwrap();

    assert_eq!(fed, paths.bare_glb());
    assert!(!paths.clean_glb().exists(), "nothing was written");
    assert!(
        !argv_dump(dir.path()).exists(),
        "Blender was invoked for a character that asked for no fixer"
    );
    for rule in ["mesh.non_manifold_post", "mesh.cleanup_effective"] {
        let finding = a_report(dir.path(), mesh::CLEANUP_STAGE)
            .findings()
            .iter()
            .find(|finding| finding.rule == rule)
            .unwrap_or_else(|| panic!("{rule} reported nothing"))
            .clone();
        assert_eq!(finding.severity, Severity::Skipped, "{finding:#?}");
    }
}

/// The gate is before the money: a mesh that fails a pre-cleanup rule stops
/// the stage, and the report says which rule.
#[test]
fn a_mesh_that_fails_a_gate_stops_the_stage_before_rigging() {
    let dir = tempfile::tempdir().unwrap();
    let mut env = EnvGuard::new();
    install_bare_mesh(dir.path(), "survivor", &mut env);
    let paths = Paths::new(dir.path(), "survivor");
    // A second object the profile does not name, which is debris the rigger
    // would be paid to skin.
    std::fs::write(
        paths.bare_glb(),
        crate::meshes::SyntheticMesh::figure()
            .plus_an_object("Icosphere")
            .to_glb(),
    )
    .unwrap();

    let error = stages::clean_mesh(&a_spec("survivor"), &paths, dir.path())
        .unwrap_err()
        .to_string();

    assert!(error.contains("mesh.stray_object"), "got: {error}");
    assert!(error.contains("not worth its credits"), "got: {error}");
}

/// A fixer that finished and wrote nothing is a fixer nothing can measure.
#[test]
fn a_fixer_that_wrote_no_mesh_stops_the_stage() {
    let dir = tempfile::tempdir().unwrap();
    let mut env = EnvGuard::new();
    install_fixer(dir.path(), "survivor", &mut env, "");

    let error = stages::clean_mesh(
        &a_spec("survivor"),
        &Paths::new(dir.path(), "survivor"),
        dir.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(
        error.contains("wrote no art/staging/survivor/clean.glb"),
        "got: {error}"
    );
}

/// A fixer that wrote a file nothing can read: the error a human needs is
/// the defect and the report it is written in, not the ten rules that had
/// nothing left to read.
#[test]
fn a_fixer_that_wrote_an_unreadable_mesh_says_so_rather_than_naming_every_rule() {
    let dir = tempfile::tempdir().unwrap();
    let mut env = EnvGuard::new();
    install_fixer(
        dir.path(),
        "survivor",
        &mut env,
        "echo not-a-glb > \"$out\"\n",
    );

    let error = stages::clean_mesh(
        &a_spec("survivor"),
        &Paths::new(dir.path(), "survivor"),
        dir.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("1 defect(s) on mesh.holes"), "got: {error}");
    assert!(error.contains("cleaned.survivor.1.json"), "got: {error}");
    assert!(!error.contains("never read"), "got: {error}");
}

/// And a fixer that measured something has taken over a job that is not
/// its: rule three of the design.
#[test]
fn a_fixer_that_reported_a_finding_stops_the_stage() {
    let dir = tempfile::tempdir().unwrap();
    let mut env = EnvGuard::new();
    install_fixer(
        dir.path(),
        "survivor",
        &mut env,
        "printf '{\"stage\":\"cleanup\",\"item\":\"survivor\",\"attempt\":1,\"findings\":[]}' \
         > \"$MARROWFALL_REPORT\"\n",
    );

    let error = stages::clean_mesh(
        &a_spec("survivor"),
        &Paths::new(dir.path(), "survivor"),
        dir.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("the fixer measures nothing"), "got: {error}");
}

/// Rigging is sent the mesh itself, and never a task id: `input_task_id`
/// wins if both are sent, so the cleanup would be thrown away.
#[tokio::test]
async fn rigging_sends_the_cleaned_mesh_as_a_data_uri() {
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

    let dir = tempfile::tempdir().unwrap();
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());
    install_bare_mesh(dir.path(), "survivor", &mut env);
    let mut spec = a_spec("survivor");
    spec.animations.clear();

    stages::rig(
        &spec,
        &a_library(),
        dir.path(),
        &[TaskRef::Model {
            id: "m1".to_owned(),
        }],
        |_| {},
    )
    .await
    .unwrap();

    let sent: serde_json::Value = server
        .received_requests()
        .await
        .expect("the requests are recorded")
        .iter()
        .find(|request| request.url.path() == Endpoint::Rigging.path())
        .expect("one rigging request")
        .body_json()
        .unwrap();
    let cleaned = std::fs::read(Paths::new(dir.path(), "survivor").clean_glb()).unwrap();
    assert_eq!(sent["model_url"], meshy::to_model_uri(&cleaned));
    assert!(sent.get("input_task_id").is_none(), "{sent}");
}

/// One report the fixer step writes, for a test to read a finding out of.
fn a_report(root: &std::path::Path, stage: &str) -> Report {
    Report::read(&Artifacts::new(root, stage, "survivor", 1).unwrap().report())
        .unwrap_or_else(|error| panic!("reading the {stage} report: {error:#}"))
}

/// Every rule one report carries, once per subject it reported on.
fn rules_of(root: &std::path::Path, stage: &str) -> Vec<String> {
    let mut rules: Vec<String> = a_report(root, stage)
        .findings()
        .iter()
        .map(|finding| format!("{} on {}", finding.rule, finding.subject))
        .collect();
    rules.sort();
    rules
}

// --- retarget -------------------------------------------------------------

/// A clip from a provider that names its bones its own way.
fn a_mixamo_clip() -> Animation {
    Animation {
        skeleton: HUMANOID.to_owned(),
        loops: true,
        fps: 20,
        source_fps: 30,
        travels: true,
        source: MotionSource::Mixamo {
            product_id: "c9ccc468-b96c-11e4-a802-0aaa78deedf9".to_owned(),
        },
    }
}

#[test]
fn the_retarget_needs_its_script() {
    let dir = tempfile::tempdir().unwrap();
    let error = stages::retarget(
        &dir.path().join("walk_back.fbx"),
        &dir.path().join("walk_back.glb"),
        "walk_back",
        &a_mixamo_clip(),
        dir.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("retarget_animation.py"), "got: {error}");
}

/// The canonical rig is what a clip is fitted to, so its absence is the one
/// failure that cannot be worked around by re-fetching.
#[test]
fn the_retarget_needs_the_canonical_rig_and_says_where_it_comes_from() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("tools/blender/src")).unwrap();
    std::fs::write(
        dir.path().join("tools/blender/src/retarget_animation.py"),
        "",
    )
    .unwrap();

    let error = stages::retarget(
        &dir.path().join("walk_back.fbx"),
        &dir.path().join("walk_back.glb"),
        "walk_back",
        &a_mixamo_clip(),
        dir.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("humanoid"), "names the skeleton: {error}");
    assert!(error.contains("art/skeletons/README.md"), "got: {error}");
}

/// A bought clip is a source, not the library's file: the vendor animates the
/// rig it sold, so the download carries the vendor's names and the vendor's
/// rest pose. It goes through the source check and the retarget a Mixamo clip
/// goes through, and what lands in `art/animations/` is the fit.
#[tokio::test]
async fn a_bought_clip_is_fitted_onto_the_canonical_rig_and_the_character_is_not() {
    let server = MockServer::start().await;
    let downloaded = a_vendor_clip();
    serve_a_rig_and_a_clip(&server, &downloaded).await;

    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "survivor");
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());
    let argv = install_the_fit(dir.path(), &mut env);

    stages::download(
        &a_spec("survivor"),
        &a_bought_clip_library(),
        &paths,
        dir.path(),
        &a_rig_and_a_clip(),
    )
    .await
    .unwrap();

    // Counted, never printed: two GLBs are a megabyte of numbers nobody reads.
    let fitted = std::fs::read(a_bought_clip_library().glb(dir.path(), CLIP)).unwrap_or_default();
    assert!(
        fitted != downloaded,
        "the library file is the retarget's output, not the download, and both \
         are {} bytes",
        fitted.len()
    );
    let staged = std::fs::read(AnimationLibrary::staged_download(dir.path(), CLIP, "glb")).unwrap();
    assert!(
        staged == downloaded,
        "the vendor's own bytes are kept beside the fit"
    );

    let argv = std::fs::read_to_string(&argv).unwrap_or_default();
    // Measured as it arrived, then fitted, in that order.
    assert!(
        argv.find("check_source.py") < argv.find("retarget_animation.py"),
        "got: {argv}"
    );
    // In the convention the vendor ships, onto the conformed canonical rig.
    assert_eq!(
        argv.matches("--convention\nmeshy").count(),
        2,
        "got: {argv}"
    );
    let rig = AnimationLibrary::reference_rig(dir.path(), HUMANOID);
    assert!(
        argv.contains(&format!("--rig\n{}", rig.display())),
        "got: {argv}"
    );
    // Neither script is ever handed the character: it keeps its mesh, which
    // is what gets rendered.
    assert!(!argv.contains("model.glb"), "got: {argv}");
}

/// The vendor's own bytes are kept, so re-running the stage after a local fix
/// asks the provider for nothing. A Meshy task URL expires, and the rename,
/// the conform and the fit are all offline work on files already here.
#[tokio::test]
async fn a_download_already_on_disk_is_never_asked_for_again() {
    // A server with no route at all: reaching it is the failure.
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "survivor");
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());
    install_the_fit(dir.path(), &mut env);

    std::fs::create_dir_all(paths.staging()).unwrap();
    std::fs::write(paths.rigged_glb(), a_rigged_character()).unwrap();
    let staged = AnimationLibrary::staged_download(dir.path(), CLIP, "glb");
    std::fs::create_dir_all(staged.parent().unwrap()).unwrap();
    std::fs::write(&staged, a_vendor_clip()).unwrap();

    stages::download(
        &a_spec("survivor"),
        &a_bought_clip_library(),
        &paths,
        dir.path(),
        &a_rig_and_a_clip(),
    )
    .await
    .unwrap();

    assert!(paths.character_glb().exists(), "the conform still ran");
    assert!(a_bought_clip_library().glb(dir.path(), CLIP).exists());
}

// --- the rename and the conform, between the download and model.glb -------

/// A rigged file as a vendor returns one: its own spine numbering, a `Hips`
/// turned off the direction to its child, and one leg longer than the other.
/// Every one of those is measured on the three rigs T12 generated.
fn a_vendor_rig() -> Vec<u8> {
    let rig = crate::rigs::SyntheticRig::conformant()
        .renamed("Spine", "Spine02")
        .renamed("Spine1", "Spine01")
        .renamed("Spine2", "Spine")
        .renamed("Neck", "neck")
        .nudged("head_end", glam::DVec3::new(0.05, 0.0, 0.0))
        .moved("LeftLeg", glam::DVec3::new(0.0, 0.02, 0.0));
    rig.to_glb(&rig.bind_pose_clip())
}

/// A repo holding one rigged file and the skeleton the step reads.
fn a_repo_with_a_rigged_file(rigged: &[u8]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    install_skeleton(dir.path());
    let paths = Paths::new(dir.path(), "survivor");
    std::fs::create_dir_all(paths.staging()).unwrap();
    std::fs::write(paths.rigged_glb(), rigged).unwrap();
    dir
}

fn report_of(root: &std::path::Path, stage: &str) -> xtask_art::check::Report {
    xtask_art::check::Report::read(
        &root.join(format!("art/staging/reports/{stage}.survivor.1.json")),
    )
    .unwrap()
}

fn broken(report: &xtask_art::check::Report) -> std::collections::BTreeSet<&str> {
    report
        .findings()
        .iter()
        .filter(|finding| finding.severity == xtask_art::check::Severity::Error)
        .map(|finding| finding.rule.as_str())
        .collect()
}

/// The `rig` report records what arrived and refuses nothing, because a
/// bought rig fails the three name rules by construction and the rename is
/// what closes them. The `conformed` report is the gate.
#[test]
fn the_rig_report_records_the_vendor_and_the_conformed_one_gates() {
    let dir = a_repo_with_a_rigged_file(&a_vendor_rig());
    let paths = Paths::new(dir.path(), "survivor");

    stages::conform_rig(&a_spec("survivor"), &paths, dir.path()).unwrap();

    let vendor = report_of(dir.path(), "rig");
    let before = broken(&vendor);
    for rule in [
        "rig.names_standard",
        "rig.bone_set",
        "rig.child_axis",
        "rig.mirror_length",
    ] {
        assert!(before.contains(rule), "{rule} holds on the vendor's file");
    }
    let after = report_of(dir.path(), "conformed");
    assert!(
        broken(&after).is_empty(),
        "the conformed rig still breaks {:?}",
        broken(&after)
    );

    // And the file downstream reads is the conformed one, under our names.
    let names = xtask_art::check::gltf_world::Skeleton::read(&paths.character_glb()).unwrap();
    assert!(names.get("Spine1").is_some() && names.get("Spine01").is_none());
}

/// The defect no rest-frame edit can close: the humerus is the direction from
/// the shoulder joint to the elbow, which is where the mesh's arm is.
#[test]
fn a_rig_the_conform_cannot_close_stops_the_step_and_names_the_rule() {
    let rig = crate::rigs::SyntheticRig::conformant()
        .moved("LeftForeArm", glam::DVec3::new(0.0, -0.2, 0.0))
        .moved("RightForeArm", glam::DVec3::new(0.0, -0.2, 0.0));
    let dir = a_repo_with_a_rigged_file(&rig.to_glb(&rig.bind_pose_clip()));
    let paths = Paths::new(dir.path(), "survivor");

    let error = stages::conform_rig(&a_spec("survivor"), &paths, dir.path())
        .unwrap_err()
        .to_string();

    assert!(error.contains("rig.humerus_angle"), "got: {error}");
    assert!(error.contains("conformed.survivor.1.json"), "got: {error}");
}

/// A rig nothing fingerprints is refused rather than renamed by guesswork: a
/// wrong guess would put a bone under the wrong role and no gate could tell.
#[test]
fn a_rig_no_convention_fingerprints_stops_before_anything_is_written() {
    let rig = crate::rigs::SyntheticRig::conformant().renamed("headfront", "face");
    let dir = a_repo_with_a_rigged_file(&rig.to_glb(&rig.bind_pose_clip()));
    let paths = Paths::new(dir.path(), "survivor");

    let error = format!(
        "{:#}",
        stages::conform_rig(&a_spec("survivor"), &paths, dir.path()).unwrap_err()
    );

    assert!(error.contains("no convention fingerprints"), "got: {error}");
    assert!(!paths.character_glb().exists());
}

/// And a rig the rename leaves under two of one name stops before the
/// geometry is touched, because every profile row below reads by bone name.
#[test]
fn a_rename_that_leaves_a_duplicate_stops_before_the_conform() {
    let rig = crate::rigs::SyntheticRig::conformant().renamed("LeftHand", "LeftFoot");
    let dir = a_repo_with_a_rigged_file(&rig.to_glb(&rig.bind_pose_clip()));
    let paths = Paths::new(dir.path(), "survivor");

    let error = format!(
        "{:#}",
        stages::conform_rig(&a_spec("survivor"), &paths, dir.path()).unwrap_err()
    );

    assert!(error.contains("rig.bone_set"), "got: {error}");
    assert!(!paths.character_glb().exists());
}
