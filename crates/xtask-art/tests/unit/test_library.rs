//! The shared animation library.

use xtask_art::check::profile::Profile;
use xtask_art::check::{Artifacts, Report, Severity, clip};
use xtask_art::library::{
    Animation, AnimationLibrary, ClipFiles, HUMANOID, LibraryLock, MotionSource, Verdict,
};

use crate::support::repo_root;

#[test]
fn a_project_with_no_library_yet_reads_as_empty() {
    let dir = tempfile::tempdir().unwrap();
    let library = AnimationLibrary::load(dir.path()).unwrap();
    assert!(library.animations.is_empty());
}

#[test]
fn a_library_round_trips_through_save_and_load() {
    let dir = tempfile::tempdir().unwrap();
    let library = AnimationLibrary::template();
    library.save(dir.path()).unwrap();

    let loaded = AnimationLibrary::load(dir.path()).unwrap();
    assert_eq!(loaded.animations, library.animations);
}

#[test]
fn the_saved_library_ends_with_a_newline() {
    let dir = tempfile::tempdir().unwrap();
    AnimationLibrary::template().save(dir.path()).unwrap();
    let text = std::fs::read_to_string(AnimationLibrary::path(dir.path())).unwrap();
    assert!(
        text.ends_with('\n'),
        "otherwise the eof pre-commit hook trips"
    );
}

#[test]
fn a_corrupt_library_names_the_file_it_could_not_parse() {
    let dir = tempfile::tempdir().unwrap();
    let path = AnimationLibrary::path(dir.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "not ron at all").unwrap();

    let error = AnimationLibrary::load(dir.path()).unwrap_err().to_string();
    assert!(error.contains("library.ron"), "got: {error}");
}

#[test]
fn animations_live_outside_any_one_character() {
    let glb = AnimationLibrary::template().glb(std::path::Path::new("/repo"), "run");
    assert!(glb.ends_with("art/animations/run.glb"), "{glb:?}");
    assert!(
        !glb.starts_with("/repo/art/characters"),
        "a shared animation must not be filed under a character: {glb:?}"
    );
}

#[test]
fn an_unknown_name_lists_what_is_available() {
    let library = AnimationLibrary::template();
    let error = library.get("moonwalk").unwrap_err().to_string();
    assert!(error.contains("moonwalk"), "names the miss: {error}");
    assert!(error.contains("idle"), "lists the alternatives: {error}");
    assert!(
        error.contains("walk_back"),
        "lists the alternatives: {error}"
    );
}

#[test]
fn an_empty_library_says_so_rather_than_listing_nothing() {
    let error = AnimationLibrary::default()
        .get("idle")
        .unwrap_err()
        .to_string();
    assert!(error.contains("none yet"), "got: {error}");
}

#[test]
fn resolving_preserves_the_order_the_spec_asked_for() {
    let library = AnimationLibrary::template();
    let names = vec!["run".to_owned(), "idle".to_owned()];
    let resolved = library.resolve(&names, HUMANOID).unwrap();

    // Packing keys the character's scale to the first entry, so order is not
    // cosmetic.
    assert_eq!(
        resolved.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
        ["run", "idle"]
    );
    assert_eq!(resolved[0].1.source, MotionSource::Meshy { action_id: 15 });
}

#[test]
fn resolving_fails_on_the_first_name_that_is_not_declared() {
    let library = AnimationLibrary::template();
    let names = vec!["idle".to_owned(), "backflip".to_owned()];
    let error = library.resolve(&names, HUMANOID).unwrap_err().to_string();
    assert!(error.contains("backflip"), "got: {error}");
}

#[test]
fn a_library_declares_each_motion_once() {
    let library = AnimationLibrary::template();
    let ids: Vec<u32> = library
        .animations
        .values()
        .filter_map(|animation| match animation.source {
            MotionSource::Meshy { action_id } => Some(action_id),
            MotionSource::Mixamo { .. } | MotionSource::Authored => None,
        })
        .collect();
    let unique: std::collections::HashSet<u32> = ids.iter().copied().collect();
    assert_eq!(
        ids.len(),
        unique.len(),
        "two names for one Meshy action would buy the same motion twice"
    );
}

#[test]
fn locomotion_loops() {
    let library = AnimationLibrary::template();
    for name in ["idle", "run", "walk_back"] {
        assert!(library.get(name).unwrap().loops, "{name} should loop");
    }
}

