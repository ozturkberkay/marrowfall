//! Blender as the pipeline uses it: one shell stub that writes the clip, the
//! source motion beside it, the report and the sentinel.
//!
//! Shared, because two paths run the same two scripts: `cargo art fetch`
//! fits a Mixamo clip, and the download stage fits the clips the vendor
//! animated. A stub of one report body per rule the runner owes keeps either
//! from passing on a gate that never reported.

use std::collections::BTreeMap;
use std::path::Path;

use xtask_art::check::profile::Profile;
use xtask_art::check::{Finding, Rule, clip, foot, source};
use xtask_art::library::HUMANOID;

/// One executable stub, named after what it does wrong.
///
/// Every one answers `--version` first, because a fetch reads the Blender
/// build before it fits anything.
pub fn a_stub(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
    let stub = dir.join(name);
    std::fs::write(
        &stub,
        format!("#!/bin/sh\n{}{body}", crate::support::answers_its_version()),
    )
    .unwrap();
    std::fs::set_permissions(
        &stub,
        <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
    )
    .unwrap();
    stub
}

/// Blender as the fetch path uses it: it writes the clip, the source motion
/// beside it, the report and the sentinel.
///
/// The clip and the sidecar are the synthetic cross-rig pair, standing, so
/// every file-side `clip.*` rule measures a real fit rather than a stub
/// string and the three foot contact ones have a foot that plants.
pub fn a_blender_stub(dir: &Path) -> std::path::PathBuf {
    a_blender_stub_of(dir, &crate::clips::CrossRig::new(a_convention()).standing())
}

/// The same stub handing back a pair a test chose.
pub fn a_blender_stub_of(dir: &Path, pair: &crate::clips::CrossRig) -> std::path::PathBuf {
    std::fs::write(dir.join("fitted.glb"), pair.output_glb()).unwrap();
    std::fs::write(dir.join("source.json"), pair.source_motion()).unwrap();
    a_stub(
        dir,
        "blender-stub.sh",
        &format!(
            r#"printf '%s\n' "$@" >> "$MARROWFALL_STUB_ARGV"
out=""; motion=""
while [ $# -gt 0 ]; do
  if [ "$1" = "--out" ]; then out="$2"; fi
  if [ "$1" = "--source-motion" ]; then motion="$2"; fi
  shift
done
if [ -n "$out" ]; then
  mkdir -p "$(dirname "$out")" "$(dirname "$motion")"
  cp "{clip}" "$out"
  cp "{source}" "$motion"
fi
{report}
: > "$MARROWFALL_SENTINEL"
exit 0
"#,
            clip = dir.join("fitted.glb").display(),
            source = dir.join("source.json").display(),
            report = writes_a_report(dir),
        ),
    )
}

/// The canonical role to bone map, out of the committed skeleton file.
pub fn a_convention() -> std::collections::BTreeMap<String, String> {
    let table =
        xtask_art::check::aim::AimTable::of(&crate::support::repo_root(), HUMANOID).unwrap();
    table.bones(table.canonical()).unwrap().clone()
}

/// The report writing, as the real script looks like from outside: the
/// header comes off the path the runner set, and one finding per rule stands
/// in for the 69 the retarget and the 33 the source check really write.
///
/// Each stage defaults to the clean set of every rule it owns, kept on a
/// file, because the runner refuses either script when it leaves one of its
/// own rules unread. A test overrides one stage's whole list.
pub fn writes_a_report(dir: &Path) -> String {
    let fitted = dir.join("retarget-findings.json");
    let vendor = dir.join("fetch-findings.json");
    std::fs::write(&fitted, a_retarget_report(0.0)).unwrap();
    std::fs::write(&vendor, as_findings(&a_source_report())).unwrap();
    format!(
        r#"
name=$(basename "$MARROWFALL_REPORT" .json)
stage=${{name%%.*}}; rest=${{name#*.}}; item=${{rest%.*}}; attempt=${{rest##*.}}
if [ "$stage" = "fetch" ]; then
  if [ -n "$MARROWFALL_STUB_SOURCE_FINDINGS" ]; then
    findings="$MARROWFALL_STUB_SOURCE_FINDINGS"
  else findings=$(cat "{vendor}"); fi
elif [ -n "$MARROWFALL_STUB_FINDINGS" ]; then findings="$MARROWFALL_STUB_FINDINGS"
else findings=$(cat "{fitted}"); fi
printf '{{"stage":"%s","item":"%s","attempt":%s,"findings":[%s]}}' \
  "$stage" "$item" "$attempt" "$findings" > "$MARROWFALL_REPORT"
"#,
        fitted = fitted.display(),
        vendor = vendor.display()
    )
}

/// What the retarget reports, as JSON: one clean finding per rule it owns,
/// with `clip.interpolation` reading `measured`.
///
/// Built through the rule registry, so the stub cannot report a limit, a unit
/// or a space the published list does not carry, and never decides its own
/// severity.
pub fn a_retarget_report(measured: f64) -> String {
    as_findings(&reporting(clip::RETARGET_RULES.iter().copied(), |rule| {
        match rule.id {
            // A clean reading inside every published limit. The one ratio of
            // the seven sits near 1, and everything else reads none of what
            // it measures.
            id if id == clip::INTERPOLATION.id => measured,
            // The one ratio, and the one foot that has to plant at least once.
            id if id == clip::STRIDE_RATIO.id || id == foot::PLANTS.id => 1.0,
            _ => 0.0,
        }
    }))
}

/// What the source check reports: one clean finding per rule it owns, keyed
/// by rule id so a test can swap one for the defect it is about.
pub fn a_source_report() -> BTreeMap<&'static str, Finding> {
    let travel = a_profile().source.travel_meters;
    reporting(source::RULES.iter().copied(), |rule| {
        // `source.traveling` is the one `ge` rule of the six: a clip that
        // travels reads at least the threshold, and every other rule reads
        // none of what it is measuring.
        if rule.id == source::TRAVELING.id {
            travel
        } else {
            0.0
        }
    })
}

/// One finding per rule, each reading whatever `reads` says, so the registry
/// and not the stub decides the severity.
pub fn reporting(
    rules: impl Iterator<Item = &'static Rule>,
    reads: impl Fn(&Rule) -> f64,
) -> BTreeMap<&'static str, Finding> {
    let profile = a_profile();
    rules
        .map(|rule| {
            (
                rule.id,
                rule.measured(&profile, "Hips", reads(rule), 1, "stub".to_owned()),
            )
        })
        .collect()
}

/// A findings list as the body of a JSON array, which is how the stub pastes
/// it into a report.
pub fn as_findings(reported: &BTreeMap<&str, Finding>) -> String {
    reported
        .values()
        .map(|finding| serde_json::to_string(finding).unwrap())
        .collect::<Vec<String>>()
        .join(",")
}

pub fn a_profile() -> Profile {
    Profile::of(&crate::support::repo_root(), HUMANOID).unwrap()
}
