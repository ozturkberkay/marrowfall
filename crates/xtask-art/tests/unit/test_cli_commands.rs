//! The `cargo art` subcommands, driven over a temp tree and a local server.

use serde_json::json;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};
use xtask_art::cli::{
    Cli, Command, RunOptions, check, confirm_spend, new_character, pause_for_review, repo_root,
    report_balance, run_from_args, should_pause, status,
};
use xtask_art::lock::{Lock, Provider, Stage, StageRecord};
use xtask_art::spec::{CharacterSpec, CharacterType, Paths};

use crate::support::{EnvGuard, a_library, a_png, a_spec, install_library};

use base64::Engine as _;
use clap::Parser as _;

/// A repo-shaped temp tree: `cargo art` locates the root by finding `crates/`.
fn a_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("crates")).unwrap();
    std::fs::write(dir.path().join("Cargo.toml"), "[workspace]\n").unwrap();
    // Animations are shared, so every repo has the library before it can run.
    install_library(dir.path());
    dir
}

// --- new ------------------------------------------------------------------

#[test]
fn new_writes_a_template_spec_that_validates_apart_from_the_description() {
    let dir = a_repo();
    new_character(dir.path(), "skeleton", CharacterType::Humanoid).unwrap();

    let paths = Paths::new(dir.path(), "skeleton");
    assert!(paths.spec().exists());
    let spec = CharacterSpec::load(&paths.spec()).unwrap();
    assert_eq!(spec.name, "skeleton");
    assert!(
        spec.validate().is_err(),
        "the placeholder description must fail until a human replaces it"
    );
}

#[test]
fn new_refuses_to_overwrite_an_existing_spec() {
    let dir = a_repo();
    new_character(dir.path(), "skeleton", CharacterType::Humanoid).unwrap();
    let error = new_character(dir.path(), "skeleton", CharacterType::Humanoid)
        .unwrap_err()
        .to_string();
    assert!(error.contains("already exists"), "got: {error}");
}

#[test]
fn a_non_humanoid_template_has_no_animations() {
    let dir = a_repo();
    new_character(dir.path(), "hound", CharacterType::Quadruped).unwrap();
    let spec = CharacterSpec::load(&Paths::new(dir.path(), "hound").spec()).unwrap();
    assert!(spec.animations.is_empty(), "Meshy rigs bipeds only");
}

// --- status ---------------------------------------------------------------

#[test]
fn status_reports_todo_done_and_stale() {
    let library = a_library();
    let dir = a_repo();
    let paths = Paths::new(dir.path(), "survivor");
    let mut spec = a_spec("survivor");
    spec.save(&paths.spec()).unwrap();

    let mut lock = Lock::default();
    lock.record(Stage::Concept, &spec, &library, StageRecord::default());
    lock.save(&paths.lock()).unwrap();
    status(dir.path(), "survivor", false).unwrap();

    // Editing a field the concept stage consumed makes it stale.
    spec.subject.description = "a different character entirely".to_owned();
    spec.save(&paths.spec()).unwrap();
    let lock = Lock::load(&paths.lock()).unwrap();
    assert!(!lock.is_current(Stage::Concept, &spec, &library));
    assert!(lock.stages.contains_key(&Stage::Concept));
    status(dir.path(), "survivor", false).unwrap();
}

#[test]
fn status_json_lists_every_stage() {
    let dir = a_repo();
    let paths = Paths::new(dir.path(), "survivor");
    a_spec("survivor").save(&paths.spec()).unwrap();
    status(dir.path(), "survivor", true).unwrap();
}

#[test]
fn status_of_an_unknown_character_names_the_missing_file() {
    let dir = a_repo();
    let error = status(dir.path(), "nobody", false).unwrap_err().to_string();
    assert!(error.contains("nobody"), "got: {error}");
}

// --- check ----------------------------------------------------------------

#[test]
fn check_passes_over_a_directory_of_valid_specs() {
    let dir = a_repo();
    for name in ["survivor", "skeleton"] {
        a_spec(name)
            .save(&Paths::new(dir.path(), name).spec())
            .unwrap();
    }
    check(dir.path(), None).unwrap();
}

#[test]
fn check_fails_and_counts_the_invalid_specs() {
    let dir = a_repo();
    a_spec("good")
        .save(&Paths::new(dir.path(), "good").spec())
        .unwrap();
    let mut bad = a_spec("bad");
    bad.subject.description = "TODO: describe the character".to_owned();
    bad.save(&Paths::new(dir.path(), "bad").spec()).unwrap();

    let error = check(dir.path(), None).unwrap_err().to_string();
    assert!(error.contains("1 spec(s) invalid"), "got: {error}");
}

#[test]
fn check_on_an_empty_repo_says_so_rather_than_failing() {
    let dir = a_repo();
    check(dir.path(), None).unwrap();
}

