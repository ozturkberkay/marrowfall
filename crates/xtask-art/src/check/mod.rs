//! The gate harness. One record, one report, one exit-code contract.
//!
//! Every gate in the pipeline emits a [`Finding`]. Rust builds them directly,
//! the Blender scripts write the same JSON through
//! `tools/blender/src/findings.py`, and [`Report::read`] parses those back, so
//! one contract covers both sides.
//!
//! Nothing here decides *whether* a rule is worth running. A gate measures,
//! records the space it measured in, and states the limit it was read
//! against. The runner only counts errors.

pub mod aim;
pub mod atlas;
pub mod bake;
pub mod clip;
pub mod concept;
pub mod foot;
pub mod gltf_clip;
pub mod gltf_mesh;
pub mod gltf_world;
pub mod mesh;
pub mod motion;
pub mod profile;
pub mod rig;
pub mod source;
pub mod validator;

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, ensure};
use serde::{Deserialize, Serialize};

use profile::Profile;

/// Derived, gitignored, one directory for the whole pipeline.
const REPORTS_DIR: &str = "art/staging/reports";

/// A path as a finding names it: relative to the repository, so a report
/// reads the same on every machine.
pub fn relative_to(path: &Path, repo_root: &Path) -> String {
    path.strip_prefix(repo_root)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// How bad a finding is. Only [`Severity::Error`] can stop the pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    /// Measured, and inside its limit. Every rule reports its measurement,
    /// because a rule that goes quiet when it passes cannot be told from one
    /// that never ran.
    Info,
    /// Not measured, because a declaration switched this rule off for this
    /// subject: a spec field, a `travels` flag, a clip the library does not
    /// call a loop, `--keep-root-motion`. The measurement carries nothing and
    /// only the message says why, so it is a severity of its own rather than
    /// an `info` a consumer would have to tell apart by reading prose.
    Skipped,
}

impl Severity {
    /// How bad this is, for picking the worst of a report. A skip is the
    /// lowest, because it took no measurement.
    const fn rank(self) -> u8 {
        match self {
            Self::Skipped => 0,
            Self::Info => 1,
            Self::Warning => 2,
            Self::Error => 3,
        }
    }
}

/// Whether a character is declared bilaterally symmetric.
///
/// Per character, not global: a monster can be asymmetric on purpose, and a
/// one-armed thing with a tail must not fail a rule written for a human.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Symmetry {
    /// `spec.subject.symmetry` is true: the fixer mirrors the mesh and every
    /// mirror rule measures it.
    Enforced,
    /// It is false: nothing mirrors the character and every mirror rule
    /// reports [`Severity::Skipped`] on that declaration.
    Declined,
}

impl Symmetry {
    pub const fn declared(symmetry: bool) -> Self {
        if symmetry {
            Self::Enforced
        } else {
            Self::Declined
        }
    }
}

/// The largest angle two directions can be apart.
///
/// The ceiling every recording angle rule reads against, because a finding
/// still needs a limit. It cannot be crossed, so the severity is always
/// `info` and the number is always on record.
pub const HALF_A_TURN: f64 = 180.0;

/// What every mirror rule files when a spec declines symmetry. One sentence,
/// so the mesh side and the rig side say the same thing.
pub const NOT_MIRRORED: &str = "spec.subject.symmetry is false, so this character is not mirrored \
                                and nothing reads it against its own reflection";

/// How a measurement is read against its limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Comparison {
    /// At most.
    Le,
    /// Strictly less than.
    Lt,
    /// Exactly.
    Eq,
    /// At least.
    Ge,
}

impl Comparison {
    /// The spelling both sides use on the wire, which is also what
    /// `--list-rules` prints.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Le => "le",
            Self::Lt => "lt",
            Self::Eq => "eq",
            Self::Ge => "ge",
        }
    }

    pub fn holds(self, measured: f64, limit: f64) -> bool {
        match self {
            Self::Le => measured <= limit,
            Self::Lt => measured < limit,
            Self::Eq => measured == limit,
            Self::Ge => measured >= limit,
        }
    }
}

