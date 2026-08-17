use crate::support::a_library;
use xtask_art::cli::{RunOptions, Step, plan};
use xtask_art::library::AnimationLibrary;
use xtask_art::lock::{Lock, Stage, StageRecord, fingerprint};
use xtask_art::spec::{CharacterSpec, CharacterType};

fn spec() -> CharacterSpec {
    let mut spec = CharacterSpec::template("survivor", CharacterType::Humanoid);
    spec.subject.description = "a lean survivor".to_owned();
    spec
}

fn completed(spec: &CharacterSpec, stages: &[Stage], library: &AnimationLibrary) -> Lock {
    let mut lock = Lock::default();
    for stage in stages {
        // Insert directly: `record` deliberately clears downstream stages,
        // which would defeat setting up a fully-complete fixture.
        lock.stages.insert(
            *stage,
            StageRecord {
                fingerprint: fingerprint(*stage, spec, library),
                ..StageRecord::default()
            },
        );
    }
    lock
}

#[test]
fn a_fresh_character_runs_every_stage() {
    let library = a_library();
    let spec = spec();
    let steps = plan(
        &Lock::default(),
        &spec,
        &library,
        RunOptions::default(),
        false,
    );
    assert_eq!(steps.len(), Stage::all().len());
    assert!(steps.iter().all(|step| matches!(step, Step::Run(_))));
}

#[test]
fn completed_stages_are_reported_as_cached() {
    let library = a_library();
    let spec = spec();
    let lock = completed(&spec, &[Stage::Concept, Stage::Model], &library);
    let steps = plan(&lock, &spec, &library, RunOptions::default(), false);
    assert_eq!(steps[0], Step::Cached(Stage::Concept));
    assert_eq!(steps[1], Step::Cached(Stage::Model));
    assert_eq!(steps[2], Step::Run(Stage::Rig));
}

/// The tool's core economic promise: re-running local work never spends.
#[test]
fn rerunning_local_stages_never_reaches_a_paid_stage() {
    let library = a_library();
    let spec = spec();
    let lock = completed(&spec, &Stage::all(), &library);
    let steps = plan(
        &lock,
        &spec,
        &library,
        RunOptions {
            from: Some(Stage::Bake),
            ..RunOptions::default()
        },
        false,
    );
    assert!(
        steps.iter().all(|step| !matches!(
            step,
            Step::Run(stage) | Step::ConfirmSpend(stage) if stage.costs_credits()
        )),
        "no paid stage may run: {steps:?}"
    );
    assert_eq!(steps, vec![Step::Run(Stage::Bake), Step::Run(Stage::Pack)]);
}

/// `--from` must not become a silent way to re-spend: it forces the stage
/// to run, but a previously completed paid stage still asks first.
#[test]
fn from_a_paid_stage_asks_before_spending() {
    let library = a_library();
    let spec = spec();
    let lock = completed(&spec, &Stage::all(), &library);
    let steps = plan(
        &lock,
        &spec,
        &library,
        RunOptions {
            from: Some(Stage::Concept),
            ..RunOptions::default()
        },
        false,
    );
    assert_eq!(steps[0], Step::ConfirmSpend(Stage::Concept));
    assert_eq!(steps[1], Step::ConfirmSpend(Stage::Model));
    assert_eq!(
        steps[3],
        Step::Run(Stage::Download),
        "unpaid stages just run"
    );
}

/// `--only` on a completed paid stage previously bypassed the cache check
/// entirely and re-spent without asking.
#[test]
fn only_a_paid_stage_asks_before_spending() {
    let library = a_library();
    let spec = spec();
    let lock = completed(&spec, &Stage::all(), &library);
    let steps = plan(
        &lock,
        &spec,
        &library,
        RunOptions {
            only: Some(Stage::Model),
            ..RunOptions::default()
        },
        false,
    );
    assert_eq!(steps, vec![Step::ConfirmSpend(Stage::Model)]);
}

#[test]
fn only_runs_exactly_one_stage() {
    let library = a_library();
    let spec = spec();
    let steps = plan(
        &Lock::default(),
        &spec,
        &library,
        RunOptions {
            only: Some(Stage::Bake),
            ..RunOptions::default()
        },
        false,
    );
    assert_eq!(steps, vec![Step::Run(Stage::Bake)]);
}

#[test]
fn retry_forces_completed_stages_to_run_again() {
    let library = a_library();
    let spec = spec();
    let lock = completed(&spec, &Stage::all(), &library);
    let steps = plan(
        &lock,
        &spec,
        &library,
        RunOptions {
            retry: true,
            ..RunOptions::default()
        },
        false,
    );
    assert_eq!(steps[0], Step::ConfirmSpend(Stage::Concept), "paid: asks");
    assert_eq!(steps[4], Step::Run(Stage::Bake), "free: just runs");
}

/// A GLB on disk with no lock record came from outside the tool. Because
/// AI generation is not reproducible, re-deriving it would spend credits
/// *and* replace the original with a different character.
#[test]
fn an_existing_checkpoint_prevents_regenerating_it() {
    let library = a_library();
    let spec = spec();
    let steps = plan(
        &Lock::default(),
        &spec,
        &library,
        RunOptions::default(),
        true,
    );

    for stage in [Stage::Concept, Stage::Model, Stage::Rig, Stage::Download] {
        let step = steps
            .iter()
            .find(|s| matches!(s, Step::Skipped(got, _) if *got == stage));
        assert!(step.is_some(), "{stage} must be skipped, got {steps:?}");
    }
    assert_eq!(steps[4], Step::Run(Stage::Bake), "local work still runs");
    assert_eq!(steps[5], Step::Run(Stage::Pack));
}

/// The checkpoint is a safety net, not a lock: asking explicitly still works.
#[test]
fn an_existing_checkpoint_can_be_overridden_explicitly() {
    let library = a_library();
    let spec = spec();
    let steps = plan(
        &Lock::default(),
        &spec,
        &library,
        RunOptions {
            only: Some(Stage::Model),
            ..RunOptions::default()
        },
        true,
    );
    assert_eq!(steps, vec![Step::Run(Stage::Model)]);
}

/// `--from` is an explicit instruction and must outrank the
/// checkpoint-on-disk guard, which exists only to stop *accidental*
/// regeneration.
#[test]
fn from_overrides_an_existing_checkpoint() {
    let library = a_library();
    let spec = spec();
    let steps = plan(
        &Lock::default(),
        &spec,
        &library,
        RunOptions {
            from: Some(Stage::Model),
            ..RunOptions::default()
        },
        true,
    );
    assert_eq!(steps[0], Step::Run(Stage::Model));
}

#[test]
fn unriggable_characters_skip_the_rig_stage() {
    let library = a_library();
    let mut spec = spec();
    spec.subject.kind = CharacterType::Quadruped;
    spec.animations.clear();

    let steps = plan(
        &Lock::default(),
        &spec,
        &library,
        RunOptions::default(),
        false,
    );
    assert!(matches!(steps[2], Step::Skipped(Stage::Rig, _)));
    assert_eq!(steps[3], Step::Run(Stage::Download));
}

#[test]
fn stale_fingerprints_cause_a_rerun() {
    let library = a_library();
    let mut spec = spec();
    let lock = completed(&spec, &Stage::all(), &library);
    spec.bake.sprite_height = 200;

    let steps = plan(&lock, &spec, &library, RunOptions::default(), false);
    assert_eq!(
        steps[0],
        Step::Cached(Stage::Concept),
        "paid work is preserved"
    );
    assert_eq!(steps[5], Step::Run(Stage::Pack), "pack reads sprite_height");
}
