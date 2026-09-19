//! Driving Godot, and reading what it printed.
//!
//! The exit code is not the gate. Measured on 4.7.2, a script error and a
//! failed resource load both leave the run at 0 and say so in the log, so
//! [`Run::problem`] reads the log. Every phrase it greps was provoked once by
//! hand; the design's T16 corrections record how.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// Where Godot is, when it is not on `PATH`. The art pipeline's pack stage
/// reads the same one.
const BIN_ENV: &str = "MARROWFALL_GODOT_BIN";

/// How long one Godot run may take. It is a watchdog and not a courtesy:
/// Godot hangs rather than exits on a fatal error, so without a kill the tier
/// never returns. A cold import of this project takes 8 s.
const WATCHDOG: Duration = Duration::from_secs(180);

/// How often the watchdog looks at the child.
const POLL: Duration = Duration::from_millis(50);

/// The Godot to drive, or `None` after printing the one line that says why
/// this run does nothing.
///
/// A machine with no Godot is a valid setup for everything but this tier. CI
/// cannot be quiet the same way: it installs Godot and proves the binary
/// answers in a step of its own, so a missing engine fails the job before any
/// test runs.
pub fn godot_or_skip() -> Option<String> {
    let program = std::env::var(BIN_ENV).unwrap_or_else(|_| "godot".to_owned());
    if Command::new(&program).arg("--version").output().is_ok() {
        return Some(program);
    }
    println!("skipped: `{program}` is not on PATH, and {BIN_ENV} does not say where else to look");
    None
}

/// Builds the shared library Godot loads, once per test binary.
///
/// Only a plain build writes it: see the gdext gotcha in this tier's own docs.
/// Once, because these tests run in parallel and five cargo invocations would
/// queue on the same build lock.
pub fn build_extension(root: &Path) {
    static BUILT: std::sync::Once = std::sync::Once::new();
    BUILT.call_once(|| {
        let status = Command::new(env!("CARGO"))
            .args(["build", "--package", "render"])
            .current_dir(root)
            .status()
            .expect("running cargo build --package render");
        assert!(status.success(), "cargo build --package render failed");
    });
}

/// Imports every asset in `project`, which is what writes the `.import`
/// sidecars and the `.godot/` cache the runtime loads an atlas through.
pub fn import(godot: &str, log: &Path, project: &Path) -> Run {
    run(
        godot,
        log,
        &["--headless", "--import", "--path", text(project)],
    )
}

/// One Godot run: what it printed, and how it ended.
pub struct Run {
    /// The command, verbatim, so a failure can be repeated by hand.
    pub argv: String,
    pub log: String,
    /// Where the log stayed, for the same reason.
    pub log_path: PathBuf,
    ended: End,
}

enum End {
    Exited(ExitStatus),
    /// The watchdog killed it, after this long.
    TimedOut(Duration),
}

/// Runs Godot with `args`, keeping the whole log at `log`.
pub fn run(godot: &str, log: &Path, args: &[&str]) -> Run {
    run_within(godot, log, args, WATCHDOG)
}

/// The same, on a watchdog of the caller's own, which is how the watchdog
/// gets a test that does not take [`WATCHDOG`] to run.
pub fn run_within(godot: &str, log: &Path, args: &[&str], limit: Duration) -> Run {
    let file = std::fs::File::create(log).expect("creating the log");
    let errors = file.try_clone().expect("sharing the log");

    // A file rather than a pipe: the watchdog below has to poll, and a child
    // that fills a pipe nobody is draining would hang against it.
    let mut child = Command::new(godot)
        .args(args)
        .stdout(Stdio::from(file))
        .stderr(Stdio::from(errors))
        .spawn()
        .expect("spawning godot");

    let deadline = Instant::now() + limit;
    let ended = loop {
        match child.try_wait().expect("waiting for godot") {
            Some(status) => break End::Exited(status),
            None if Instant::now() >= deadline => {
                child.kill().expect("killing godot");
                child.wait().expect("reaping godot");
                break End::TimedOut(limit);
            }
            None => std::thread::sleep(POLL),
        }
    };

    Run {
        argv: format!("{godot} {}", args.join(" ")),
        log: std::fs::read_to_string(log).expect("reading the log"),
        log_path: log.to_owned(),
        ended,
    }
}

impl Run {
    /// Why this run failed, or `None`.
    ///
    /// Three phrases and two outcomes, and each phrase is one Godot prints:
    /// `ERROR:` for an engine or game error, `SCRIPT ERROR` for a script one,
    /// and `ObjectDB instance` for the leak line at exit, which reads
    /// "instance was" for one object and "instances were" for more.
    pub fn problem(&self) -> Option<String> {
        match self.ended {
            End::TimedOut(limit) => {
                return Some(format!(
                    "no exit within {} s, so the watchdog killed it",
                    limit.as_secs()
                ));
            }
            End::Exited(status) if !status.success() => return Some(format!("{status}")),
            End::Exited(_) => {}
        }
        let fatal: Vec<&str> = self
            .log
            .lines()
            .filter(|line| {
                line.starts_with("ERROR:")
                    || line.starts_with("SCRIPT ERROR")
                    || line.contains("ObjectDB instance")
            })
            .collect();
        (!fatal.is_empty()).then(|| fatal.join("\n"))
    }

    /// Panics with the argv and the log unless the run was clean.
    pub fn expect_clean(&self) {
        if let Some(problem) = self.problem() {
            panic!("{problem}\n{}", self.diagnostics());
        }
    }

    /// Panics unless the log holds `phrase`.
    pub fn expect_printed(&self, phrase: &str) {
        assert!(
            self.log.contains(phrase),
            "godot never printed {phrase:?}\n{}",
            self.diagnostics()
        );
    }

    /// The argv, where the log is, and its tail. The tail matters in CI,
    /// where the file is gone by the time anyone reads the failure.
    fn diagnostics(&self) -> String {
        let lines: Vec<&str> = self.log.lines().collect();
        let tail = lines[lines.len().saturating_sub(40)..].join("\n");
        format!(
            "  argv: {}\n  log:  {}\n--- last {} line(s) ---\n{tail}",
            self.argv,
            self.log_path.display(),
            lines.len().min(40),
        )
    }
}

/// A path as an argument. Every path this tier builds is under the worktree,
/// so none of them is unprintable.
pub fn text(path: &Path) -> &str {
    path.to_str().expect("a path Godot can be given")
}