#[test]
fn an_animation_describes_the_motion_and_nothing_about_a_character() {
    let animation = Animation {
        skeleton: HUMANOID.to_owned(),
        loops: true,
        fps: 12,
        source_fps: 24,
        travels: false,
        source: MotionSource::Meshy { action_id: 251 },
    };
    assert_eq!(animation.source, MotionSource::Meshy { action_id: 251 });
    assert!(animation.loops);
    assert_eq!(animation.skeleton, "humanoid");
}

#[test]
fn only_a_provider_sourced_motion_costs_anything() {
    assert!(MotionSource::Meshy { action_id: 1 }.costs_credits());
    assert!(
        !MotionSource::Authored.costs_credits(),
        "hand-built motion is committed, not bought"
    );
    assert!(
        !a_mixamo_source().costs_credits(),
        "Mixamo animations are free"
    );
}

fn a_mixamo_source() -> MotionSource {
    MotionSource::Mixamo {
        product_id: "c9ccc468-b96c-11e4-a802-0aaa78deedf9".to_owned(),
    }
}

/// A library holding one clip of each provenance.
fn a_mixed_library() -> AnimationLibrary {
    let mut library = AnimationLibrary::template();
    library.animations.insert(
        "strafe_left".to_owned(),
        Animation {
            skeleton: HUMANOID.to_owned(),
            loops: true,
            fps: 24,
            source_fps: 30,
            travels: true,
            source: a_mixamo_source(),
        },
    );
    library
}

#[test]
fn only_motion_we_own_may_be_published_with_the_art() {
    // Meshy's plan hands ownership over and those clips cost credits, so
    // committing them stops contributors buying them again. Adobe forbids
    // redistributing the file itself, and this repository is public.
    assert!(MotionSource::Meshy { action_id: 1 }.redistributable());
    assert!(MotionSource::Authored.redistributable());
    assert!(!a_mixamo_source().redistributable());
}

/// Two rigs can use one bone name for different bones, so the retarget is told
/// which convention a file uses rather than guessing from the names in it.
///
/// What it names is the file the vendor delivers, which is what the source
/// check and the retarget open: Meshy animates the rig it sold, Mixamo ships
/// its own FBX, and authored motion is made on ours. The committed copy is in
/// the standard convention either way, and `[fingerprints]` says so.
#[test]
fn each_provider_declares_how_it_names_bones() {
    assert_eq!(a_mixamo_source().bone_convention(), "mixamo");
    assert_eq!(
        MotionSource::Meshy { action_id: 1 }.bone_convention(),
        "meshy"
    );
    assert_eq!(
        MotionSource::Authored.bone_convention(),
        "standard",
        "hand-built motion is made on the canonical rig"
    );
}

#[test]
fn motion_we_may_not_publish_lands_outside_the_committed_library() {
    let library = a_mixed_library();
    let root = std::path::Path::new("/repo");

    let committed = library.glb(root, "run");
    let local = library.glb(root, "strafe_left");
    assert!(
        committed.ends_with("art/animations/run.glb"),
        "{committed:?}"
    );
    assert!(
        local.ends_with("art/animations/local/strafe_left.glb"),
        "a clip we may not redistribute must not sit with the committed art: {local:?}"
    );
}

#[test]
fn a_mixamo_entry_round_trips_through_ron() {
    let dir = tempfile::tempdir().unwrap();
    a_mixed_library().save(dir.path()).unwrap();

    let loaded = AnimationLibrary::load(dir.path()).unwrap();
    assert_eq!(loaded.get("strafe_left").unwrap().source, a_mixamo_source());
}

#[test]
fn the_canonical_rig_is_named_after_the_skeleton_it_defines() {
    // A second skeleton is a second file and no code change.
    let rig = AnimationLibrary::reference_rig(std::path::Path::new("/repo"), HUMANOID);
    assert!(rig.ends_with("art/skeletons/humanoid.glb"), "{rig:?}");
}

/// One directory for every provider's own download, and the extension is the
/// provider's: Mixamo exports FBX and Meshy delivers GLB.
#[test]
fn a_download_is_staged_where_derived_art_goes() {
    let root = std::path::Path::new("/repo");
    for (extension, tail) in [
        ("fbx", "art/staging/downloads/walk_back.fbx"),
        ("glb", "art/staging/downloads/walk_back.glb"),
    ] {
        let staged = AnimationLibrary::staged_download(root, "walk_back", extension);
        assert!(
            staged.ends_with(tail),
            "gitignored, so a provider's own file is never committed: {staged:?}"
        );
    }
}

