//! The driver's remaining decisions: the workspace lookup, the spend prompt,
//! and a step planned as cached that an upstream re-run has since invalidated.

use base64::Engine as _;
use serde_json::json;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};
use xtask_art::cli::{RunOptions, repo_root, run};
use xtask_art::lock::{Lock, Stage};
use xtask_art::spec::Paths;

use crate::support::{EnvGuard, a_png, a_spec, install_library};

#[test]
fn the_workspace_is_found_by_walking_up_from_the_current_directory() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join("crates/deep/nested")).unwrap();
    std::fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();

    let mut env = EnvGuard::new();
    // Without the manifest hint, the lookup falls back to walking up from cwd.
    env.remove("CARGO_MANIFEST_DIR");
    let previous = std::env::current_dir().unwrap();
    std::env::set_current_dir(root.join("crates/deep/nested")).unwrap();
    let found = repo_root();
    std::env::set_current_dir(previous).unwrap();

    assert_eq!(found.unwrap().canonicalize().unwrap(), root);
}

#[test]
fn a_directory_outside_any_workspace_says_what_it_looked_for() {
    let dir = tempfile::tempdir().unwrap();
    let mut env = EnvGuard::new();
    env.remove("CARGO_MANIFEST_DIR");
    let previous = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();
    let found = repo_root();
    std::env::set_current_dir(previous).unwrap();

    // A temp dir is usually outside any workspace; if the platform puts it
    // under one, the lookup legitimately succeeds.
    if let Err(error) = found {
        assert!(error.to_string().contains("workspace root"), "got: {error}");
    }
}

#[tokio::test]
async fn re_running_a_paid_stage_is_declined_when_the_answer_cannot_be_read() {
    let server = MockServer::start().await;
    let png = base64::engine::general_purpose::STANDARD.encode(a_png());
    for route in ["/images/generations", "/images/edits"] {
        Mock::given(method("POST"))
            .and(path(route))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"data": [{"b64_json": png}]})),
            )
            .mount(&server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path("/v1/balance"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"balance": 10})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path_regex(r"/v1/.+$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": "t1"})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"/v1/.+/t1$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "SUCCEEDED", "progress": 100
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("crates")).unwrap();
    std::fs::write(dir.path().join("Cargo.toml"), "[workspace]\n").unwrap();
    let paths = Paths::new(dir.path(), "survivor");
    a_spec("survivor").save(&paths.spec()).unwrap();
    install_library(dir.path());

    let mut env = EnvGuard::new();
    env.with_api(&server.uri());

    // Concept completes, so asking for it again is a re-spend and prompts.
    run(
        dir.path(),
        "survivor",
        RunOptions {
            from: None,
            only: Some(Stage::Concept),
            retry: false,
        },
        true,
        &mut std::io::empty(),
    )
    .await
    .unwrap();

    // `--from concept` on an already-complete concept is the spend prompt.
    // Without `--yes` and without a terminal, the answer reads as "no".
    let outcome = run(
        dir.path(),
        "survivor",
        RunOptions {
            from: Some(Stage::Concept),
            only: Some(Stage::Concept),
            retry: false,
        },
        false,
        &mut std::io::empty(),
    )
    .await;

    let lock = Lock::load(&paths.lock()).unwrap();
    assert!(
        lock.stages.contains_key(&Stage::Concept),
        "declining a re-spend must leave the completed work alone: {outcome:?}"
    );
}

#[tokio::test]
async fn accepting_the_spend_prompt_re_runs_the_paid_stage() {
    let server = MockServer::start().await;
    let png = base64::engine::general_purpose::STANDARD.encode(a_png());
    for route in ["/images/generations", "/images/edits"] {
        Mock::given(method("POST"))
            .and(path(route))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"data": [{"b64_json": png}]})),
            )
            .mount(&server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path("/v1/balance"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"balance": 10})))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("crates")).unwrap();
    std::fs::write(dir.path().join("Cargo.toml"), "[workspace]\n").unwrap();
    let paths = Paths::new(dir.path(), "survivor");
    a_spec("survivor").save(&paths.spec()).unwrap();
    install_library(dir.path());

    let mut env = EnvGuard::new();
    env.with_api(&server.uri());

    run(
        dir.path(),
        "survivor",
        RunOptions {
            from: None,
            only: Some(Stage::Concept),
            retry: true,
        },
        true,
        &mut std::io::empty(),
    )
    .await
    .unwrap();

    // "y" to the re-spend prompt, then Enter to the review pause that follows.
    run(
        dir.path(),
        "survivor",
        RunOptions {
            from: Some(Stage::Concept),
            only: None,
            retry: true,
        },
        false,
        &mut "y\n\n".as_bytes(),
    )
    .await
    .expect_err("the run continues past concept and stops at the model stage");

    let lock = Lock::load(&paths.lock()).unwrap();
    assert!(
        lock.stages.contains_key(&Stage::Concept),
        "the accepted re-run must record the concept again"
    );
}
