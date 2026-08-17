use xtask_art::spec::TextureResolution;
use xtask_art::spec::View;
use xtask_art::spec::*;

fn spec() -> CharacterSpec {
    let mut spec = CharacterSpec::template("survivor", CharacterType::Humanoid);
    spec.subject.description = "a lean survivor in torn shorts".to_owned();
    spec
}

#[test]
fn spec_round_trips_through_save_and_load() {
    let dir = std::env::temp_dir().join(format!("marrowfall_spec_{}", std::process::id()));
    let path = dir.join("spec.ron");
    let original = spec();
    original.save(&path).unwrap();

    // Round-trips through the real write path, so a wrong serializer
    // config cannot pass unnoticed.
    let parsed = CharacterSpec::load(&path).unwrap();
    assert_eq!(parsed.name, original.name);
    assert_eq!(parsed.animations.len(), original.animations.len());
    assert_eq!(parsed.subject.kind, CharacterType::Humanoid);
    assert_eq!(parsed.texture.resolution, TextureResolution::K2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn valid_spec_passes() {
    spec().validate().unwrap();
}

#[test]
fn placeholder_description_is_rejected() {
    let spec = CharacterSpec::template("survivor", CharacterType::Humanoid);
    assert!(
        spec.validate()
            .unwrap_err()
            .to_string()
            .contains("placeholder")
    );
}

#[test]
fn only_supported_direction_counts_are_accepted() {
    let mut spec = spec();
    for bad in [0, 3, 6, 7, 12, 16] {
        spec.bake.directions = bad;
        assert!(
            spec.validate().is_err(),
            "{bad} directions should be rejected"
        );
    }
    for good in [4, 8] {
        spec.bake.directions = good;
        spec.validate().unwrap();
    }
}

/// Both ends of Meshy's documented 100..=300000 range. The upper bound is
/// also the rigger's hard limit, so exceeding it fails after payment.
#[test]
fn remesh_target_outside_the_supported_range_is_rejected() {
    let mut spec = spec();
    for bad in [0, 99, 300_001, 400_000] {
        spec.remesh.target = bad;
        assert!(spec.validate().is_err(), "{bad} should be rejected");
    }
    for good in [100, 30_000, 300_000] {
        spec.remesh.target = good;
        spec.validate().unwrap();
    }
}

#[test]
fn duplicate_animation_names_are_rejected() {
    let mut spec = spec();
    spec.animations[1] = spec.animations[0].clone();
    assert!(
        spec.validate()
            .unwrap_err()
            .to_string()
            .contains("duplicate")
    );
}

/// Meshy's rigger only handles bipeds, so anything else must not claim
/// animations it can never receive.
#[test]
fn only_humanoids_can_be_rigged() {
    assert!(CharacterType::Humanoid.can_be_rigged());
    assert!(!CharacterType::Quadruped.can_be_rigged());
    assert!(!CharacterType::Other.can_be_rigged());

    for kind in [CharacterType::Quadruped, CharacterType::Other] {
        let mut spec = spec();
        spec.subject.kind = kind;
        assert!(spec.validate().is_err(), "{kind:?} must reject animations");
        spec.animations.clear();
        spec.validate().unwrap();
    }
}

#[test]
fn template_gives_non_humanoids_no_animations() {
    for kind in [CharacterType::Quadruped, CharacterType::Other] {
        assert!(CharacterSpec::template("x", kind).animations.is_empty());
    }
    assert!(
        !CharacterSpec::template("x", CharacterType::Humanoid)
            .animations
            .is_empty()
    );
}

#[test]
fn uppercase_names_are_rejected() {
    let mut spec = spec();
    spec.name = "Survivor".to_owned();
    assert!(spec.validate().is_err());
}

#[test]
fn paths_are_derived_from_name() {
    let paths = Paths::new("/repo", "skeleton");
    assert!(paths.spec().ends_with("art/characters/skeleton/spec.ron"));
    assert!(paths.lock().ends_with("art/characters/skeleton/spec.lock"));
    assert!(
        paths
            .character_glb()
            .ends_with("art/characters/skeleton/model.glb")
    );
    assert!(
        paths
            .concept(View::Front)
            .ends_with("art/characters/skeleton/concept/front.png")
    );
    // Animations are shared, so they live outside any one character.
    // Derived output stays outside the character's committed directory.
    assert!(paths.staging().ends_with("art/staging/skeleton"));
    assert!(paths.preview().ends_with("art/preview/skeleton"));
}

/// Notes stored in the committed lock must not contain another
/// developer's home directory.
#[test]
fn paths_render_relative_to_the_repo() {
    let paths = Paths::new("/repo", "skeleton");
    assert_eq!(
        paths.relative(&paths.assets()),
        "project/assets/characters/skeleton"
    );
}

#[test]
fn every_body_plan_states_a_pose_and_only_humanoids_can_be_rigged() {
    for kind in [
        CharacterType::Humanoid,
        CharacterType::Quadruped,
        CharacterType::Other,
    ] {
        assert!(!kind.pose_instruction().is_empty(), "{kind:?} has no pose");
    }
    assert!(CharacterType::Humanoid.can_be_rigged());
    assert!(!CharacterType::Quadruped.can_be_rigged());
    assert!(!CharacterType::Other.can_be_rigged());
}