// --- The library lock -----------------------------------------------------

/// Where one retarget attempt's files belong, which is what names a verdict.
fn an_artifact_set(clip: &str, attempt: u32) -> Artifacts {
    Artifacts::new(std::path::Path::new("/repo"), "retarget", clip, attempt).unwrap()
}

/// One stored verdict, as a passing retarget writes it.
fn a_verdict() -> Verdict {
    Verdict {
        report: "retarget.strafe_left.1".to_owned(),
        worst: Severity::Info,
        rules: vec!["clip.interpolation".to_owned()],
    }
}

#[test]
fn nothing_fetched_yet_reads_as_an_empty_record() {
    let dir = tempfile::tempdir().unwrap();
    assert!(LibraryLock::load(dir.path()).unwrap().fetched.is_empty());
}

#[test]
fn a_fetch_record_round_trips_and_ends_with_a_newline() {
    let dir = tempfile::tempdir().unwrap();
    let mut lock = LibraryLock::default();
    lock.record(
        "strafe_left",
        a_mixamo_source(),
        ClipFiles {
            download: b"the download",
            glb: b"the glb",
        },
        "0123456789abcdef",
        a_verdict(),
    );
    lock.save(dir.path()).unwrap();

    let text = std::fs::read_to_string(LibraryLock::path(dir.path())).unwrap();
    assert!(
        text.ends_with('\n'),
        "otherwise the eof pre-commit hook trips"
    );
    let loaded = LibraryLock::load(dir.path()).unwrap();
    assert_eq!(loaded.fetched["strafe_left"], lock.fetched["strafe_left"]);
}

#[test]
fn a_fetch_records_both_what_arrived_and_what_was_kept() {
    let mut lock = LibraryLock::default();
    lock.record(
        "strafe_left",
        a_mixamo_source(),
        ClipFiles {
            download: b"the download",
            glb: b"the glb",
        },
        "0123456789abcdef",
        a_verdict(),
    );
    let fetched = &lock.fetched["strafe_left"];

    assert_eq!(fetched.source, a_mixamo_source());
    assert_ne!(
        fetched.download, fetched.glb,
        "the upstream file and what we kept are different things"
    );
    assert_eq!(
        fetched.glb.len(),
        16,
        "the fingerprint the locks already use"
    );
}

#[test]
fn a_corrupt_fetch_record_says_how_to_start_over() {
    let dir = tempfile::tempdir().unwrap();
    let path = LibraryLock::path(dir.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "not ron at all").unwrap();

    let error = LibraryLock::load(dir.path()).unwrap_err().to_string();
    assert!(error.contains("library.lock"), "got: {error}");
    assert!(error.contains("re-fetch"), "got: {error}");
}

/// The vendor file is not committed, so the record in `library.lock` is the
/// only proof an uncommitted input passed its gates.
#[test]
fn a_fetch_records_the_verdict_the_retarget_reached() {
    let profile = Profile::of(&repo_root(), HUMANOID).unwrap();
    let mut report = Report::new("retarget", "strafe_left", 1);
    report
        .add(clip::INTERPOLATION.measured(&profile, "Hips", 0.0, 1, "clean".to_owned()))
        .unwrap();
    report
        .add(clip::LOOP.skipped(&profile, "Hips", 1, "not a loop".to_owned()))
        .unwrap();
    let verdict = Verdict::of(&report, &an_artifact_set("strafe_left", 1));

    assert_eq!(verdict.worst, Severity::Info);
    assert_eq!(
        verdict.report, "retarget.strafe_left.1",
        "which names the attempt too"
    );
    assert_eq!(
        verdict.rules,
        vec![clip::INTERPOLATION.id.to_owned(), clip::LOOP.id.to_owned()],
        "every rule that reported, so one that went quiet is visible here too"
    );
    assert!(!verdict.failed());
}

/// A verdict is what a re-fetch decision reads, so failing has to be a
/// question anything can ask it.
#[test]
fn a_verdict_that_failed_its_gates_says_so() {
    let mut report = Report::new("retarget", "strafe_left", 2);
    report
        .add(clip::INTERPOLATION.measured(
            &Profile::of(&repo_root(), HUMANOID).unwrap(),
            "Hips",
            99.0,
            2,
            "four channels left on Bezier".to_owned(),
        ))
        .unwrap();
    let verdict = Verdict::of(&report, &an_artifact_set("strafe_left", 2));

    assert_eq!(verdict.worst, Severity::Error);
    assert!(verdict.failed());
}

