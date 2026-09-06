//! The `cargo art` subcommands, driven over a temp tree and a local server.

use serde_json::json;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};
use xtask_art::check::profile::Profile;
use xtask_art::cli::{
    Cli, Command, RunOptions, check, confirm_spend, new_character, repo_root, report_balance,
    run_from_args, spend_prompt, status,
};
use xtask_art::lock::{Lock, Provider, Stage, StageRecord};
use xtask_art::spec::{CharacterSpec, CharacterType, Paths};

use crate::support::{
    EnvGuard, a_concept_view, a_library, a_spec, install_library, install_skeleton,
    repo_root as real_repo,
};

use base64::Engine as _;
use clap::Parser as _;

/// A repo-shaped temp tree: `cargo art` locates the root by finding `crates/`.
fn a_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("crates")).unwrap();
    std::fs::write(dir.path().join("Cargo.toml"), "[workspace]\n").unwrap();
    // Animations are shared, so every repo has the library before it can run.
    install_library(dir.path());
    // And every published limit is skeleton data, which the concept gate at
    // the first stage boundary already reads.
    install_skeleton(dir.path());
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
    check(dir.path(), None, false).unwrap();
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

    let error = check(dir.path(), None, false).unwrap_err().to_string();
    assert!(error.contains("1 spec(s) invalid"), "got: {error}");
}

#[test]
fn check_on_an_empty_repo_says_so_rather_than_failing() {
    let dir = a_repo();
    check(dir.path(), None, false).unwrap();
}

#[test]
fn check_can_target_a_single_character() {
    let dir = a_repo();
    a_spec("survivor")
        .save(&Paths::new(dir.path(), "survivor").spec())
        .unwrap();
    check(dir.path(), Some("survivor"), false).unwrap();
}

/// A repo holding the committed skeleton profile and the committed rig as
/// one character's model, so the rig gates have real art to measure and
/// their report lands in the temp tree.
fn a_repo_with_a_rig(name: &str) -> tempfile::TempDir {
    let dir = a_repo();
    let profile = real_repo().join("art/skeletons/humanoid.toml");
    let target = dir.path().join("art/skeletons/humanoid.toml");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::copy(profile, target).unwrap();
    let spec = a_spec(name);
    spec.save(&Paths::new(dir.path(), name).spec()).unwrap();
    let glb = Paths::new(dir.path(), name).character_glb();
    std::fs::create_dir_all(glb.parent().unwrap()).unwrap();
    std::fs::copy(real_repo().join("art/skeletons/humanoid.glb"), glb).unwrap();
    dir
}

/// The committed rig breaks eight rules on twenty-three subjects, seven of
/// them `rig.*` and one the aim table's `hips` row, and the command says so
/// rather than reporting the spec as fine.
#[test]
fn check_itemizes_the_defects_of_the_rig_on_disk() {
    let dir = a_repo_with_a_rig("survivor");

    let error = check(dir.path(), None, false).unwrap_err().to_string();

    assert!(error.contains("14 defect(s)"), "got: {error}");
}

/// The mesh gates run on the bare mesh, which is the mesh before rigging.
/// The committed `model.glb` is the file after rigging, so it stands in as
/// the calibration asset until `bare.glb` can be downloaded.
#[test]
fn check_measures_the_bare_mesh_and_writes_its_own_report() {
    let dir = a_repo_with_a_rig("survivor");
    let bare = Paths::new(dir.path(), "survivor").bare_glb();
    std::fs::create_dir_all(bare.parent().unwrap()).unwrap();
    std::fs::copy(real_repo().join("art/characters/survivor/model.glb"), &bare).unwrap();

    // The rig still fails, so the command still fails.
    check(dir.path(), None, false).unwrap_err();

    let report = xtask_art::check::Report::read(
        &dir.path().join("art/staging/reports/mesh.survivor.1.json"),
    )
    .unwrap();
    assert_eq!(report.stage(), "mesh");
    assert_eq!(report.findings().len(), 13);
    assert!(!report.has_errors(), "the calibration asset passes");
    // `check` calls nothing, so the one remote rule reports as unavailable
    // rather than going quiet.
    let remote = report
        .findings()
        .iter()
        .find(|finding| finding.rule == "mesh.printability")
        .expect("the remote rule reports either way");
    assert_eq!(remote.severity, xtask_art::check::Severity::Warning);
}