/// One measurement against one limit.
///
/// `comparison` is required: without it `mesh.cleanup_effective` passes a
/// fixer that changed nothing, at 8 le 8. `measured_on` is required too,
/// because precise numbers on the wrong representation is how 171 holes read
/// as 13,368.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    /// Stable rule id, such as `clip.swing`.
    pub rule: String,
    pub severity: Severity,
    /// Bone, file, frame, object or direction.
    pub subject: String,
    pub measured: f64,
    pub limit: f64,
    pub comparison: Comparison,
    pub unit: String,
    /// Which regeneration produced this.
    pub attempt: u32,
    /// The representation and the space, named.
    pub measured_on: String,
    pub message: String,
}

impl Finding {
    /// Whether the measurement is inside its limit.
    pub fn holds(&self) -> bool {
        self.comparison.holds(self.measured, self.limit)
    }

    fn validate(&self) -> Result<()> {
        for (field, value) in [
            ("rule", &self.rule),
            ("subject", &self.subject),
            ("unit", &self.unit),
            ("measured_on", &self.measured_on),
            ("message", &self.message),
        ] {
            ensure!(
                !value.trim().is_empty(),
                "{}: {field} must not be empty",
                self.rule
            );
        }
        // A gate reports an undefined measurement as an error with a stated
        // message. NaN would ride through every comparison as "not worse".
        for (field, value) in [("measured", self.measured), ("limit", self.limit)] {
            ensure!(
                value.is_finite(),
                "{}: {field} must be finite, got {value}",
                self.rule
            );
        }
        ensure!(self.attempt >= 1, "{}: the first attempt is 1", self.rule);
        Ok(())
    }
}

/// One gate, as `cargo art check --list-rules` prints it.
///
/// Findings are built through here rather than by hand, so a rule cannot
/// report a limit, a unit or a space that the printed list does not carry,
/// and no caller decides whether its own measurement passed.
pub struct Rule {
    /// Stable id, such as `rig.child_axis`.
    pub id: &'static str,
    pub comparison: Comparison,
    pub unit: &'static str,
    /// The representation and the space. Every finding of this rule repeats
    /// it, because a precise number on the wrong representation is how 171
    /// holes read as 13,368.
    pub space: &'static str,
    /// Every published limit is profile data, so the rule reads it rather
    /// than holding a copy. One rule publishes none and answers `NaN`, which
    /// [`Rule::publishes_a_limit`] is how anything else asks.
    pub limit: fn(&Profile) -> f64,
}

impl Rule {
    /// One measurement of this rule. The comparison decides the severity, so
    /// a broken measurement cannot be filed as information.
    pub fn measured(
        &self,
        profile: &Profile,
        subject: &str,
        measured: f64,
        attempt: u32,
        message: String,
    ) -> Finding {
        let limit = (self.limit)(profile);
        Finding {
            rule: self.id.to_owned(),
            severity: if self.comparison.holds(measured, limit) {
                Severity::Info
            } else {
                Severity::Error
            },
            subject: subject.to_owned(),
            measured,
            limit,
            comparison: self.comparison,
            unit: self.unit.to_owned(),
            attempt,
            measured_on: self.space.to_owned(),
            message,
        }
    }

    /// One measurement read against a limit this subject sets rather than the
    /// profile: what a fixer left, against what it was given.
    ///
    /// Everything else is as [`Rule::measured`], the comparison included, so
    /// a fixer that changed nothing cannot file its own result as
    /// information.
    pub fn against(
        &self,
        subject: &str,
        measured: f64,
        limit: f64,
        attempt: u32,
        message: String,
    ) -> Finding {
        Finding {
            rule: self.id.to_owned(),
            severity: if self.comparison.holds(measured, limit) {
                Severity::Info
            } else {
                Severity::Error
            },
            subject: subject.to_owned(),
            measured,
            limit,
            comparison: self.comparison,
            unit: self.unit.to_owned(),
            attempt,
            measured_on: self.space.to_owned(),
            message,
        }
    }