#[test]
fn check_can_target_a_single_character() {
    let dir = a_repo();
    a_spec("survivor")
        .save(&Paths::new(dir.path(), "survivor").spec())
        .unwrap();
    check(dir.path(), Some("survivor")).unwrap();
}

// --- repo root ------------------------------------------------------------

#[test]
fn the_repo_root_is_found_from_the_manifest_directory() {
    // CARGO_MANIFEST_DIR points at crates/xtask-art when tests run.
    let root = repo_root().unwrap();
    assert!(root.join("crates").is_dir(), "got: {}", root.display());
    assert!(root.join("Cargo.toml").is_file());
}

// --- prompts --------------------------------------------------------------

#[test]
fn only_the_reviewable_stages_pause() {
    for stage in [Stage::Concept, Stage::Model, Stage::Bake, Stage::Pack] {
        assert!(should_pause(stage), "{stage} produces something to look at");
    }
    for stage in [Stage::Rig, Stage::Download] {
        assert!(!should_pause(stage), "{stage} has nothing to review");
    }
}

#[test]
fn yes_confirms_a_spend_without_reading_the_terminal() {
    for stage in Stage::all() {
        assert!(confirm_spend(stage, true, &mut std::io::empty()).unwrap());
    }
}

#[test]
fn a_spend_is_only_confirmed_by_an_explicit_yes() {
    for answer in ["y", "Y", "yes", "YES", " yes \n"] {
        assert!(
            confirm_spend(Stage::Model, false, &mut answer.as_bytes()).unwrap(),
            "{answer:?} should confirm"
        );
    }
    for answer in ["", "n", "no", "\n", "maybe", "yeah"] {
        assert!(
            !confirm_spend(Stage::Model, false, &mut answer.as_bytes()).unwrap(),
            "{answer:?} must not spend credits"
        );
    }
}

#[test]
fn the_review_gate_continues_on_an_empty_line() {
    let dir = a_repo();
    let paths = Paths::new(dir.path(), "survivor");
    // A bare Enter is the default, which is "yes, carry on".
    pause_for_review(Stage::Concept, &paths, &mut "\n".as_bytes()).unwrap();
    pause_for_review(Stage::Concept, &paths, &mut "y\n".as_bytes()).unwrap();
}

#[test]
fn the_review_gate_stops_the_run_when_the_answer_is_no() {
    let dir = a_repo();
    let paths = Paths::new(dir.path(), "survivor");
    for answer in ["n\n", "no\n", "NO\n"] {
        let error = pause_for_review(Stage::Concept, &paths, &mut answer.as_bytes())
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("completed stages are cached"),
            "got: {error}"
        );
    }
}

#[test]
fn the_review_gate_refuses_to_pass_itself_with_no_terminal_attached() {
    let dir = a_repo();
    let paths = Paths::new(dir.path(), "survivor");
    // An agent or a pipe: continuing silently would defeat the gate.
    let error = pause_for_review(Stage::Concept, &paths, &mut std::io::empty())
        .unwrap_err()
        .to_string();
    assert!(error.contains("--yes"), "got: {error}");
}

#[tokio::test]
async fn a_balance_lookup_failing_does_not_stop_the_pipeline() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/balance"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());

    // Advisory only: this must return, not panic or propagate.
    report_balance(Provider::Meshy).await;
    report_balance(Provider::OpenAI).await;
}

#[tokio::test]
async fn a_successful_balance_lookup_is_reported() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/balance"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"balance": 99})))
        .mount(&server)
        .await;
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());

    report_balance(Provider::Meshy).await;
}

// --- argument parsing -----------------------------------------------------

#[test]
fn every_subcommand_parses() {
    let cli = Cli::try_parse_from(["art", "new", "skeleton", "--kind", "humanoid"]).unwrap();
    assert!(matches!(
        cli.command,
        Command::New { ref name, kind } if name == "skeleton" && kind == CharacterType::Humanoid
    ));

    let cli = Cli::try_parse_from(["art", "run", "survivor", "--from", "bake", "--yes"]).unwrap();
    assert!(matches!(
        cli.command,
        Command::Run {
            from: Some(Stage::Bake),
            yes: true,
            ..
        }
    ));

    let cli = Cli::try_parse_from(["art", "status", "survivor", "--json"]).unwrap();
    assert!(matches!(cli.command, Command::Status { json: true, .. }));

    let cli = Cli::try_parse_from(["art", "check"]).unwrap();
    assert!(matches!(cli.command, Command::Check { name: None }));
}

#[test]
fn an_unknown_stage_name_lists_the_valid_ones() {
    let error = Cli::try_parse_from(["art", "run", "survivor", "--from", "wat"])
        .unwrap_err()
        .to_string();
    assert!(error.contains("concept"), "got: {error}");
}

