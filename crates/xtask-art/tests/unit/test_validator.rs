//! The Khronos glTF-Validator gate.
//!
//! The real validator runs here, against the committed art and against a
//! deliberately broken copy of it. A stubbed validator would only prove that
//! our own mapping compiles.

use std::path::Path;

use xtask_art::check::{Comparison, Severity, validator};

use crate::support::{EnvGuard, committed_glb, repo_root};

/// The four GLBs the repository commits. The calibration set: known-good art
/// in, no errors out.
const COMMITTED: [&str; 4] = [
    "art/animations/idle.glb",
    "art/animations/run.glb",
    "art/characters/survivor/model.glb",
    "art/skeletons/humanoid.glb",
];

/// Writes a copy of `glb` with one float in its first float accessor set to
/// NaN. Byte surgery, because the point is a file no exporter would produce.
fn with_an_injected_nan(glb: &Path, out: &Path) {
    let mut bytes = std::fs::read(glb).unwrap();
    assert_eq!(
        &bytes[..4],
        b"glTF",
        "{} is a Git LFS pointer, not a GLB. Run `git lfs pull`, and in CI \
         pass `lfs: true` to actions/checkout.",
        glb.display()
    );
    let json_length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let json_start = 20;
    let bin_start = json_start + json_length + 8;
    let document: serde_json::Value =
        serde_json::from_slice(&bytes[json_start..json_start + json_length]).unwrap();

    let accessor = document["accessors"]
        .as_array()
        .unwrap()
        .iter()
        // 5126 is glTF's FLOAT component type.
        .find(|accessor| accessor["componentType"] == 5126)
        .expect("the committed art has a float accessor");
    let view = &document["bufferViews"][accessor["bufferView"].as_u64().unwrap() as usize];
    let offset = bin_start
        + view["byteOffset"].as_u64().unwrap_or(0) as usize
        + accessor["byteOffset"].as_u64().unwrap_or(0) as usize;

    bytes[offset..offset + 4].copy_from_slice(&f32::NAN.to_le_bytes());
    std::fs::write(out, bytes).unwrap();
}

#[test]
fn the_four_committed_glbs_carry_no_errors() {
    let root = repo_root();
    for file in COMMITTED {
        let findings = validator::validate(&committed_glb(file), &root, 1).unwrap();

        assert!(
            !findings.iter().any(|f| f.severity == Severity::Error),
            "{file}: {findings:#?}"
        );
    }
}

#[test]
fn an_injected_nan_is_an_error() {
    let root = repo_root();
    let dir = tempfile::tempdir().unwrap();
    let broken = dir.path().join("nan.glb");
    with_an_injected_nan(&root.join("art/skeletons/humanoid.glb"), &broken);

    let findings = validator::validate(&broken, &root, 1).unwrap();

    let error = findings
        .iter()
        .find(|f| f.severity == Severity::Error)
        .unwrap_or_else(|| panic!("no error on a NaN: {findings:#?}"));
    assert_eq!(error.rule, "gltf.validator");
    assert!(error.message.contains("NaN"), "got: {}", error.message);
    assert!(
        error.measured_on.contains("glTF-Validator"),
        "the space names the tool and its version: {}",
        error.measured_on
    );
}

#[test]
fn a_missing_file_is_an_error_rather_than_a_skip() {
    let root = repo_root();

    let findings = validator::validate(&root.join("art/nope.glb"), &root, 1).unwrap();

    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].severity, Severity::Error);
    assert_eq!(findings[0].subject, "art/nope.glb");
    assert_eq!(findings[0].comparison, Comparison::Eq);
    // Named as absent, and answered without spending a process on it. The
    // fall-through would report a Node errno string instead.
    assert_eq!(findings[0].unit, "missing files");
    assert_eq!(findings[0].message, "art/nope.glb does not exist");
    assert_eq!(findings[0].measured_on, "the file system");
}