    /// Whether the profile publishes this rule's limit.
    ///
    /// One rule does not: `mesh.cleanup_effective` publishes no number and
    /// answers `false` here, which is what lets [`Report::off_registry`] read
    /// the limit its findings carry and what `--list-rules` prints instead of
    /// a figure.
    pub fn publishes_a_limit(&self, profile: &Profile) -> bool {
        (self.limit)(profile).is_finite()
    }

    /// The limit column `--list-rules` prints: a number, or the word
    /// `before` for the one rule read against the subject's own count as it
    /// stood before a fixer ran.
    pub fn printed_limit(&self, profile: &Profile) -> String {
        if self.publishes_a_limit(profile) {
            (self.limit)(profile).to_string()
        } else {
            "before".to_owned()
        }
    }

    /// A rule a declaration switched off for this subject, which is the one
    /// thing a number cannot say. It still carries this rule's own limit, so
    /// the registry reads it like any other finding, and only the message
    /// says why nothing was measured.
    ///
    /// The rule that publishes no limit files zero: a skip took no
    /// measurement, so there is nothing for one to be read against.
    pub fn skipped(
        &self,
        profile: &Profile,
        subject: &str,
        attempt: u32,
        message: String,
    ) -> Finding {
        Finding {
            rule: self.id.to_owned(),
            severity: Severity::Skipped,
            subject: subject.to_owned(),
            measured: 0.0,
            limit: if self.publishes_a_limit(profile) {
                (self.limit)(profile)
            } else {
                0.0
            },
            comparison: self.comparison,
            unit: self.unit.to_owned(),
            attempt,
            measured_on: self.space.to_owned(),
            message,
        }
    }

    /// A measurement that does not exist: a bone with no length, an axis with
    /// no direction. It carries its own unit, the way an unreadable file does
    /// in [`validator`], because this rule's unit would be a lie. Always an
    /// error, because a gate never emits NaN and never goes quiet.
    pub fn undefined(&self, subject: &str, attempt: u32, message: String) -> Finding {
        Finding {
            rule: self.id.to_owned(),
            severity: Severity::Error,
            subject: subject.to_owned(),
            measured: 1.0,
            limit: 0.0,
            comparison: Comparison::Eq,
            unit: UNDEFINED_UNIT.to_owned(),
            attempt,
            measured_on: self.space.to_owned(),
            message,
        }
    }
}

/// The unit an undefined measurement carries, on both sides.
/// `findings.py::Rule.undefined` writes the same words.
pub const UNDEFINED_UNIT: &str = "undefined measurements";

/// Every rule the pipeline publishes, in the order `--list-rules` prints
/// them. One list, so a family cannot ship with its rules unprintable.
pub fn every_rule() -> impl Iterator<Item = &'static Rule> {
    concept::RULES
        .into_iter()
        .chain(rig::RULES)
        .chain(aim::RULES)
        .chain(mesh::RULES)
        .chain(source::RULES)
        .chain(clip::RULES)
        .chain(foot::RULES)
        .chain(bake::RULES)
        .chain(atlas::RULES)
}

/// Every finding one stage attempt produced, for one item.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    stage: String,
    /// What the stage ran on: a character, a clip. One stage runs many times.
    item: String,
    attempt: u32,
    findings: Vec<Finding>,
}

impl Report {
    pub fn new(stage: &str, item: &str, attempt: u32) -> Self {
        Self {
            stage: stage.to_owned(),
            item: item.to_owned(),
            attempt,
            findings: Vec::new(),
        }
    }

    /// The only way a finding enters a report, so the contract cannot be
    /// side-stepped by building the struct directly.
    pub fn add(&mut self, finding: Finding) -> Result<()> {
        finding.validate()?;
        ensure!(
            finding.attempt == self.attempt,
            "{} is from attempt {}, this report is attempt {}",
            finding.rule,
            finding.attempt,
            self.attempt
        );
        self.findings.push(finding);
        Ok(())
    }

    pub fn extend(&mut self, findings: impl IntoIterator<Item = Finding>) -> Result<()> {
        findings.into_iter().try_for_each(|f| self.add(f))
    }

    pub fn stage(&self) -> &str {
        &self.stage
    }

