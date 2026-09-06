//! The `pose_mode` spike's own machinery, with no network and no Blender.
//!
//! What the three paid runs cannot be tested on is what they measured; the
//! design's table holds that. What is testable is the shape of step 0's
//! request, that each mode keeps its own files and its own report name, and
//! that a spike run never touches the character's own art.

use serde_json::json;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};
use xtask_art::providers::meshy::{Client, Endpoint};
use xtask_art::spec::{Paths, PoseMode, View};
use xtask_art::spike;

use crate::support::{
    EnvGuard, a_bare_mesh, a_cleaned_mesh, a_concept_view, a_png, a_spec, install_skeleton,
};

async fn client(server: &MockServer) -> Client {
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());
    Client::from_env().expect("client from env")
}

/// One whole run, with the spend prompt answered the way `--yes` answers it.
/// One test below answers it by hand instead.
async fn spiked(root: &std::path::Path, rig: bool) -> anyhow::Result<()> {
    spike::run(root, "survivor", rig, true, &mut std::io::empty()).await
}

/// Every request one probe sent, in order.
async fn probed(server: &MockServer) -> Vec<serde_json::Value> {
    server
        .received_requests()
        .await
        .expect("the mock records every request")
        .iter()
        .map(Request::body_json::<serde_json::Value>)
        .map(|body| body.expect("a JSON body"))
        .collect()
}

// --- step 0, the free probe ------------------------------------------------

/// The probe reads the refusal rather than turning it into an error, because
/// the refusal is the measurement.
#[tokio::test]
async fn a_refused_request_comes_back_as_its_status_and_its_own_words() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(Endpoint::MultiImageTo3d.path()))
        .respond_with(
            ResponseTemplate::new(400).set_body_json(json!({"message": "PoseMode must be one of"})),
        )
        .mount(&server)
        .await;

    let (status, said) = client(&server)
        .await
        .probe(Endpoint::MultiImageTo3d, json!({}))
        .await
        .unwrap();

    assert_eq!(status, 400);
    assert!(said.contains("PoseMode must be one of"), "got: {said}");
}

/// Three requests: a baseline with the field absent, the value under
/// measurement, and a value nobody documents. Without the baseline an
/// identical refusal proves nothing, and without the undocumented value an
/// ignored field looks like an accepted one.
#[tokio::test]
async fn step_zero_sends_a_baseline_a_real_value_and_a_value_nobody_documents() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(Endpoint::MultiImageTo3d.path()))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({"message": "no"})))
        .mount(&server)
        .await;

    spike::probe_pose_mode(&client(&server).await)
        .await
        .unwrap();

    let sent = probed(&server).await;
    assert_eq!(sent.len(), 3, "{sent:#?}");
    assert!(sent[0].get("pose_mode").is_none(), "{}", sent[0]);
    assert_eq!(sent[1]["pose_mode"], PoseMode::APose.as_str());
    assert!(
        sent[2]["pose_mode"] != json!(PoseMode::APose.as_str())
            && sent[2]["pose_mode"] != json!(PoseMode::TPose.as_str()),
        "the third value has to be one the API does not document: {}",
        sent[2]
    );
}

/// Nothing here can become a task, so nothing here can be billed. Every
/// request carries an `image_urls` no fetcher can resolve.
#[tokio::test]
async fn step_zero_cannot_be_billed_because_no_request_carries_a_fetchable_image() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(Endpoint::MultiImageTo3d.path()))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({"message": "no"})))
        .mount(&server)
        .await;

    spike::probe_pose_mode(&client(&server).await)
        .await
        .unwrap();

    for body in probed(&server).await {
        let urls = body["image_urls"].as_array().expect("image_urls is a list");
        assert_eq!(urls.len(), 1, "{body}");
        let url = urls[0].as_str().unwrap();
        assert!(
            !url.starts_with("http") && !url.starts_with("data:"),
            "{url} is fetchable, so this request could be billed"
        );
    }
}

// --- one directory and one report name per mode ---------------------------

#[test]
fn each_mode_keeps_its_own_files_and_its_own_report_name() {
    let paths = Paths::new("/repo", "survivor");

    let named: Vec<(String, String)> = PoseMode::ALL
        .into_iter()
        .map(|mode| {
            let mine = spike::paths_for(&paths, mode);
            (mine.staging().display().to_string(), mine.item().to_owned())
        })
        .collect();

    assert_eq!(
        named,
        vec![
            (
                "/repo/art/staging/survivor/spike/unset".to_owned(),
                "survivor-unset".to_owned()
            ),
            (
                "/repo/art/staging/survivor/spike/a-pose".to_owned(),
                "survivor-a-pose".to_owned()
            ),
            (
                "/repo/art/staging/survivor/spike/t-pose".to_owned(),
                "survivor-t-pose".to_owned()
            ),
        ]
    );
}

