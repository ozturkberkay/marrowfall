use crate::support::{BLENDER, a_library, a_tree, edit_an_aim_row, flip_a_byte, inputs};
use std::path::Path;
use xtask_art::blender::Build;
use xtask_art::library::{Animation, AnimationLibrary, HUMANOID, MotionSource};
use xtask_art::lock::{
    self, Inputs, Lock, Provider, Stage, StageRecord, State, TaskRef, blender_inputs, fingerprint,
};
use xtask_art::providers::meshy::Endpoint;
use xtask_art::spec::{CharacterSpec, CharacterType, Paths, View};

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
    let tree = a_tree();
    let root = tree.path();
    let mut lock = Lock::default();
    lock.record(
        Stage::Concept,
        &inputs(root, &spec, &library),
        StageRecord::default(),
    )
    .unwrap();
    assert!(
        lock.is_current(Stage::Concept, &inputs(root, &spec, &library))
            .unwrap()
    );

    spec.subject.description = "something else entirely".to_owned();
    assert!(
        !lock
            .is_current(Stage::Concept, &inputs(root, &spec, &library))
            .unwrap()
    );
}

/// The economic property the whole fingerprint design exists to protect:
/// tweaking a sprite setting must never re-spend credits.
#[test]
fn bake_settings_do_not_invalidate_paid_stages() {
    let library = a_library();
    let mut spec = spec();
    let tree = a_tree();
    let root = tree.path();
    let mut lock = Lock::default();
    for stage in Stage::all() {
        lock.stages.insert(
            stage,
            StageRecord {
                fingerprint: fingerprint(stage, &inputs(root, &spec, &library)).unwrap(),
                ..StageRecord::default()
            },
        );
    }

    spec.bake.sprite_height = 200;

    assert!(
        lock.is_current(Stage::Concept, &inputs(root, &spec, &library))
            .unwrap(),
        "concept costs credits"
    );
    assert!(
        lock.is_current(Stage::Model, &inputs(root, &spec, &library))
            .unwrap(),
        "model costs credits"
    );
    assert!(
        lock.is_current(Stage::Rig, &inputs(root, &spec, &library))
            .unwrap(),
        "rig costs credits"
    );
    assert!(
        !lock
            .is_current(Stage::Pack, &inputs(root, &spec, &library))
            .unwrap(),
        "pack reads sprite_height"
    );
}

#[test]
fn changing_description_only_invalidates_concept() {
    let library = a_library();
    let mut spec = spec();
    let tree = a_tree();
    let root = tree.path();
    let mut lock = Lock::default();
    for stage in Stage::all() {
        lock.stages.insert(
            stage,
            StageRecord {
                fingerprint: fingerprint(stage, &inputs(root, &spec, &library)).unwrap(),
                ..StageRecord::default()
            },
        );
    }

    spec.subject.description = "a different character".to_owned();

    assert!(
        !lock
            .is_current(Stage::Concept, &inputs(root, &spec, &library))
            .unwrap()
    );
    assert!(
        lock.is_current(Stage::Bake, &inputs(root, &spec, &library))
            .unwrap()
    );
}

/// Recording a stage must clear its successors even when their own
/// fingerprints still match: a new mesh invalidates old sprites, and no
/// spec field captures that.
#[test]
fn recording_a_stage_clears_everything_downstream() {
    let library = a_library();
    let spec = spec();
    let tree = a_tree();
    let root = tree.path();
    let mut lock = Lock::default();
    for stage in Stage::all() {
        lock.stages.insert(
            stage,
            StageRecord {
                fingerprint: fingerprint(stage, &inputs(root, &spec, &library)).unwrap(),
                ..StageRecord::default()
            },
        );
    }

    lock.record(
        Stage::Model,
        &inputs(root, &spec, &library),
        StageRecord::default(),
    )
    .unwrap();

    assert!(
        lock.is_current(Stage::Concept, &inputs(root, &spec, &library))
            .unwrap(),
        "upstream survives"
    );
    assert!(
        lock.is_current(Stage::Model, &inputs(root, &spec, &library))
            .unwrap(),
        "the stage itself is recorded"
    );
    for stage in [Stage::Rig, Stage::Download, Stage::Bake, Stage::Pack] {
        assert!(
            !lock
                .is_current(stage, &inputs(root, &spec, &library))
                .unwrap(),
            "{stage} must be cleared"
        );
    }
}