    pub fn item(&self) -> &str {
        &self.item
    }

    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }

    /// The worst thing this report says. A report with nothing in it says a
    /// skip: nothing was measured.
    pub fn worst(&self) -> Severity {
        self.findings
            .iter()
            .map(|finding| finding.severity)
            .max_by_key(|severity| severity.rank())
            .unwrap_or(Severity::Skipped)
    }

    pub fn has_errors(&self) -> bool {
        self.findings.iter().any(|f| f.severity == Severity::Error)
    }

    /// Every way this report disagrees with the rule list `--list-rules`
    /// prints, one complaint per finding.
    ///
    /// The Blender scripts build their Findings by hand, so this is what
    /// holds them to the registry. A rule with no entry has no published
    /// limit for anyone to read it against, and a finding that keeps the
    /// id and changes the comparison is worse than a missing one: it is
    /// `mesh.cleanup_effective` passing a fixer that changed nothing, at
    /// 8 le 8.
    ///
    /// The severity is checked the same way, because the comparison decides
    /// it and never the script that measured. Without that, four channels
    /// left on Bezier filed as `info` reads 4 eq 0 and still exits 0, and
    /// the only gate the retarget has is one word in a JSON file.
    pub fn off_registry(&self, profile: &Profile) -> Vec<String> {
        self.findings
            .iter()
            .filter_map(
                |finding| match every_rule().find(|r| r.id == finding.rule) {
                    None => Some(format!(
                        "{} is not a rule `--list-rules` prints",
                        finding.rule
                    )),
                    Some(rule) => Self::disagreement(finding, rule, profile),
                },
            )
            .collect()
    }

    fn disagreement(finding: &Finding, rule: &Rule, profile: &Profile) -> Option<String> {
        // An undefined measurement is the one shape that cannot carry its
        // rule's unit or limit: an angle that does not exist is not 180
        // degrees. Both sides write it identically, so it agrees with any
        // rule read on the same space.
        if Self::undefined_shape(finding) && finding.measured_on == rule.space {
            return None;
        }
        let limit = (rule.limit)(profile);
        let wrong = [
            ("comparison", finding.comparison != rule.comparison),
            ("unit", finding.unit != rule.unit),
            (
                "limit",
                rule.publishes_a_limit(profile) && finding.limit != limit,
            ),
            ("measured_on", finding.measured_on != rule.space),
            ("severity", !Self::severity_follows(finding)),
        ]
        .into_iter()
        .filter_map(|(field, differs)| differs.then_some(field))
        .collect::<Vec<&str>>();
        (!wrong.is_empty()).then(|| {
            format!(
                "{} on {} reports a {} the rule list does not carry: \
                 {:?} at {} {} {} {} {:?} against {} {} {} {:?}",
                finding.rule,
                finding.subject,
                wrong.join(" and a "),
                finding.severity,
                finding.measured,
                finding.comparison.as_str(),
                finding.limit,
                finding.unit,
                finding.measured_on,
                rule.comparison.as_str(),
                limit,
                rule.unit,
                rule.space,
            )
        })
    }

    /// Whether a finding is the record [`Rule::undefined`] writes, down to
    /// the severity: anything softer than an error would be a gate going
    /// quiet on a measurement it could not take.
    fn undefined_shape(finding: &Finding) -> bool {
        finding.severity == Severity::Error
            && finding.comparison == Comparison::Eq
            && finding.limit == 0.0
            && finding.unit == UNDEFINED_UNIT
    }

    /// Whether a finding's severity is the one its own comparison gives.
    ///
    /// A warning says a measurement could not be taken and a skip says a
    /// declaration switched the rule off, so neither one follows from a
    /// number. The other two do, and exactly.
    fn severity_follows(finding: &Finding) -> bool {
        match finding.severity {
            // An error exactly when the measurement is outside its limit,
            // and information exactly when it is inside.
            Severity::Info | Severity::Error => {
                (finding.severity == Severity::Error) != finding.holds()
            }
            Severity::Warning | Severity::Skipped => true,
        }
    }

    /// Non-zero if and only if at least one error is present.
    pub fn exit_code(&self) -> i32 {
        i32::from(self.has_errors())
    }

    /// Writes `art/staging/reports/<stage>.<item>.<attempt>.json`.
    pub fn write(&self, repo_root: &Path) -> Result<PathBuf> {
        let path = self.artifacts(repo_root)?.report();
        let parent = path.parent().expect("a report path has a parent");
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
        let json = serde_json::to_string_pretty(self).context("encoding the report")?;
        std::fs::write(&path, json + "\n")
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(path)
    }

    /// Reads a report back, most often one a Blender script wrote. Every
    /// finding is re-validated, so the Python side cannot smuggle in a
    /// measurement with no space or a NaN.
    pub fn read(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let report: Self =
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        // Through the naming owner, so a header this repository could never
        // have written is refused here rather than downstream.
        Artifacts::new(Path::new(""), &report.stage, &report.item, report.attempt)
            .with_context(|| format!("in {}", path.display()))?;
        let mut checked = Self::new(&report.stage, &report.item, report.attempt);
        checked
            .extend(report.findings)
            .with_context(|| format!("in {}", path.display()))?;
        Ok(checked)
    }

    /// Where this report's own files belong. The runner compares this against
    /// the path it asked for, so a mislabeled header cannot pass.
    pub fn artifacts(&self, repo_root: &Path) -> Result<Artifacts> {
        Artifacts::new(repo_root, &self.stage, &self.item, self.attempt)
    }
}