#[test]
fn a_fetch_record_carries_the_rig_it_was_fitted_to() {
    let mut lock = LibraryLock::default();
    lock.record(
        "strafe_left",
        a_mixamo_source(),
        ClipFiles {
            download: b"the download",
            glb: b"the glb",
        },
        "0123456789abcdef",
        a_verdict(),
    );

    assert_eq!(lock.fetched["strafe_left"].fingerprint, "0123456789abcdef");
    assert_eq!(lock.fetched["strafe_left"].verdict, Some(a_verdict()));
}

/// A record written before either field existed loads, and reads as having
/// no verdict, which is what sends it back to be fetched again.
#[test]
fn a_record_from_before_the_verdict_loads_without_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = LibraryLock::path(dir.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        r#"LibraryLock(fetched: {"strafe_left": Fetched(
            source: Mixamo(product_id: "c9c97b90-b96c-11e4-a802-0aaa78deedf9"),
            download: "aaaaaaaaaaaaaaaa",
            glb: "bbbbbbbbbbbbbbbb",
        )})
"#,
    )
    .unwrap();

    let loaded = LibraryLock::load(dir.path()).unwrap();
    assert_eq!(loaded.fetched["strafe_left"].verdict, None);
    assert!(loaded.fetched["strafe_left"].fingerprint.is_empty());
}

#[test]
fn the_skeleton_name_carries_no_vendor() {
    // Meshy happens to produce this skeleton today; the name outlives it.
    assert_eq!(HUMANOID, "humanoid");
    assert_eq!(xtask_art::providers::meshy::RIGS, HUMANOID);
}

#[test]
fn an_animation_for_another_skeleton_is_refused() {
    let mut library = AnimationLibrary::template();
    library.animations.insert(
        "chitter".to_owned(),
        Animation {
            skeleton: "insectoid".to_owned(),
            loops: true,
            fps: 12,
            source_fps: 24,
            travels: false,
            source: MotionSource::Authored,
        },
    );

    let names = vec!["chitter".to_owned()];
    let error = library.resolve(&names, HUMANOID).unwrap_err().to_string();
    assert!(error.contains("insectoid"), "names the mismatch: {error}");
    assert!(error.contains("bone names"), "explains why: {error}");

    // ...and is fine for a character actually rigged that way.
    library.resolve(&names, "insectoid").unwrap();
}

#[test]
fn animations_can_be_listed_per_skeleton() {
    let mut library = AnimationLibrary::template();
    library.animations.insert(
        "chitter".to_owned(),
        Animation {
            skeleton: "insectoid".to_owned(),
            loops: true,
            fps: 12,
            source_fps: 24,
            travels: false,
            source: MotionSource::Authored,
        },
    );

    let humanoid: Vec<&str> = library.for_skeleton(HUMANOID).collect();
    assert_eq!(humanoid, ["idle", "run", "walk_back"]);
    assert_eq!(
        library.for_skeleton("insectoid").collect::<Vec<_>>(),
        ["chitter"]
    );
}

// --- the committed library ------------------------------------------------

/// Every `source_fps` and every `travels` on record, and the two product ids
/// a fetch spends a Mixamo session on. All of them are measurements, so a
/// silent edit here is a clip fitted at the wrong rate or a gate switched off.
#[test]
fn the_committed_animation_library_loads() {
    let library = AnimationLibrary::load(&crate::support::repo_root()).unwrap();

    assert_eq!(library.animations.len(), 5);
    assert_eq!(
        library.get("strafe_left").unwrap().source,
        MotionSource::Mixamo {
            product_id: "c9c97b90-b96c-11e4-a802-0aaa78deedf9".to_owned()
        }
    );
    assert_eq!(
        library.get("strafe_right").unwrap().source,
        MotionSource::Mixamo {
            product_id: "c9c96f9e-b96c-11e4-a802-0aaa78deedf9".to_owned()
        }
    );
    // Measured: key spacing for the rate, hips first frame to last for the
    // flag. The design document records what each was read on.
    for (name, source_fps, travels) in [
        ("idle", 24, false),
        ("run", 24, false),
        ("walk_back", 24, true),
        ("strafe_left", 30, true),
        ("strafe_right", 30, true),
    ] {
        let animation = library.get(name).unwrap();
        assert_eq!((name, animation.source_fps), (name, source_fps));
        assert_eq!((name, animation.travels), (name, travels));
    }
}