#[test]
fn a_warning_stays_a_warning_and_a_hint_stays_information() {
    let report = r#"{"validatorVersion":"2.0.0-dev.3.10","issues":{"messages":[
        {"code":"ACCESSOR_INVALID_FLOAT","severity":0,"pointer":"/accessors/0",
         "message":"Accessor element at index 0 is NaN."},
        {"code":"NODE_SKINNED_MESH_NON_ROOT","severity":1,"pointer":"/nodes/24",
         "message":"Node with a skinned mesh is not root."},
        {"code":"UNUSED_OBJECT","severity":2,"pointer":"/materials/1",
         "message":"This object may be unused."},
        {"code":"MESH_PRIMITIVE_UNUSED_TEXCOORD","severity":3,"offset":0,
         "message":"Material does not use texture coordinates sets."}]}}"#;

    let findings = validator::findings(report, "humanoid.glb", 1).unwrap();

    let severities: Vec<Severity> = findings.iter().map(|f| f.severity).collect();
    assert_eq!(
        severities,
        [
            Severity::Error,
            Severity::Warning,
            Severity::Info,
            Severity::Info
        ]
    );
    assert_eq!(findings[0].subject, "/accessors/0");
    assert_eq!(
        findings[3].subject, "byte offset 0",
        "a container issue is located by offset, not by pointer"
    );
    assert!(
        findings[1]
            .message
            .starts_with("NODE_SKINNED_MESH_NON_ROOT: "),
        "the stable code leads the message: {}",
        findings[1].message
    );
}

/// A corrupt delivery from Meshy is broken art, not a broken tool, so it
/// belongs in the report the stage boundary reads.
#[test]
fn a_file_that_is_not_gltf_at_all_is_an_error_finding() {
    let root = repo_root();
    let dir = tempfile::tempdir().unwrap();
    let not_gltf = dir.path().join("not.glb");
    std::fs::write(&not_gltf, b"not a gltf at all").unwrap();

    let findings = validator::validate(&not_gltf, &root, 1).unwrap();

    assert_eq!(findings.len(), 1, "{findings:#?}");
    assert_eq!(findings[0].severity, Severity::Error);
    assert_eq!(findings[0].rule, "gltf.validator");
    assert!(
        !findings[0].message.trim().is_empty(),
        "the reason is stated"
    );
    assert!(
        findings[0].measured_on.starts_with("glTF-Validator 2."),
        "one rule, one shape: the version names the tool that refused it, \
         the way a reported issue does. Got: {}",
        findings[0].measured_on
    );
}

/// The other half of that split: a tool we cannot run is not a measurement.
#[test]
fn a_broken_validator_stops_the_run_instead_of_reporting_on_the_art() {
    let root = repo_root();
    let dir = tempfile::tempdir().unwrap();
    let glb = dir.path().join("x.glb");
    std::fs::write(&glb, b"glTF").unwrap();
    let stub = dir.path().join("broken-bun.sh");
    std::fs::write(&stub, "#!/bin/sh\necho 'no such module' >&2\nexit 1\n").unwrap();
    std::fs::set_permissions(
        &stub,
        <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
    )
    .unwrap();
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_BUN_BIN", stub.to_str().unwrap());

    let error = validator::validate(&glb, &root, 1).unwrap_err().to_string();

    assert!(error.contains("bun install"), "got: {error}");
}

#[test]
fn an_issue_the_validator_located_nowhere_is_attributed_to_the_file() {
    let report = r#"{"validatorVersion":"2.0.0","issues":{"messages":[
        {"code":"X","severity":0,"message":"y"}]}}"#;

    let findings = validator::findings(report, "humanoid.glb", 1).unwrap();

    assert_eq!(findings[0].subject, "humanoid.glb");
}

#[test]
fn a_clean_asset_produces_no_findings_at_all() {
    let report = r#"{"validatorVersion":"2.0.0","issues":{"messages":[]}}"#;
    assert!(validator::findings(report, "x.glb", 1).unwrap().is_empty());
}

/// The validator speaking a scale we do not know is not a measurement.
#[test]
fn an_unknown_severity_is_refused_rather_than_defaulted() {
    let report = r#"{"validatorVersion":"2.0.0","issues":{"messages":[
        {"code":"X","severity":9,"pointer":"/x","message":"y"}]}}"#;

    let error = validator::findings(report, "x.glb", 1)
        .unwrap_err()
        .to_string();
    assert!(error.contains("unknown severity 9"), "got: {error}");
}

#[test]
fn an_unparseable_report_is_an_error() {
    assert!(validator::findings("not json", "x.glb", 1).is_err());
}

#[test]
fn a_missing_driver_script_is_reported_by_path() {
    let dir = tempfile::tempdir().unwrap();
    let glb = dir.path().join("x.glb");
    std::fs::write(&glb, b"glTF").unwrap();

    let error = validator::validate(&glb, dir.path(), 1)
        .unwrap_err()
        .to_string();

    assert!(error.contains("validate.mjs"), "got: {error}");
}
