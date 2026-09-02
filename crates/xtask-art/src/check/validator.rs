//! The Khronos glTF-Validator, run through its npm package under Bun.
//!
//! Every issue it reports at `Error` severity is an error for us. There is no
//! Homebrew formula and the published binaries are x64 while our CI runner is
//! arm64, so the npm package is the channel: it is Dart compiled to JS, and
//! therefore architecture independent.

use std::path::Path;
use std::process::Command;

use anyhow::{Context as _, Result, bail, ensure};
use serde::Deserialize;

use super::{Comparison, Finding, Severity};

/// The rule id every finding here carries.
pub const RULE: &str = "gltf.validator";

/// The script that calls `validateBytes` and prints the report.
const DRIVER: &str = "tools/gltf_validator/validate.mjs";

/// What the driver exits with when the file is not glTF at all. That is a
/// broken asset, so it belongs in the report. Any other non-zero code is a
/// broken tool, which is not a measurement.
const NOT_GLTF: i32 = 3;

/// Locates Bun. Overridable for an install outside PATH.
fn bun_binary() -> String {
    std::env::var("MARROWFALL_BUN_BIN").unwrap_or_else(|_| "bun".to_owned())
}

/// Validates one glTF or GLB file.
///
/// A missing file is an error finding rather than a skip, because a gate that
/// goes quiet on absent input proves nothing.
pub fn validate(file: &Path, repo_root: &Path, attempt: u32) -> Result<Vec<Finding>> {
    let subject = relative(file, repo_root);
    if !file.exists() {
        return Ok(vec![unreadable(
            &subject,
            attempt,
            "missing files".to_owned(),
            "the file system".to_owned(),
            format!("{subject} does not exist"),
        )]);
    }

    let driver = repo_root.join(DRIVER);
    ensure!(
        driver.exists(),
        "missing the validator driver at {}",
        driver.display()
    );
    let output = Command::new(bun_binary())
        .arg("run")
        .arg(&driver)
        .arg(file)
        .current_dir(repo_root)
        .output()
        .context("running bun, is it installed? See `setup` in scripts/src/")?;
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if output.status.code() == Some(NOT_GLTF) {
        let message = if stderr.is_empty() {
            format!("{subject} is not glTF")
        } else {
            stderr.clone()
        };
        // There is no report to read the version from, so the driver puts it
        // on stdout instead.
        let version = String::from_utf8_lossy(&output.stdout);
        return Ok(vec![unreadable(
            &subject,
            attempt,
            "unreadable files".to_owned(),
            measured_on(version.trim(), &subject),
            message,
        )]);
    }
    if !output.status.success() {
        bail!(
            "gltf-validator could not run on {subject}. Run `bun install` if \
             node_modules is missing.\n{stderr}"
        );
    }

    findings(&String::from_utf8_lossy(&output.stdout), &subject, attempt)
}

/// A file the validator could not read at all. One error finding, so it
/// lands in the report rather than aborting the whole run.
fn unreadable(
    subject: &str,
    attempt: u32,
    unit: String,
    measured_on: String,
    message: String,
) -> Finding {
    Finding {
        rule: RULE.to_owned(),
        severity: Severity::Error,
        subject: subject.to_owned(),
        measured: 1.0,
        limit: 0.0,
        comparison: Comparison::Eq,
        unit,
        attempt,
        measured_on,
        message,
    }
}

/// Maps a validator report into findings. Pure, so the mapping is tested
/// without spawning anything.
pub fn findings(report: &str, subject: &str, attempt: u32) -> Result<Vec<Finding>> {
    let report: ValidatorReport =
        serde_json::from_str(report).context("parsing the gltf-validator report")?;
    let measured_on = measured_on(&report.validator_version, subject);
    report
        .issues
        .messages
        .into_iter()
        .map(|issue| {
            Ok(Finding {
                rule: RULE.to_owned(),
                severity: severity_of(issue.severity)?,
                subject: issue.subject(subject),
                measured: 1.0,
                limit: 0.0,
                comparison: Comparison::Eq,
                unit: "issues".to_owned(),
                attempt,
                measured_on: measured_on.clone(),
                message: format!("{}: {}", issue.code, issue.message),
            })
        })
        .collect()
}

/// The space a finding was measured in. One shape, whether the validator
/// reported on the file or refused to read it.
fn measured_on(version: &str, subject: &str) -> String {
    format!("glTF-Validator {version} on {subject}, as delivered")
}

/// The validator's own severity scale. An unknown value is refused rather
/// than defaulted, because defaulting it downwards would silence an error.
fn severity_of(severity: u8) -> Result<Severity> {
    match severity {
        0 => Ok(Severity::Error),
        1 => Ok(Severity::Warning),
        2 | 3 => Ok(Severity::Info),
        other => bail!("gltf-validator reported unknown severity {other}"),
    }
}

fn relative(path: &Path, repo_root: &Path) -> String {
    path.strip_prefix(repo_root)
        .unwrap_or(path)
        .display()
        .to_string()
}

#[derive(Deserialize)]
struct ValidatorReport {
    #[serde(rename = "validatorVersion")]
    validator_version: String,
    issues: Issues,
}

#[derive(Deserialize)]
struct Issues {
    messages: Vec<Issue>,
}

#[derive(Deserialize)]
struct Issue {
    code: String,
    severity: u8,
    message: String,
    /// A JSON pointer into the asset. The schema gives this or `offset`.
    pointer: Option<String>,
    /// A byte offset, for issues in the GLB container itself.
    offset: Option<u64>,
}

impl Issue {
    fn subject(&self, file: &str) -> String {
        match (&self.pointer, self.offset) {
            (Some(pointer), _) if !pointer.is_empty() => pointer.clone(),
            (_, Some(offset)) => format!("byte offset {offset}"),
            _ => file.to_owned(),
        }
    }
}