#[test]
fn check_says_when_a_character_has_no_bare_mesh_on_disk_yet() {
    let dir = a_repo_with_a_rig("survivor");

    // The rig is there and the bare mesh is not, so only the rig is
    // measured and only the rig fails.
    let error = check(dir.path(), None, false).unwrap_err().to_string();

    assert!(error.contains("14 defect(s)"), "got: {error}");
    assert!(
        !dir.path()
            .join("art/staging/reports/mesh.survivor.1.json")
            .exists(),
        "nothing to measure leaves no report"
    );
}

#[test]
fn check_writes_the_rig_report_where_the_runner_writes_every_report() {
    let dir = a_repo_with_a_rig("survivor");

    check(dir.path(), None, false).unwrap_err();

    let report =
        xtask_art::check::Report::read(&dir.path().join("art/staging/reports/rig.survivor.1.json"))
            .unwrap();
    assert_eq!(report.stage(), "rig");
    assert_eq!(report.item(), "survivor");
    assert!(report.has_errors());
}

#[test]
fn check_says_when_a_character_has_no_rig_on_disk_yet() {
    let dir = a_repo();
    a_spec("survivor")
        .save(&Paths::new(dir.path(), "survivor").spec())
        .unwrap();

    // No model.glb, so there is nothing to measure and nothing to report.
    check(dir.path(), None, false).unwrap();
}

#[test]
fn check_leaves_a_character_that_is_never_rigged_alone() {
    let dir = a_repo();
    let mut quadruped = a_spec("hound");
    quadruped.subject.kind = CharacterType::Quadruped;
    quadruped.animations.clear();
    quadruped
        .save(&Paths::new(dir.path(), "hound").spec())
        .unwrap();

    check(dir.path(), None, false).unwrap();
}

#[test]
fn the_rule_list_needs_the_profile_that_publishes_every_limit() {
    check(&real_repo(), None, true).unwrap();

    // A repository with no `art/skeletons` at all, which is what a rule list
    // with nothing to read its limits from looks like.
    let bare = tempfile::tempdir().unwrap();
    let error = check(bare.path(), None, true).unwrap_err().to_string();
    assert!(error.contains("no skeleton profile"), "got: {error}");
}

