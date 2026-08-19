//! The `cargo art` command line: what to run, in what order, and when to stop
//! and ask. Stages themselves live in [`crate::stages`].
//!
//! The pipeline splits at the GLB. Before it is AI generation, expensive and
//! not reproducible, so it is committed. After it is deterministic local work,
//! free to re-run. That boundary is why tweaking a sprite setting never
//! re-spends credits.

use std::collections::BTreeSet;
use std::io::{BufRead, Write as _};
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use clap::{Parser, Subcommand};

use crate::library::AnimationLibrary;
use crate::lock::{Lock, Provider, Stage, TaskRef};
use crate::spec::{CharacterSpec, CharacterType, Paths};

#[derive(Debug, Parser)]
#[command(name = "cargo art", about = "Marrowfall character art pipeline")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Scaffold a new character spec with the project's locked defaults.
    New {
        name: String,
        #[arg(long, value_enum, default_value_t = CharacterType::Humanoid)]
        kind: CharacterType,
    },
    /// Run the pipeline, resuming from wherever it left off.
    Run {
        name: String,
        /// Start here, discarding this stage and everything after it.
        #[arg(long)]
        from: Option<Stage>,
        /// Run only this stage.
        #[arg(long, conflicts_with = "from")]
        only: Option<Stage>,
        /// Re-run stages already recorded as complete.
        #[arg(long)]
        retry: bool,
        /// Do not pause for review between stages.
        #[arg(long)]
        yes: bool,
    },
    /// Show which stages are complete.
    Status {
        name: String,
        /// Emit machine-readable JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
    /// Validate specs without running anything.
    Check {
        /// Defaults to every spec in art/characters.
        name: Option<String>,
    },
}

/// What the driver decided to do with a stage, before any of it happens.
/// Separate from execution so the decision is testable without network or
/// disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Run(Stage),
    /// Already done with matching inputs.
    Cached(Stage),
    /// Not applicable to this character.
    Skipped(Stage, &'static str),
    /// Would re-run a completed stage that costs money; needs confirmation.
    ConfirmSpend(Stage),
}

/// Options affecting which stages run.
#[derive(Debug, Clone, Copy, Default)]
pub struct RunOptions {
    pub from: Option<Stage>,
    pub only: Option<Stage>,
    pub retry: bool,
}

/// Decides what to do with every stage. Pure: no IO, no side effects. A GLB
/// present without a lock record came from outside this tool and wins, since
/// AI generation is not reproducible.
pub fn plan(
    lock: &Lock,
    spec: &CharacterSpec,
    library: &AnimationLibrary,
    options: RunOptions,
    checkpoint_on_disk: bool,
) -> Vec<Step> {
    // Completion is judged *before* `--from` invalidates anything, so an
    // explicit `--from concept` still warns before re-spending on a stage that
    // had already succeeded.
    let complete: BTreeSet<Stage> = Stage::all()
        .into_iter()
        .filter(|stage| lock.is_current(*stage, spec, library))
        .collect();

    let selected: Vec<Stage> = match (options.only, options.from) {
        (Some(only), _) => vec![only],
        (None, Some(from)) => Stage::all().into_iter().filter(|s| *s >= from).collect(),
        (None, None) => Stage::all().to_vec(),
    };

    selected
        .into_iter()
        .map(|stage| {
            if !spec.subject.kind.can_be_rigged() && stage == Stage::Rig {
                return Step::Skipped(stage, "body plan cannot be auto-rigged");
            }
            let done = complete.contains(&stage);
            // `--from` and `--only` both mean "do this again", so they must
            // not silently reuse the cache, but they must still confirm
            // before spending.
            let explicitly_forced =
                options.retry || options.only == Some(stage) || options.from.is_some();
            if checkpoint_on_disk && !done && !explicitly_forced && stage <= Stage::Download {
                return Step::Skipped(stage, "checkpoint GLB already on disk");
            }
            match (done, explicitly_forced, stage.costs_credits()) {
                (true, false, _) => Step::Cached(stage),
                (true, true, true) => Step::ConfirmSpend(stage),
                _ => Step::Run(stage),
            }
        })
        .collect()
}

/// Locates the workspace root. `CARGO_MANIFEST_DIR` covers the cargo alias;
/// the upward walk covers a directly executed binary.
pub fn repo_root() -> Result<PathBuf> {
    if let Some(manifest) = std::env::var_os("CARGO_MANIFEST_DIR") {
        let dir = PathBuf::from(manifest);
        if let Some(root) = dir.ancestors().find(|dir| dir.join("crates").is_dir()) {
            return Ok(root.to_path_buf());
        }
    }
    let mut dir = std::env::current_dir()?;
    loop {
        if dir.join("Cargo.toml").exists() && dir.join("crates").is_dir() {
            return Ok(dir);
        }
        if !dir.pop() {
            bail!("could not find the workspace root (no Cargo.toml with crates/ above cwd)");
        }
    }
}