#[test]
fn lock_round_trips_through_save_and_load() {
    let library = a_library();
    let spec = spec();
    let tree = a_tree();
    let root = tree.path();
    let path = root.join("survivor.lock");

    let mut lock = Lock::default();
    lock.record(
        Stage::Model,
        &inputs(root, &spec, &library),
        StageRecord {
            tasks: vec![TaskRef::Model {
                id: "task-123".to_owned(),
            }],
            credits: Some(20),
            ..StageRecord::default()
        },
    )
    .unwrap();
    lock.save(&path).unwrap();

    let parsed = Lock::load(&path).unwrap();
    assert!(
        parsed
            .is_current(Stage::Model, &inputs(root, &spec, &library))
            .unwrap()
    );
    assert_eq!(parsed.stages[&Stage::Model].credits, Some(20));
    assert_eq!(
        parsed.tasks(),
        vec![TaskRef::Model {
            id: "task-123".to_owned()
        }]
    );
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

/// A fingerprint over inputs alone cannot express "the code that produced
/// this has been fixed", so the stages that run our own code carry its
/// version, otherwise a corrected packer reports `cached` forever.
///
/// The concept and the rig do not, because a local fix must never re-spend
/// on OpenAI or on rigging. The download does: it renames and conforms the
/// rig it fetched and fits every bought clip onto it, and all three are ours.
/// Re-running it bills nothing, because it keeps the vendor's own files.
#[test]
fn only_the_stages_running_our_own_code_are_versioned() {
    for stage in [Stage::Model, Stage::Download, Stage::Bake, Stage::Pack] {
        assert!(stage.is_versioned(), "{stage} produces its own output");
    }
    for stage in [Stage::Concept, Stage::Rig] {
        assert!(!stage.is_versioned(), "{stage} runs no code of ours");
    }
}

/// The rename and the conform read the profile, so an edit to it rebuilds
/// `model.glb`. The rig stage must not read it: that one bills.
#[test]
fn a_profile_edit_re_conforms_the_rig_and_buys_nothing() {
    let library = a_library();
    let spec = spec();
    let tree = a_tree();
    let root = tree.path();
    let before = |stage| fingerprint(stage, &inputs(root, &spec, &library)).unwrap();
    let (rig, download) = (before(Stage::Rig), before(Stage::Download));

    edit_an_aim_row(root);

    assert_eq!(before(Stage::Rig), rig, "a table edit must not re-rig");
    assert_ne!(before(Stage::Download), download);
}

/// And so does the file the vendor returned, which is what they both read.
#[test]
fn replacing_the_vendors_rigged_file_re_conforms_and_buys_nothing() {
    let library = a_library();
    let spec = spec();
    let tree = a_tree();
    let root = tree.path();
    let rigged = Paths::new(root, "survivor").rigged_glb();
    std::fs::create_dir_all(rigged.parent().unwrap()).unwrap();
    std::fs::write(&rigged, b"glTF one").unwrap();
    let before = |stage| fingerprint(stage, &inputs(root, &spec, &library)).unwrap();
    let (rig, download) = (before(Stage::Rig), before(Stage::Download));

    std::fs::write(&rigged, b"glTF two").unwrap();

    assert_eq!(before(Stage::Rig), rig);
    assert_ne!(before(Stage::Download), download);
}

/// Free motion must not be able to invalidate a paid stage.
#[test]
fn adding_a_free_animation_does_not_re_run_the_paid_rig() {
    let base_library = a_library();
    let base_spec = spec();
    let tree = a_tree();
    let root = tree.path();
    let rig_before = fingerprint(Stage::Rig, &inputs(root, &base_spec, &base_library)).unwrap();
    let bake_before = fingerprint(Stage::Bake, &inputs(root, &base_spec, &base_library)).unwrap();

    let mut library = base_library.clone();
    let mut spec = base_spec.clone();
    library.animations.insert(
        "strafe_left".to_owned(),
        Animation {
            skeleton: xtask_art::library::HUMANOID.to_owned(),
            loops: true,
            fps: 24,
            source_fps: 30,
            travels: true,
            source: MotionSource::Mixamo {
                product_id: "c9c97b90-b96c-11e4-a802-0aaa78deedf9".to_owned(),
            },
        },
    );
    spec.animations.push("strafe_left".to_owned());

    assert_eq!(
        fingerprint(Stage::Rig, &inputs(root, &spec, &library)).unwrap(),
        rig_before,
        "a Mixamo clip is free, so it cannot make the rig stage run again"
    );
    assert_ne!(
        fingerprint(Stage::Bake, &inputs(root, &spec, &library)).unwrap(),
        bake_before,
        "the bake does read it, because it reads one file per animation"
    );
}

#[test]
fn missing_lock_file_starts_empty() {
    let lock = Lock::load(Path::new("/nonexistent/nope.lock")).unwrap();
    assert!(lock.stages.is_empty());
}

/// The pose is prompt text rather than a spec field, so editing it in code has
/// to invalidate the concept, otherwise a T-pose character keeps A-pose art.
#[test]
fn changing_the_pose_invalidates_the_concept() {
    let library = a_library();
    let humanoid = spec();
    let tree = a_tree();
    let root = tree.path();
    let mut other = humanoid.clone();
    other.subject.kind = CharacterType::Quadruped;

    assert_ne!(
        fingerprint(Stage::Concept, &inputs(root, &humanoid, &library)).unwrap(),
        fingerprint(Stage::Concept, &inputs(root, &other, &library)).unwrap(),
        "a different pose instruction must produce a different fingerprint"
    );
}

// --- What the lock reads off disk ----------------------------------------

/// Replacing the canonical rig makes every fitted clip and every baked frame
/// wrong, and costs the paid stages nothing: they never read it. The download
/// stage is one of the two that fit against it.
#[test]
fn one_byte_of_the_canonical_rig_invalidates_the_bake_and_not_the_paid_stages() {
    let library = a_library();
    let spec = spec();
    let tree = a_tree();
    let root = tree.path();
    let before: Vec<String> = Stage::all()
        .iter()
        .map(|stage| fingerprint(*stage, &inputs(root, &spec, &library)).unwrap())
        .collect();

    flip_a_byte(&root.join("art/skeletons/humanoid.glb"));

    let after: Vec<String> = Stage::all()
        .iter()
        .map(|stage| fingerprint(*stage, &inputs(root, &spec, &library)).unwrap())
        .collect();
    for (stage, (before, after)) in Stage::all().iter().zip(before.iter().zip(&after)) {
        match stage {
            Stage::Bake => assert_ne!(before, after, "the bake plays clips fitted to that rig"),
            Stage::Download => assert_ne!(before, after, "the download fits them onto it"),
            _ => assert_eq!(before, after, "{stage} never opens it"),
        }
    }
}

/// The mesh is reconstructed from the four views, so a regenerated view is a
/// different mesh. Nothing after the model stage reads them.
#[test]
fn a_regenerated_concept_view_invalidates_the_model() {
    let library = a_library();
    let spec = spec();
    let tree = a_tree();
    let root = tree.path();
    let before: Vec<String> = Stage::all()
        .iter()
        .map(|stage| fingerprint(*stage, &inputs(root, &spec, &library)).unwrap())
        .collect();

    flip_a_byte(&Paths::new(root, "survivor").concept(View::Left));

    for (stage, before) in Stage::all().iter().zip(&before) {
        let after = fingerprint(*stage, &inputs(root, &spec, &library)).unwrap();
        match stage {
            Stage::Model => assert_ne!(*before, after, "the mesh is built from the views"),
            _ => assert_eq!(*before, after, "{stage} never opens them"),
        }
    }
}

/// The aim table is what the transfer reads, so editing one row refits every
/// clip and re-renders every frame.
#[test]
fn an_aim_table_row_invalidates_the_bake() {
    let library = a_library();
    let spec = spec();
    let tree = a_tree();
    let root = tree.path();
    let before = fingerprint(Stage::Bake, &inputs(root, &spec, &library)).unwrap();

    edit_an_aim_row(root);

    assert_ne!(
        before,
        fingerprint(Stage::Bake, &inputs(root, &spec, &library)).unwrap()
    );
}

/// The bake renders this file, so a `model.glb` that arrived from git is a
/// different character under the same sprites. The paid stages produced it
/// and never read it back.
#[test]
fn a_replaced_character_mesh_invalidates_the_bake_and_nothing_paid() {
    let library = a_library();
    let spec = spec();
    let tree = a_tree();
    let root = tree.path();
    let before: Vec<String> = Stage::all()
        .iter()
        .map(|stage| fingerprint(*stage, &inputs(root, &spec, &library)).unwrap())
        .collect();

    flip_a_byte(&Paths::new(root, "survivor").character_glb());

    for (stage, before) in Stage::all().iter().zip(&before) {
        let after = fingerprint(*stage, &inputs(root, &spec, &library)).unwrap();
        match stage {
            Stage::Bake => assert_ne!(*before, after, "the bake renders it"),
            _ => assert_eq!(*before, after, "{stage} never opens it"),
        }
    }
}

/// Mirroring the mesh changes the body sent to rigging, and nothing else.
#[test]
fn flipping_symmetry_invalidates_the_rig() {
    let library = a_library();
    let spec = spec();
    let tree = a_tree();
    let root = tree.path();
    let mut mirrored = spec.clone();
    mirrored.subject.symmetry = !spec.subject.symmetry;

    assert_ne!(
        fingerprint(Stage::Rig, &inputs(root, &spec, &library)).unwrap(),
        fingerprint(Stage::Rig, &inputs(root, &mirrored, &library)).unwrap(),
        "the fixer mirrors the mesh sent to rigging"
    );
    assert_eq!(
        fingerprint(Stage::Bake, &inputs(root, &spec, &library)).unwrap(),
        fingerprint(Stage::Bake, &inputs(root, &mirrored, &library)).unwrap(),
        "the bake reads the rigged file, not the flag"
    );
}

/// A refetched clip is a different motion under the same name, and the bake
/// is the stage that plays it.
#[test]
fn a_replaced_animation_file_invalidates_the_bake() {
    let library = a_library();
    let spec = spec();
    let tree = a_tree();
    let root = tree.path();
    let before = fingerprint(Stage::Bake, &inputs(root, &spec, &library)).unwrap();

    flip_a_byte(&library.glb(root, "idle"));

    assert_ne!(
        before,
        fingerprint(Stage::Bake, &inputs(root, &spec, &library)).unwrap()
    );
}

/// The scripts are the bake, so a fix to one of them has to re-render.
#[test]
fn editing_a_blender_script_invalidates_the_bake() {
    let library = a_library();
    let spec = spec();
    let tree = a_tree();
    let root = tree.path();
    let before = fingerprint(Stage::Bake, &inputs(root, &spec, &library)).unwrap();

    flip_a_byte(&root.join("tools/blender/src/bake_sprites.py"));

    assert_ne!(
        before,
        fingerprint(Stage::Bake, &inputs(root, &spec, &library)).unwrap()
    );
}

/// The download stage runs the source check and the retarget, so a fix to
/// either script has to re-fit every bought clip. Nothing paid may move.
#[test]
fn editing_a_blender_script_re_fits_the_download_and_buys_nothing() {
    let library = a_library();
    let spec = spec();
    let tree = a_tree();
    let root = tree.path();
    let before = |stage| fingerprint(stage, &inputs(root, &spec, &library)).unwrap();
    let (concept, model, rig, download) = (
        before(Stage::Concept),
        before(Stage::Model),
        before(Stage::Rig),
        before(Stage::Download),
    );

    flip_a_byte(&root.join("tools/blender/src/retarget_animation.py"));

    assert_ne!(before(Stage::Download), download, "the fit is ours to redo");
    assert_eq!(
        before(Stage::Concept),
        concept,
        "a script edit must not bill"
    );
    assert_eq!(before(Stage::Model), model, "a script edit must not bill");
    assert_eq!(before(Stage::Rig), rig, "a script edit must not bill");
}

/// A different Blender renders different frames, and reads no paid stage.
#[test]
fn the_blender_build_invalidates_the_bake_and_nothing_paid() {
    let library = a_library();
    let spec = spec();
    let tree = a_tree();
    let root = tree.path();
    let newer = Build::stated("Blender 5.2.0");
    let other = Inputs {
        root,
        spec: &spec,
        library: &library,
        blender: &newer,
    };

    for stage in Stage::all() {
        let same = fingerprint(stage, &inputs(root, &spec, &library)).unwrap();
        let moved = fingerprint(stage, &other).unwrap();
        match stage {
            Stage::Bake => assert_ne!(same, moved, "the bake is the stage that renders"),
            Stage::Download => assert_ne!(same, moved, "the download is the stage that fits"),
            _ => assert_eq!(same, moved, "{stage} never starts Blender"),
        }
    }
}

/// Every stage that fits or renders reads the same four things, so one
/// `[aim_table]` row invalidates both the fit and the bake.
#[test]
fn a_fit_and_a_bake_read_the_same_rig_and_tooling() {
    let tree = a_tree();
    let root = tree.path();
    let before = blender_inputs(root, HUMANOID, BLENDER).unwrap();

    flip_a_byte(&root.join("art/skeletons/humanoid.glb"));

    assert_ne!(before, blender_inputs(root, HUMANOID, BLENDER).unwrap());
}

/// A file the pipeline has not produced yet is not an error and not a hash of
/// nothing: it reads absent, and starts reading its content the moment the
/// stage that writes it has run.
#[test]
fn a_staging_file_not_produced_yet_reads_as_absent() {
    let library = a_library();
    let spec = spec();
    let tree = a_tree();
    let root = tree.path();
    let paths = Paths::new(root, "survivor");
    let absent = fingerprint(Stage::Rig, &inputs(root, &spec, &library)).unwrap();

    std::fs::create_dir_all(paths.staging()).unwrap();
    std::fs::write(paths.bare_glb(), b"glTF a bare mesh").unwrap();

    assert_ne!(
        absent,
        fingerprint(Stage::Rig, &inputs(root, &spec, &library)).unwrap(),
        "the mesh sent to rigging is what the rig record names"
    );
}

/// A committed input the pipeline cannot run without is an error here rather
/// than a hash of nothing, which every later run would read as unchanged.
#[test]
fn a_missing_committed_input_is_an_error_rather_than_an_empty_hash() {
    let library = a_library();
    let spec = spec();
    let tree = a_tree();
    let root = tree.path();

    std::fs::remove_file(root.join("art/skeletons/humanoid.glb")).unwrap();

    let error = fingerprint(Stage::Bake, &inputs(root, &spec, &library))
        .unwrap_err()
        .to_string();
    assert!(error.contains("humanoid.glb"), "got: {error}");
}

/// The lock the repository ships has to load and answer for every stage,
/// because a parse error would send a reader to delete the task ids a rig
/// was paid for.
///
/// Which stages are stale is not pinned here: T15 regenerates the survivor
/// and rewrites all six. What is pinned is that the answer is complete and
/// in pipeline order, which is what makes `stale.first()` the earliest.
#[test]
fn the_committed_lock_loads_and_answers_for_every_stage() {
    let root = crate::support::repo_root();
    let paths = Paths::new(&root, "survivor");
    let spec = CharacterSpec::load(&paths.spec()).unwrap();
    let library = AnimationLibrary::load(&root).unwrap();
    let lock = Lock::load(&paths.lock()).unwrap();

    let states = lock.states(&inputs(&root, &spec, &library));
    assert_eq!(
        states.keys().copied().collect::<Vec<Stage>>(),
        Stage::all(),
        "every stage answers"
    );
    assert!(
        states.values().all(|state| *state != State::Todo),
        "every stage is recorded: {states:?}"
    );
    let stale = lock::stale(&states);
    assert!(
        stale.windows(2).all(|pair| pair[0] < pair[1]),
        "in pipeline order: {stale:?}"
    );
}
