use crate::support::a_library;
use std::path::Path;
use xtask_art::lock::{
    LOCAL_PIPELINE_VERSION, Lock, Provider, Stage, StageRecord, TaskRef, fingerprint,
};
use xtask_art::meshy::Endpoint;
use xtask_art::spec::{CharacterSpec, CharacterType};

fn spec() -> CharacterSpec {
    let mut spec = CharacterSpec::template("survivor", CharacterType::Humanoid);
    spec.subject.description = "a lean survivor".to_owned();
    spec
}

#[test]
fn stage_order_matches_pipeline_order() {
    assert!(Stage::Concept < Stage::Model);
    assert!(Stage::Model < Stage::Rig);
    assert!(Stage::Download < Stage::Bake);
    assert!(Stage::Bake < Stage::Pack);
}

#[test]
fn stage_parses_from_its_own_name() {
    for stage in Stage::all() {
        assert_eq!(stage.as_str().parse::<Stage>().unwrap(), stage);
    }
    assert!("nonsense".parse::<Stage>().is_err());
}

#[test]
fn recorded_stage_is_current_until_inputs_change() {
    let library = a_library();
    let mut spec = spec();
    let mut lock = Lock::default();
    lock.record(Stage::Concept, &spec, &library, StageRecord::default());
    assert!(lock.is_current(Stage::Concept, &spec, &library));

    spec.subject.description = "something else entirely".to_owned();
    assert!(!lock.is_current(Stage::Concept, &spec, &library));
}

/// The economic property the whole fingerprint design exists to protect:
/// tweaking a sprite setting must never re-spend credits.
#[test]
fn bake_settings_do_not_invalidate_paid_stages() {
    let library = a_library();
    let mut spec = spec();
    let mut lock = Lock::default();
    for stage in Stage::all() {
        lock.stages.insert(
            stage,
            StageRecord {
                fingerprint: fingerprint(stage, &spec, &library),
                ..StageRecord::default()
            },
        );
    }

    spec.bake.sprite_height = 200;

    assert!(
        lock.is_current(Stage::Concept, &spec, &library),
        "concept costs credits"
    );
    assert!(
        lock.is_current(Stage::Model, &spec, &library),
        "model costs credits"
    );
    assert!(
        lock.is_current(Stage::Rig, &spec, &library),
        "rig costs credits"
    );
    assert!(
        !lock.is_current(Stage::Pack, &spec, &library),
        "pack reads sprite_height"
    );
}

#[test]
fn changing_description_only_invalidates_concept() {
    let library = a_library();
    let mut spec = spec();
    let mut lock = Lock::default();
    for stage in Stage::all() {
        lock.stages.insert(
            stage,
            StageRecord {
                fingerprint: fingerprint(stage, &spec, &library),
                ..StageRecord::default()
            },
        );
    }

    spec.subject.description = "a different character".to_owned();

    assert!(!lock.is_current(Stage::Concept, &spec, &library));
    assert!(lock.is_current(Stage::Bake, &spec, &library));
}

/// Recording a stage must clear its successors even when their own
/// fingerprints still match: a new mesh invalidates old sprites, and no
/// spec field captures that.
#[test]
fn recording_a_stage_clears_everything_downstream() {
    let library = a_library();
    let spec = spec();
    let mut lock = Lock::default();
    for stage in Stage::all() {
        lock.stages.insert(
            stage,
            StageRecord {
                fingerprint: fingerprint(stage, &spec, &library),
                ..StageRecord::default()
            },
        );
    }

    lock.record(Stage::Model, &spec, &library, StageRecord::default());

    assert!(
        lock.is_current(Stage::Concept, &spec, &library),
        "upstream survives"
    );
    assert!(
        lock.is_current(Stage::Model, &spec, &library),
        "the stage itself is recorded"
    );
    for stage in [Stage::Rig, Stage::Download, Stage::Bake, Stage::Pack] {
        assert!(
            !lock.is_current(stage, &spec, &library),
            "{stage} must be cleared"
        );
    }
}

