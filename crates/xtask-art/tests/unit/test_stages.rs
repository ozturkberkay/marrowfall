use crate::support::a_library;
use std::path::Path;
use xtask_art::spec::{CharacterSpec, CharacterType, Paths};
use xtask_art::stages::*;

#[test]
fn non_humanoids_skip_rigging() {
    assert!(!CharacterType::Quadruped.can_be_rigged());
    assert!(!CharacterType::Other.can_be_rigged());
    assert!(CharacterType::Humanoid.can_be_rigged());
}

#[test]
fn bake_requires_the_script() {
    let spec = CharacterSpec::template("x", CharacterType::Humanoid);
    let paths = Paths::new("/nonexistent", "x");
    let error = bake(&spec, &a_library(), &paths, Path::new("/nonexistent")).unwrap_err();
    assert!(error.to_string().contains("bake script"));
}