pub fn new_character(root: &Path, name: &str, kind: CharacterType) -> Result<()> {
    let paths = Paths::new(root, name);
    if paths.spec().exists() {
        bail!("{} already exists", paths.spec().display());
    }
    CharacterSpec::template(name, kind).save(&paths.spec())?;

    println!("created {}", paths.relative(&paths.spec()));
    println!("\nnext: describe the character in that file, then run:");
    println!("  cargo art run {name}");
    Ok(())
}

pub async fn run(
    root: &Path,
    name: &str,
    options: RunOptions,
    yes: bool,
    input: &mut impl BufRead,
) -> Result<()> {
    let paths = Paths::new(root, name);
    let spec = CharacterSpec::load(&paths.spec())?;
    spec.validate().context("spec is not valid")?;

    let library = AnimationLibrary::load(root)?;
    // Fail before spending anything if a spec names an animation that is not
    // declared; the library's error lists what is available.
    library.resolve(&spec.animations, &spec.subject.skeleton)?;

    let mut lock = Lock::load(&paths.lock())?;
    // Note: the lock is deliberately NOT invalidated up front. `plan` already
    // forces every stage from `--from` onward, and `Lock::record` cascades once
    // a stage actually succeeds, so declining a spend prompt leaves the
    // recorded work, and its task ids, intact.
    let steps = plan(
        &lock,
        &spec,
        &library,
        options,
        paths.character_glb().exists(),
    );

    for step in steps {
        let stage = match step {
            Step::Cached(stage) if lock.is_current(stage, &spec, &library) => {
                println!("{stage}: cached");
                continue;
            }
            // Planned as cached, but an upstream stage has since re-run and
            // invalidated it.
            Step::Cached(stage) => stage,
            Step::Skipped(stage, why) => {
                println!("{stage}: skipped ({why})");
                continue;
            }
            Step::ConfirmSpend(stage) => {
                if !confirm_spend(stage, yes, input)? {
                    println!("{stage}: skipped by user");
                    continue;
                }
                stage
            }
            Step::Run(stage) => stage,
        };

        if let Some(provider) = stage.provider() {
            report_balance(provider).await;
        }
        println!("{stage}: running…");

        let record = match stage {
            Stage::Concept => crate::stages::concept(&spec, &paths, options.retry).await?,
            Stage::Model => crate::stages::model(&spec, &paths).await?,
            Stage::Rig => {
                // Rigging plus N animations is several separate charges, so
                // each task id is persisted the moment it succeeds.
                let known = lock.tasks();
                let mut fresh: Vec<TaskRef> = Vec::new();
                let outcome =
                    crate::stages::rig(&spec, &library, root, &known, |task| fresh.push(task))
                        .await;
                if outcome.is_err() && !fresh.is_empty() {
                    lock.stages
                        .entry(Stage::Rig)
                        .or_default()
                        .tasks
                        .extend(fresh);
                    lock.save(&paths.lock())?;
                    eprintln!(
                        "  note: {} completed task(s) recorded; re-running will reuse them",
                        lock.stages[&Stage::Rig].tasks.len()
                    );
                }
                outcome?
            }
            Stage::Download => crate::stages::download(&paths, root, &lock.tasks()).await?,
            Stage::Bake => crate::stages::bake(&spec, &library, &paths, root)?,
            Stage::Pack => crate::stages::pack(&spec, &library, &paths)?,
        };

        if let Some(note) = &record.note {
            println!("  {note}");
        }
        lock.record(stage, &spec, &library, record);
        lock.save(&paths.lock())?;

        if !yes && options.only.is_none() && should_pause(stage) {
            pause_for_review(stage, &paths, input)?;
        }
    }

    println!("\n{name}: done");
    Ok(())
}

/// Stages whose output is worth looking at before spending more.
pub fn should_pause(stage: Stage) -> bool {
    matches!(
        stage,
        Stage::Concept | Stage::Model | Stage::Bake | Stage::Pack
    )
}

/// Stops after a reviewable stage until the operator says to continue. Reads
/// `input` rather than stdin, so no-terminal is testable.
pub fn pause_for_review(stage: Stage, paths: &Paths, input: &mut impl BufRead) -> Result<()> {
    println!(
        "\n  review {stage} output in {}",
        paths.relative(&paths.preview())
    );
    print!("  continue? [Y/n] ");
    std::io::stdout().flush()?;

    let mut answer = String::new();
    if input.read_line(&mut answer)? == 0 {
        // No terminal attached (an agent, or a pipe). Continuing silently
        // would defeat the review gate, so stop with the work so far saved.
        bail!("{stage} finished, but no terminal is attached to confirm. Pass --yes to continue.");
    }
    if matches!(answer.trim().to_ascii_lowercase().as_str(), "n" | "no") {
        bail!("stopped after {stage}, re-run when ready, completed stages are cached");
    }
    Ok(())
}

