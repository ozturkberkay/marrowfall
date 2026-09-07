//! The one place Godot is invoked.
//!
//! Only the pack stage needs it. An atlas is loadable only through the
//! `.import` sidecar beside it, and the runtime resolves the texture through
//! the imported path that sidecar names, which is Godot's to write. So the
//! pack seeds the settings it owns and then hands the project to the importer.

use std::path::Path;
use std::process::Command;

use anyhow::{Context as _, Result, bail};

/// Where Godot is, when it is not on `PATH`.
const BIN_ENV: &str = "MARROWFALL_GODOT_BIN";

/// Imports every asset in a Godot project, headless.
///
/// A missing Godot is an error rather than a skip: a pack that leaves its
/// seeds behind ships atlases the game cannot load.
///
/// The exit code is not the gate. Measured on 4.7.2: a file Godot fails to
/// import leaves the run at exit 0 and says so on stderr only, so its own
/// first `ERROR:` line fails the stage. A clean import prints nothing there.
pub fn import(project: &Path) -> Result<()> {
    let program = crate::tool_binary(BIN_ENV, "godot");
    let output = Command::new(&program)
        .arg("--headless")
        .arg("--import")
        .arg("--path")
        .arg(project)
        .output()
        .with_context(|| {
            format!(
                "running `{program} --headless --import`, is godot on PATH? \
                 {BIN_ENV} says where else to look"
            )
        })?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        bail!(
            "`{program} --headless --import` failed on {}\n{stderr}",
            project.display()
        );
    }
    if let Some(line) = stderr.lines().find(|line| line.starts_with("ERROR:")) {
        bail!("godot could not import {}: {line}", project.display());
    }
    Ok(())
}
