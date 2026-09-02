//! The one place Blender is invoked.
//!
//! Argument order is a documented silent-failure mode, so every argv comes
//! from [`argv`] and a unit test pins it. `--python-use-system-env` is what
//! lets `PYTHONPATH` reach Blender's embedded interpreter, which is how our
//! own modules become importable at all.
//!
//! `--python-exit-code` only catches a script's own top level: raised from a
//! handler, a thread or `atexit`, Blender exits 0
//! (`docs/research/agent_reports/proof_python_exit_code_coverage.md`). So the
//! success sentinel is the gate and the flag is defense in depth. [`run`]
//! deletes any stale sentinel first, then asserts the script wrote a fresh
//! one.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, Result, bail, ensure};

use crate::check::{Artifacts, Report};

/// Where the Blender scripts and the modules they import live.
pub const BLENDER_SRC: &str = "tools/blender/src";

/// Where `findings.guard` writes the success sentinel.
const SENTINEL_ENV: &str = "MARROWFALL_SENTINEL";

/// Where `findings.guard` saves the scene if the script raises.
const CRASH_BLEND_ENV: &str = "MARROWFALL_CRASH_BLEND";

/// Where `findings.report_path` writes the run's findings. The runner owns
/// every report name, so no script derives one.
const REPORT_ENV: &str = "MARROWFALL_REPORT";

/// Every `blender` argument, in the one documented order.
///
/// `--log-level debug` is not decoration: with `--log-file` alone Blender
/// writes an empty file.
pub fn argv(script: &Path, log_file: &Path, script_args: &[OsString]) -> Vec<OsString> {
    let mut argv: Vec<OsString> = vec![
        "--background".into(),
        "--factory-startup".into(),
        "--offline-mode".into(),
        "--python-use-system-env".into(),
        "--python-exit-code".into(),
        "1".into(),
        "--log-level".into(),
        "debug".into(),
        "--log-file".into(),
        log_file.into(),
        "--python".into(),
        script.into(),
        "--".into(),
    ];
    argv.extend_from_slice(script_args);
    argv
}

/// Runs a Blender script and refuses a run that did not finish.
///
/// Returns the findings the script wrote, if it wrote any. An `Err` means the
/// *run* failed, never that the art failed: a gate script measures, writes its
/// report, and exits 0, and the caller turns errors into an exit code. So a
/// script that exits non-zero has broken, and its report is not trusted.
///
/// Keeps four diagnostics beside the report: the argv verbatim, Blender's own
/// log, the `.blend` the script saves from its exception handler, and whatever
/// partial output the run produced, which nothing here deletes.
pub fn run(
    script: &Path,
    script_args: &[OsString],
    artifacts: &Artifacts,
    repo_root: &Path,
) -> Result<Option<Report>> {
    std::fs::create_dir_all(artifacts.dir())
        .with_context(|| format!("creating {}", artifacts.dir().display()))?;
    let sentinel = artifacts.sentinel();
    let wanted = artifacts.report();
    // Either one left by an earlier run would be read as this run's.
    for stale in [&sentinel, &wanted] {
        match std::fs::remove_file(stale) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(error).with_context(|| format!("clearing {}", stale.display()));
            }
            _ => {}
        }
    }

    let argv = argv(script, &artifacts.log(), script_args);
    let program = blender_binary();
    write_argv(&artifacts.argv(), &program, &argv)?;

    let output = Command::new(&program)
        .args(&argv)
        .env("PYTHONPATH", python_path(repo_root)?)
        .env(SENTINEL_ENV, &sentinel)
        .env(CRASH_BLEND_ENV, artifacts.blend())
        .env(REPORT_ENV, artifacts.report())
        .output()
        .context("running blender, is it on PATH?")?;
    if !output.status.success() {
        // Blender writes diagnostics to both streams; showing one of them
        // routinely hides the actual cause.
        bail!(
            "blender failed on {}. Diagnostics in {}\n{}\n{}",
            script.display(),
            artifacts.dir().display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    ensure!(
        sentinel.exists(),
        "blender exited 0 without finishing {}. It raised outside the \
         script's top level, which Blender does not report. Diagnostics in {}",
        script.display(),
        artifacts.dir().display()
    );
    if !wanted.exists() {
        return Ok(None);
    }
    let report = Report::read(&wanted)?;
    let named = report.artifacts(repo_root)?.report();
    ensure!(
        named == wanted,
        "the report in {} says it belongs at {}. A mislabeled report is \
         attributed to the wrong stage everywhere downstream",
        wanted.display(),
        named.display()
    );
    Ok(Some(report))
}

/// Locates the Blender executable. Overridable for a test stub, or an
/// install outside PATH.
fn blender_binary() -> String {
    std::env::var("MARROWFALL_BLENDER_BIN").unwrap_or_else(|_| "blender".to_owned())
}

/// The invocation as a human would retype it, one argument per line so a
/// path with a space stays readable.
fn write_argv(path: &Path, program: &str, argv: &[OsString]) -> Result<()> {
    let mut text = String::from(program);
    for arg in argv {
        text.push('\n');
        text.push_str(&arg.to_string_lossy());
    }
    text.push('\n');
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

/// Our own modules plus the project's site-packages, for Blender's embedded
/// interpreter to import.
fn python_path(repo_root: &Path) -> Result<OsString> {
    std::env::join_paths([
        venv_site_packages(repo_root)?.into_os_string(),
        repo_root.join(BLENDER_SRC).into_os_string(),
    ])
    .context("building PYTHONPATH for blender")
}

/// Site-packages of the project's virtualenv, handed to Blender's embedded
/// interpreter. The Python minor version must match Blender's, because
/// pydantic ships a compiled core, hence the glob.
pub fn venv_site_packages(repo_root: &Path) -> Result<PathBuf> {
    let lib = repo_root.join(".venv/lib");
    let mut candidates: Vec<(u32, u32, PathBuf)> = std::fs::read_dir(&lib)
        .with_context(|| {
            format!(
                "no virtualenv at {}, run `uv sync`",
                repo_root.join(".venv").display()
            )
        })?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let site = path.join("site-packages");
            if !site.is_dir() {
                return None;
            }
            // Sort on the parsed version, not the directory name: "python3.9"
            // sorts after "python3.13" as text, which would pick the older one.
            let name = path.file_name()?.to_str()?.strip_prefix("python")?;
            let (major, minor) = name.split_once('.')?;
            Some((major.parse().ok()?, minor.parse().ok()?, site))
        })
        .collect();
    candidates.sort_unstable();
    candidates
        .pop()
        .map(|(_, _, site)| site)
        .with_context(|| format!("no site-packages under {}, run `uv sync`", lib.display()))
}

/// A `NAME=VALUE` script argument, the shape every Blender script here reads.
pub fn pair(name: &str, value: impl AsRef<OsStr>) -> OsString {
    let mut pair = OsString::from(name);
    pair.push("=");
    pair.push(value);
    pair
}