/// The spike measures; T15 promotes. A run must not be able to write over the
/// character's own bare mesh, its rig or its concept views.
#[test]
fn a_spike_run_writes_nothing_the_pipeline_itself_owns() {
    let paths = Paths::new("/repo", "survivor");

    for mode in PoseMode::ALL {
        let mine = spike::paths_for(&paths, mode);
        assert_ne!(mine.bare_glb(), paths.bare_glb());
        assert_ne!(mine.clean_glb(), paths.clean_glb());
        assert_ne!(mine.item(), paths.item());
        // The concept views are the one input all three share, so they are
        // read from the character's own directory and never copied.
        assert_ne!(mine.preview(), paths.preview());
        assert_eq!(mine.concept(View::Front), paths.concept(View::Front));
        assert_eq!(mine.character_glb(), paths.character_glb());
    }
}

// --- the whole run, against a local server and a stub Blender -------------

/// A repository with the four concept views, the skeleton, and a Blender that
/// answers the fixer and the sheet without being Blender. The sheet gets
/// `--out-dir` and the fixer `--out`, so one stub reads which it was handed.
/// `fixed` is what the fixer writes: a whole figure, or its own input back,
/// which is the stub `mesh.cleanup_effective` rejects.
fn a_repository(env: &mut EnvGuard, fixed: Vec<u8>) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let paths = Paths::new(root, "survivor");
    let spec = a_spec("survivor");
    spec.save(&paths.spec()).unwrap();
    std::fs::create_dir_all(paths.concept(View::Front).parent().unwrap()).unwrap();
    for view in View::ALL {
        std::fs::write(paths.concept(view), a_concept_view()).unwrap();
    }
    install_skeleton(root);
    std::fs::create_dir_all(root.join("tools/blender/src")).unwrap();
    for script in ["mesh_clean.py", "mesh_sheet.py"] {
        std::fs::write(root.join("tools/blender/src").join(script), "").unwrap();
    }
    std::fs::create_dir_all(root.join(".venv/lib/python3.13/site-packages")).unwrap();

    let clean = root.join("clean-source.glb");
    std::fs::write(&clean, fixed).unwrap();
    let view = root.join("view-source.png");
    std::fs::write(&view, a_png()).unwrap();

    let stub = root.join("blender-stub.sh");
    std::fs::write(
        &stub,
        format!(
            r#"#!/bin/sh
out=""
views=""
while [ $# -gt 0 ]; do
  if [ "$1" = "--out" ]; then out="$2"; fi
  if [ "$1" = "--out-dir" ]; then views="$2"; fi
  shift
done
if [ -n "$out" ]; then cp {clean:?} "$out"; fi
if [ -n "$views" ]; then
  mkdir -p "$views"
  for name in front back left right forearms; do cp {view:?} "$views/$name.png"; done
fi
: > "$MARROWFALL_SENTINEL"
exit 0
"#,
            clean = clean.display(),
            view = view.display(),
        ),
    )
    .unwrap();
    std::fs::set_permissions(
        &stub,
        <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
    )
    .unwrap();
    env.set("MARROWFALL_BLENDER_BIN", stub.to_str().unwrap());
    dir
}

/// Serves the balance, the probe, and both paid endpoints.
async fn serve_meshy(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/v1/balance"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"balance": 500})))
        .mount(server)
        .await;
    for (endpoint, id) in [(Endpoint::MultiImageTo3d, "m1"), (Endpoint::Rigging, "r1")] {
        Mock::given(method("POST"))
            .and(path(endpoint.path()))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": id})))
            .mount(server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path_regex(r"^/v1/(multi-image-to-3d|rigging)/.+"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "SUCCEEDED",
            "progress": 100,
            "model_urls": {"glb": format!("{}/glb", server.uri())},
            "thumbnail_url": format!("{}/thumb.png", server.uri()),
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/glb"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(a_bare_mesh()))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/thumb.png"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(a_png()))
        .mount(server)
        .await;
}

/// Every mode measured, every report filed, every sheet drawn, and nothing
/// under `art/characters/` written.
#[tokio::test]
async fn a_whole_run_measures_three_modes_and_rigs_each_one() {
    let server = MockServer::start().await;
    serve_meshy(&server).await;
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());
    let dir = a_repository(&mut env, a_cleaned_mesh());
    let root = dir.path();

    spiked(root, true).await.unwrap();

    let paths = Paths::new(root, "survivor");
    for mode in PoseMode::ALL {
        let mine = spike::paths_for(&paths, mode);
        for file in ["bare.glb", "clean.glb", "rigged.glb", "sheet.png"] {
            let at = mine.staging().join(file);
            assert!(at.exists(), "{} was never written", at.display());
        }
        // The model stage's own preview of the thumbnails it fetched, which
        // is how this run is the stage rather than a copy of it.
        assert!(mine.preview().join("model.png").exists());
        for set in ["mesh", "cleaned", "cleanup", "rig"] {
            let report = root.join(format!("art/staging/reports/{set}.{}.1.json", mine.item()));
            assert!(report.exists(), "{} was never filed", report.display());
        }
    }
    assert!(
        !paths.bare_glb().exists(),
        "the character's own mesh was written"
    );
    assert!(
        !paths.preview().join("model.png").exists(),
        "the character's own preview was overwritten"
    );
}

/// Three generations cost 90 credits, so a second run buys nothing it already
/// has. Only the free half runs again.
#[tokio::test]
async fn a_second_run_measures_again_and_buys_nothing() {
    let server = MockServer::start().await;
    serve_meshy(&server).await;
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());
    let dir = a_repository(&mut env, a_cleaned_mesh());
    let root = dir.path();
    let posted = async || {
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| request.method == wiremock::http::Method::POST)
            .count()
    };
    spiked(root, true).await.unwrap();
    let first = posted().await;

    spiked(root, true).await.unwrap();

    // Step 0's three probes are POSTs and are free, so they are all a run
    // whose files are already on disk sends.
    assert_eq!(first, 3 + 3 + 3, "three probes, three models, three rigs");
    assert_eq!(posted().await - first, 3, "a second run re-spent");
}