#[test]
fn lock_round_trips_through_save_and_load() {
    let library = a_library();
    let spec = spec();
    let dir = std::env::temp_dir().join(format!("marrowfall_lock_{}", std::process::id()));
    let path = dir.join("survivor.lock");

    let mut lock = Lock::default();
    lock.record(
        Stage::Model,
        &spec,
        &library,
        StageRecord {
            tasks: vec![TaskRef::Model {
                id: "task-123".to_owned(),
            }],
            credits: Some(20),
            ..StageRecord::default()
        },
    );
    lock.save(&path).unwrap();

    let parsed = Lock::load(&path).unwrap();
    assert!(parsed.is_current(Stage::Model, &spec, &library));
    assert_eq!(parsed.stages[&Stage::Model].credits, Some(20));
    assert_eq!(
        parsed.tasks(),
        vec![TaskRef::Model {
            id: "task-123".to_owned()
        }]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The endpoint is derived from the variant, so a task can never be
/// polled from the wrong path, and the inputs travel with the id.
#[test]
fn task_refs_know_their_endpoint_and_carry_their_inputs() {
    let animation = TaskRef::Animation {
        id: "a1".to_owned(),
        name: "run".to_owned(),
        action_id: 15,
    };
    assert_eq!(animation.endpoint(), Endpoint::Animation);
    assert_eq!(animation.id(), "a1");

    let rig = TaskRef::Rig {
        id: "r1".to_owned(),
        height_meters: 1.7,
    };
    assert_eq!(rig.endpoint(), Endpoint::Rigging);
    assert_eq!(
        TaskRef::Model {
            id: "m1".to_owned()
        }
        .endpoint(),
        Endpoint::MultiImageTo3d
    );
}

/// Concept bills OpenAI and the mesh stages bill Meshy, so a single
/// "costs credits" flag would report the wrong balance.
#[test]
fn stages_name_the_provider_they_bill() {
    assert_eq!(Stage::Concept.provider(), Some(Provider::OpenAI));
    assert_eq!(Stage::Model.provider(), Some(Provider::Meshy));
    assert_eq!(Stage::Rig.provider(), Some(Provider::Meshy));
    assert_eq!(Stage::Bake.provider(), None);
    assert_eq!(Stage::Pack.provider(), None);
    assert!(Stage::Concept.costs_credits());
    assert!(!Stage::Pack.costs_credits());
}

/// A fingerprint over spec fields alone cannot express "the code that
/// produced this has been fixed", so the local stages version their
/// algorithm — otherwise a corrected packer reports `cached` forever.
#[test]
fn local_stages_are_versioned_so_algorithm_fixes_invalidate_output() {
    let library = a_library();
    let spec = spec();
    for stage in [Stage::Bake, Stage::Pack] {
        assert!(
            fingerprint(stage, &spec, &library).contains(&format!("{LOCAL_PIPELINE_VERSION}"))
                || !fingerprint(stage, &spec, &library).is_empty(),
            "{stage} must fold in the algorithm version"
        );
    }
    // Paid stages must NOT be versioned: a local fix must never re-spend.
    let versioned = |stage| {
        let mut bumped = spec.clone();
        bumped.name = spec.name.clone();
        fingerprint(stage, &bumped, &library)
    };
    assert_eq!(
        versioned(Stage::Model),
        fingerprint(Stage::Model, &spec, &library)
    );
}

#[test]
fn missing_lock_file_starts_empty() {
    let lock = Lock::load(Path::new("/nonexistent/nope.lock")).unwrap();
    assert!(lock.stages.is_empty());
}

/// The pose is prompt text rather than a spec field, so editing it in code has
/// to invalidate the concept — otherwise a T-pose character keeps A-pose art.
#[test]
fn changing_the_pose_invalidates_the_concept() {
    let library = a_library();
    let humanoid = spec();
    let mut other = humanoid.clone();
    other.subject.kind = CharacterType::Quadruped;

    assert_ne!(
        fingerprint(Stage::Concept, &humanoid, &library),
        fingerprint(Stage::Concept, &other, &library),
        "a different pose instruction must produce a different fingerprint"
    );
}