/// Every file one stage attempt writes, named `<stage>.<item>.<attempt>.*`
/// under `art/staging/reports/`.
///
/// One owner for the naming, because the Rust runner writes some of these
/// files and the Blender scripts write the rest. The item is in the name
/// because a stage runs once per clip or per character, and the attempt is
/// in it because a retry must not overwrite the attempt before it.
#[derive(Debug, Clone)]
pub struct Artifacts {
    dir: PathBuf,
    stem: String,
}

impl Artifacts {
    pub fn new(repo_root: &Path, stage: &str, item: &str, attempt: u32) -> Result<Self> {
        // The Blender scripts read the header back off this name, so each
        // part must say something and none may hold the separator.
        for (field, value) in [("stage", stage), ("item", item)] {
            ensure!(!value.trim().is_empty(), "a report needs a {field}");
            ensure!(!value.contains('.'), "a {field} cannot hold a dot: {value}");
        }
        ensure!(attempt >= 1, "the first attempt is 1");
        Ok(Self {
            dir: repo_root.join(REPORTS_DIR),
            stem: format!("{stage}.{item}.{attempt}"),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The `<stage>.<item>.<attempt>` every file of this attempt shares.
    /// A stored verdict names this rather than repeating the findings.
    pub fn stem(&self) -> &str {
        &self.stem
    }

    /// The findings themselves.
    pub fn report(&self) -> PathBuf {
        self.path("json")
    }

    /// Written by the Blender script as its last act, asserted by the runner.
    pub fn sentinel(&self) -> PathBuf {
        self.path("sentinel.json")
    }

    /// The invocation, verbatim, so a failure can be re-run by hand.
    pub fn argv(&self) -> PathBuf {
        self.path("argv.txt")
    }

    /// Blender's own log, at debug level.
    pub fn log(&self) -> PathBuf {
        self.path("log")
    }

    /// The id of a task that has been submitted and not yet seen to finish.
    ///
    /// Written before the first poll and removed on success, so a run that
    /// dies mid-poll resumes what it already paid for instead of buying a
    /// second one.
    pub fn task(&self) -> PathBuf {
        self.path("task")
    }

    /// The source clip's own world orientations, written by the retarget.
    ///
    /// `clip.swing` and `clip.twist` measure the delivered GLB against the
    /// file the motion was bought in. That file is an FBX, no Rust reader
    /// opens one, and CI has no Blender, so Blender records what it read and
    /// the Rust gate reads the pair.
    pub fn source_motion(&self) -> PathBuf {
        self.path("source.json")
    }

    /// The scene as it stood when the script raised.
    pub fn blend(&self) -> PathBuf {
        self.path("blend")
    }

    fn path(&self, extension: &str) -> PathBuf {
        self.dir.join(format!("{}.{extension}", self.stem))
    }
}