// --- run ------------------------------------------------------------------

/// Serves every provider call the whole pipeline makes.
async fn serve_pipeline(server: &MockServer) {
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
    Mock::given(method("GET"))
        .and(path_regex(
            r"/v1/(multi-image-to-3d|rigging|animations)/t1$",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "t1", "status": "SUCCEEDED", "progress": 100, "consumed_credits": 5,
            "model_urls": {"glb": "http://localhost/files/x.glb"}
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn run_stops_at_the_first_stage_that_cannot_proceed() {
    let server = MockServer::start().await;
    serve_pipeline(&server).await;
    let dir = a_repo();
    a_spec("survivor")
        .save(&Paths::new(dir.path(), "survivor").spec())
        .unwrap();
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());

    // Bake shells out to Blender, which is not available here, so the run gets
    // as far as it can and then reports why.
    let outcome = xtask_art::cli::run(
        dir.path(),
        "survivor",
        RunOptions {
            from: None,
            only: None,
            retry: false,
        },
        true,
        &mut std::io::empty(),
    )
    .await;
    assert!(outcome.is_err(), "the bake cannot succeed without Blender");

    let lock = Lock::load(&Paths::new(dir.path(), "survivor").lock()).unwrap();
    assert!(
        lock.stages.contains_key(&Stage::Concept),
        "work completed before the failure must be recorded"
    );
}

#[tokio::test]
async fn only_runs_exactly_one_stage() {
    let server = MockServer::start().await;
    serve_pipeline(&server).await;
    let dir = a_repo();
    a_spec("survivor")
        .save(&Paths::new(dir.path(), "survivor").spec())
        .unwrap();
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());

    xtask_art::cli::run(
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

    let lock = Lock::load(&Paths::new(dir.path(), "survivor").lock()).unwrap();
    assert_eq!(lock.stages.len(), 1);
    assert!(lock.stages.contains_key(&Stage::Concept));
}

#[tokio::test]
async fn run_rejects_an_invalid_spec_before_spending_anything() {
    let dir = a_repo();
    let mut spec = a_spec("survivor");
    spec.subject.description = "TODO: describe the character".to_owned();
    spec.save(&Paths::new(dir.path(), "survivor").spec())
        .unwrap();

    let error = xtask_art::cli::run(
        dir.path(),
        "survivor",
        RunOptions {
            from: None,
            only: None,
            retry: false,
        },
        true,
        &mut std::io::empty(),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(error.contains("spec is not valid"), "got: {error}");
}

#[tokio::test]
async fn run_on_a_missing_character_names_the_file() {
    let dir = a_repo();
    let error = xtask_art::cli::run(
        dir.path(),
        "nobody",
        RunOptions {
            from: None,
            only: None,
            retry: false,
        },
        true,
        &mut std::io::empty(),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(error.contains("nobody"), "got: {error}");
}

// --- dispatch -------------------------------------------------------------

#[tokio::test]
async fn the_entry_point_dispatches_new_and_creates_the_spec() {
    let dir = a_repo();
    let mut env = EnvGuard::new();
    env.set("CARGO_MANIFEST_DIR", dir.path().to_str().unwrap());

    run_from_args(["art", "new", "skeleton", "--kind", "humanoid"])
        .await
        .unwrap();

    assert!(Paths::new(dir.path(), "skeleton").spec().exists());
}

#[tokio::test]
async fn the_entry_point_dispatches_check_and_status() {
    let dir = a_repo();
    a_spec("survivor")
        .save(&Paths::new(dir.path(), "survivor").spec())
        .unwrap();
    let mut env = EnvGuard::new();
    env.set("CARGO_MANIFEST_DIR", dir.path().to_str().unwrap());

    run_from_args(["art", "check"]).await.unwrap();
    run_from_args(["art", "status", "survivor"]).await.unwrap();
    run_from_args(["art", "status", "survivor", "--json"])
        .await
        .unwrap();
}

#[tokio::test]
async fn the_entry_point_dispatches_run() {
    let dir = a_repo();
    let mut spec = a_spec("survivor");
    spec.subject.description = "TODO: describe the character".to_owned();
    spec.save(&Paths::new(dir.path(), "survivor").spec())
        .unwrap();
    let mut env = EnvGuard::new();
    env.set("CARGO_MANIFEST_DIR", dir.path().to_str().unwrap());

    // Reaching the spec check proves the run arm was taken.
    let error = run_from_args(["art", "run", "survivor", "--yes"])
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("spec is not valid"), "got: {error}");
}

#[tokio::test]
async fn an_unparseable_command_line_is_an_error_rather_than_a_process_exit() {
    let error = run_from_args(["art", "no-such-command"])
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("no-such-command"), "got: {error}");
}