/// Without `--rig` no skeleton is bought, so the free half can be re-run for
/// nothing while the numbers are argued over.
#[tokio::test]
async fn the_mesh_half_alone_buys_no_skeleton() {
    let server = MockServer::start().await;
    serve_meshy(&server).await;
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());
    let dir = a_repository(&mut env, a_cleaned_mesh());
    let root = dir.path();

    spiked(root, false).await.unwrap();

    let rigged = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|request| request.url.path() == Endpoint::Rigging.path())
        .count();
    assert_eq!(rigged, 0);
    let mine = spike::paths_for(&Paths::new(root, "survivor"), None);
    assert!(mine.staging().join("sheet.png").exists());
    assert!(!mine.staging().join("rigged.glb").exists());
}

// --- what it asks before it bills ----------------------------------------

/// It bills like every other paid verb, so it asks like one. What it quotes
/// is what it has left to buy: a mode already on disk is free.
#[test]
fn what_the_spike_still_owes_drops_as_its_files_land() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "survivor");

    assert_eq!(spike::outstanding(&paths, true), 105);
    assert_eq!(spike::outstanding(&paths, false), 90);

    let unset = spike::paths_for(&paths, None);
    std::fs::create_dir_all(unset.staging()).unwrap();
    std::fs::write(unset.bare_glb(), []).unwrap();
    assert_eq!(spike::outstanding(&paths, true), 75);
    std::fs::write(unset.staging().join("rigged.glb"), []).unwrap();
    assert_eq!(spike::outstanding(&paths, true), 70);
}

/// And an answer that is not yes spends nothing: no probe, no balance read,
/// no request of any kind.
#[tokio::test]
async fn a_declined_prompt_buys_nothing_and_generates_nothing() {
    let server = MockServer::start().await;
    serve_meshy(&server).await;
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());
    let dir = a_repository(&mut env, a_cleaned_mesh());
    let root = dir.path();

    spike::run(root, "survivor", true, false, &mut "n\n".as_bytes())
        .await
        .unwrap();

    assert!(server.received_requests().await.unwrap().is_empty());
    let mine = spike::paths_for(&Paths::new(root, "survivor"), None);
    assert!(!mine.bare_glb().exists());
}

/// A mode whose mesh fails a gate is still measured, still sheeted and still
/// rigged, and the run exits non-zero naming it. That is the one place this
/// command differs from the rig stage, and it differs because the limits are
/// what it is calibrating.
#[tokio::test]
async fn a_failing_mesh_is_measured_to_the_end_and_still_fails_the_run() {
    let server = MockServer::start().await;
    serve_meshy(&server).await;
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());
    // A fixer that writes its input back reads the count it was given, which
    // `mesh.cleanup_effective` is `lt` rather than `le` in order to reject.
    let dir = a_repository(&mut env, a_bare_mesh());
    let root = dir.path();

    let error = spiked(root, true).await.unwrap_err();

    let said = format!("{error:#}");
    for mode in PoseMode::ALL {
        assert!(said.contains(PoseMode::named(mode)), "{said}");
        let mine = spike::paths_for(&Paths::new(root, "survivor"), mode);
        assert!(mine.staging().join("sheet.png").exists());
        assert!(mine.staging().join("rigged.glb").exists());
    }
}