/// Asks before re-running a stage that has already been paid for. No terminal
/// means no confirmation.
pub fn confirm_spend(stage: Stage, yes: bool, input: &mut impl BufRead) -> Result<bool> {
    if yes {
        return Ok(true);
    }
    print!("  {stage} already completed and costs credits. Re-run? [y/N] ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    input.read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

/// Prints the remaining balance for the provider about to be billed.
/// Advisory: a failed lookup must not stop the pipeline.
pub async fn report_balance(provider: Provider) {
    if provider != Provider::Meshy {
        // OpenAI exposes no balance endpoint on the images API.
        return;
    }
    if let Ok(client) = crate::meshy::Client::from_env()
        && let Ok(balance) = client.balance().await
    {
        println!("  meshy balance: {balance} credits");
    }
}

pub fn status(root: &Path, name: &str, json: bool) -> Result<()> {
    let paths = Paths::new(root, name);
    let spec = CharacterSpec::load(&paths.spec())?;
    let lock = Lock::load(&paths.lock())?;
    let library = AnimationLibrary::load(root)?;

    let state = |stage: Stage| {
        if lock.is_current(stage, &spec, &library) {
            "done"
        } else if lock.stages.contains_key(&stage) {
            "stale"
        } else {
            "todo"
        }
    };

    if json {
        let stages: serde_json::Map<String, serde_json::Value> = Stage::all()
            .into_iter()
            .map(|stage| {
                (
                    stage.as_str().to_owned(),
                    serde_json::json!({
                        "state": state(stage),
                        "note": lock.stages.get(&stage).and_then(|r| r.note.clone()),
                    }),
                )
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "name": name,
                "kind": format!("{:?}", spec.subject.kind),
                "stages": stages,
            }))?
        );
        return Ok(());
    }

    println!("{name} ({:?})", spec.subject.kind);
    for stage in Stage::all() {
        let note = lock
            .stages
            .get(&stage)
            .and_then(|record| record.note.clone())
            .unwrap_or_default();
        println!("  {:<9} {:<5} {note}", stage.as_str(), state(stage));
    }
    Ok(())
}

pub fn check(root: &Path, name: Option<&str>) -> Result<()> {
    let dir = root.join("art/characters");
    let specs: Vec<PathBuf> = match name {
        Some(name) => vec![Paths::new(root, name).spec()],
        None => {
            if !dir.exists() {
                println!("no characters yet");
                return Ok(());
            }
            // One directory per character, each holding a spec.ron.
            let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
                .with_context(|| format!("reading {}", dir.display()))?
                .filter_map(Result::ok)
                .map(|entry| entry.path().join("spec.ron"))
                .filter(|path| path.is_file())
                .collect();
            found.sort();
            found
        }
    };

    let mut failed = 0;
    for path in &specs {
        match CharacterSpec::load(path).and_then(|spec| spec.validate()) {
            Ok(()) => println!("ok    {}", path.display()),
            Err(error) => {
                failed += 1;
                println!("FAIL  {}: {error:#}", path.display());
            }
        }
    }
    anyhow::ensure!(failed == 0, "{failed} spec(s) invalid");
    println!("\n{} spec(s) ok", specs.len());
    Ok(())
}

/// Parses the command line and dispatches. The binary is only a call to this,
/// so every path stays reachable from the tests.
pub async fn run_from_args<I, T>(argv: I) -> Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    // `--help` and `--version` arrive as clap "errors". They are not failures:
    // clap renders them itself and exits 0. Anything else is a real parse
    // error, returned so the caller (and the tests) can see it.
    let cli = Cli::try_parse_from(argv).map_err(|error| match error.kind() {
        clap::error::ErrorKind::DisplayHelp
        | clap::error::ErrorKind::DisplayVersion
        | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => error.exit(),
        _ => anyhow::Error::from(error),
    })?;
    let root = repo_root()?;

    match cli.command {
        Command::New { name, kind } => new_character(&root, &name, kind),
        Command::Run {
            name,
            from,
            only,
            retry,
            yes,
        } => {
            run(
                &root,
                &name,
                RunOptions { from, only, retry },
                yes,
                &mut std::io::stdin().lock(),
            )
            .await
        }
        Command::Status { name, json } => status(&root, &name, json),
        Command::Check { name } => check(&root, name.as_deref()),
    }
}
