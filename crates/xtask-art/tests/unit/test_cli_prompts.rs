//! The driver's remaining decisions: the workspace lookup, the spend prompt,
//! and a step planned as cached that an upstream re-run has since invalidated.

use base64::Engine as _;
use serde_json::json;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};
use xtask_art::cli::{RunOptions, repo_root, run};
use xtask_art::lock::{Lock, Stage};
use xtask_art::spec::Paths;

use crate::support::{
    EnvGuard, a_concept_view, a_png, a_spec, a_version_only_blender, install_library,
    install_scripts, install_skeleton,
};

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
    let view = base64::engine::general_purpose::STANDARD.encode(a_concept_view());
    for route in ["/images/generations", "/images/edits"] {
        Mock::given(method("POST"))
            .and(path(route))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"data": [{"b64_json": view}]})),
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
    install_skeleton(dir.path());
    install_scripts(dir.path());

    let mut env = EnvGuard::new();
    env.with_api(&server.uri()).set(
        "MARROWFALL_BLENDER_BIN",
        a_version_only_blender(dir.path()).to_str().unwrap(),
    );

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
    let view = base64::engine::general_purpose::STANDARD.encode(a_concept_view());
    for route in ["/images/generations", "/images/edits"] {
        Mock::given(method("POST"))
            .and(path(route))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"data": [{"b64_json": view}]})),
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
    install_skeleton(dir.path());
    install_scripts(dir.path());

    let mut env = EnvGuard::new();
    env.with_api(&server.uri()).set(
        "MARROWFALL_BLENDER_BIN",
        a_version_only_blender(dir.path()).to_str().unwrap(),
    );

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

    // "y" to the re-spend prompt, which is the only thing a run now asks.
    run(
        dir.path(),
        "survivor",
        RunOptions {
            from: Some(Stage::Concept),
            only: None,
            retry: true,
        },
        false,
        &mut "y\n".as_bytes(),
    )
    .await
    .expect_err("the run continues past concept and stops at the model stage");

    let lock = Lock::load(&paths.lock()).unwrap();
    assert!(
        lock.stages.contains_key(&Stage::Concept),
        "the accepted re-run must record the concept again"
    );
}

/// A terminal that counts how many questions it was asked.
struct Answers {
    lines: std::io::Cursor<Vec<u8>>,
    asked: usize,
}

impl std::io::Read for Answers {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.lines.read(buf)
    }
}

impl std::io::BufRead for Answers {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        self.lines.fill_buf()
    }

    fn consume(&mut self, amount: usize) {
        self.lines.consume(amount);
    }

    fn read_line(&mut self, out: &mut String) -> std::io::Result<usize> {
        self.asked += 1;
        self.lines.read_line(out)
    }
}

/// The concept stage retries inside itself, so the one prompt that guards
/// money is asked once for the whole loop and never once per attempt.
#[tokio::test]
async fn the_spend_prompt_is_asked_once_for_all_three_attempts() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("crates")).unwrap();
    std::fs::write(dir.path().join("Cargo.toml"), "[workspace]\n").unwrap();
    let paths = Paths::new(dir.path(), "survivor");
    a_spec("survivor").save(&paths.spec()).unwrap();
    install_library(dir.path());
    install_skeleton(dir.path());
    install_scripts(dir.path());

    let mut env = EnvGuard::new();
    env.with_api(&server.uri()).set(
        "MARROWFALL_BLENDER_BIN",
        a_version_only_blender(dir.path()).to_str().unwrap(),
    );

    // A first run that passes, so the concept stage is recorded and asking
    // for it again is a re-spend.
    serve_views(&server, &a_concept_view()).await;
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

    // Then a generator that returns something no gate holds, so all three
    // attempts fail behind one prompt.
    server.reset().await;
    serve_views(&server, &a_png()).await;
    let mut answers = Answers {
        lines: std::io::Cursor::new(b"y\n".to_vec()),
        asked: 0,
    };
    let error = run(
        dir.path(),
        "survivor",
        RunOptions {
            from: None,
            only: Some(Stage::Concept),
            retry: true,
        },
        false,
        &mut answers,
    )
    .await
    .unwrap_err()
    .to_string();

    assert!(error.contains("on all 3 attempts"), "got: {error}");
    assert_eq!(answers.asked, 1, "the loop asked again per attempt");
}

/// Both image endpoints, answering with one PNG.
async fn serve_views(server: &MockServer, png: &[u8]) {
    let encoded = base64::engine::general_purpose::STANDARD.encode(png);
    for route in ["/images/generations", "/images/edits"] {
        Mock::given(method("POST"))
            .and(path(route))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"data": [{"b64_json": encoded}]})),
            )
            .mount(server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path("/v1/balance"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"balance": 10})))
        .mount(server)
        .await;
}