/// The limits are per profile, so a second skeleton has to reach the list.
/// Hardcoding one name would leave its rules unprintable.
#[test]
fn the_rule_list_covers_every_skeleton_the_repository_declares() {
    let dir = a_repo_with_a_rig("survivor");
    let profiles = Profile::dir(dir.path());
    std::fs::copy(
        profiles.join("humanoid.toml"),
        profiles.join("quadruped.toml"),
    )
    .unwrap();

    assert_eq!(
        Profile::declared(dir.path()).unwrap(),
        ["humanoid", "quadruped"]
    );
    check(dir.path(), None, true).unwrap();
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

/// The one mid-run touchpoint quotes what it is about to spend, and the
/// concept stage quotes the whole retry loop rather than one generation,
/// because that is what saying yes buys.
#[test]
fn the_spend_prompt_quotes_what_the_stage_costs() {
    assert_eq!(
        spend_prompt(Stage::Concept),
        "  concept already completed and costs up to 3 attempts of 4 OpenAI images each, \
         about 2.40 USD in total. Re-run? [y/N] "
    );
    assert_eq!(
        spend_prompt(Stage::Rig),
        "  rig already completed and costs credits. Re-run? [y/N] "
    );
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
    assert!(matches!(
        cli.command,
        Command::Check {
            name: None,
            list_rules: false
        }
    ));

    let cli = Cli::try_parse_from(["art", "check", "--list-rules"]).unwrap();
    assert!(matches!(
        cli.command,
        Command::Check {
            list_rules: true,
            ..
        }
    ));
    assert!(
        Cli::try_parse_from(["art", "check", "survivor", "--list-rules"]).is_err(),
        "one character's defects and the whole rule list are two questions"
    );
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
    let view = base64::engine::general_purpose::STANDARD.encode(a_concept_view());
    for route in ["/images/generations", "/images/edits"] {
        Mock::given(method("POST"))
            .and(path(route))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"data": [{"b64_json": view}]})),
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
async fn the_entry_point_dispatches_fetch() {
    let dir = a_repo();
    a_library().save(dir.path()).unwrap();
    let mut env = EnvGuard::new();
    env.set("CARGO_MANIFEST_DIR", dir.path().to_str().unwrap());

    // Every entry in the template library is bought with the rig, so this
    // reaches the command and finds nothing to do.
    run_from_args(["art", "fetch"]).await.unwrap();
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

/// The four combinations of the two flags, on the one command a reviewer
/// runs by hand. Each rule reports either way: what changes is whether it
/// measured or says which declaration switched it off.
#[test]
fn check_reports_every_rule_under_all_four_declarations() {
    for (cleanup, symmetry, measured) in [
        (true, true, 2),
        (true, false, 2),
        (false, true, 0),
        (false, false, 0),
    ] {
        let dir = a_repo();
        crate::support::install_skeleton(dir.path());
        let paths = Paths::new(dir.path(), "survivor");
        let mut spec = a_spec("survivor");
        spec.subject.cleanup = cleanup;
        spec.subject.symmetry = symmetry;
        spec.save(&paths.spec()).unwrap();
        std::fs::create_dir_all(paths.staging()).unwrap();
        std::fs::write(paths.bare_glb(), crate::support::a_bare_mesh()).unwrap();
        std::fs::write(paths.clean_glb(), crate::support::a_cleaned_mesh()).unwrap();

        check(dir.path(), None, false).unwrap();

        let mesh = a_report(dir.path(), "mesh");
        let mirror = finding_of(&mesh, "mesh.mirror");
        assert_eq!(
            mirror.severity == xtask_art::check::Severity::Skipped,
            !symmetry,
            "symmetry {symmetry}: {mirror:#?}"
        );
        // The file rules read the cleaned mesh only where a fixer wrote one.
        assert_eq!(
            dir.path()
                .join("art/staging/reports/cleaned.survivor.1.json")
                .exists(),
            cleanup,
            "cleanup {cleanup}"
        );
        let pair = a_report(dir.path(), "cleanup");
        assert_eq!(pair.findings().len(), 2);
        assert_eq!(
            pair.findings()
                .iter()
                .filter(|finding| finding.severity != xtask_art::check::Severity::Skipped)
                .count(),
            measured,
            "cleanup {cleanup}: {pair:#?}"
        );
    }
}

/// And a character whose fixer has not run yet is unbuilt rather than
/// broken: the rig stage is what runs it.
#[test]
fn check_says_when_a_character_has_no_cleaned_mesh_on_disk_yet() {
    let dir = a_repo();
    crate::support::install_skeleton(dir.path());
    let paths = Paths::new(dir.path(), "survivor");
    a_spec("survivor").save(&paths.spec()).unwrap();
    std::fs::create_dir_all(paths.staging()).unwrap();
    std::fs::write(paths.bare_glb(), crate::support::a_bare_mesh()).unwrap();

    check(dir.path(), None, false).unwrap();

    for stage in ["cleaned", "cleanup"] {
        assert!(
            !dir.path()
                .join(format!("art/staging/reports/{stage}.survivor.1.json"))
                .exists(),
            "nothing to measure leaves no {stage} report"
        );
    }
}

/// And a character with no mesh at all, whatever it declares: a pair of
/// skips would read as a cleanup that was measured and found switched off.
#[test]
fn check_says_nothing_about_the_cleanup_of_a_character_with_no_mesh() {
    let dir = a_repo();
    crate::support::install_skeleton(dir.path());
    let mut spec = a_spec("survivor");
    spec.subject.cleanup = false;
    spec.save(&Paths::new(dir.path(), "survivor").spec())
        .unwrap();

    check(dir.path(), None, false).unwrap();

    assert!(
        !dir.path()
            .join("art/staging/reports/cleanup.survivor.1.json")
            .exists(),
        "no mesh on disk leaves no report"
    );
}

fn a_report(root: &std::path::Path, stage: &str) -> xtask_art::check::Report {
    xtask_art::check::Report::read(
        &root.join(format!("art/staging/reports/{stage}.survivor.1.json")),
    )
    .unwrap_or_else(|error| panic!("reading the {stage} report: {error:#}"))
}

fn finding_of(report: &xtask_art::check::Report, rule: &str) -> xtask_art::check::Finding {
    report
        .findings()
        .iter()
        .find(|finding| finding.rule == rule)
        .unwrap_or_else(|| panic!("{rule} reported nothing"))
        .clone()
}
