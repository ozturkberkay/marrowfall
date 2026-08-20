//! The `cargo world` command line.
//!
//! Lives in the library rather than the binary so the tests under `tests/` can
//! reach it: an integration test cannot import a binary.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use worldgen::{IVec2, Tables, World, WorldRules};

use crate::paint::{self, Shot};

/// Fixed development seed, the same one the game boots with.
pub const DEV_SEED: u64 = 0x4D61_7272_6F77; // "Marrow"

#[derive(Parser)]
#[command(name = "cargo-world", about = "Inspect a Marrowfall world seed")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Render a square region of a seed to a PNG.
    ///
    /// Negative numbers are values here, not flags. A subcommand is its own
    /// command as far as the parser is concerned, so the setting has to be on
    /// this variant: without it a negative `--centre` reads as an unknown short
    /// flag and half the world is unreachable.
    #[command(allow_negative_numbers = true)]
    Preview {
        /// World seed. Accepts decimal or `0x` hex.
        #[arg(long, value_parser = parse_seed, default_value_t = DEV_SEED)]
        seed: u64,
        /// Tile at the centre of the image.
        #[arg(long, num_args = 2, value_names = ["X", "Y"], default_values_t = [0, 0])]
        centre: Vec<i32>,
        /// Half-width in tiles.
        #[arg(long, default_value_t = 20_000)]
        radius: i32,
        /// Tiles per pixel. Raise it to see more ground in the same image.
        #[arg(long, default_value_t = 20)]
        step: i32,
        /// Mark every point of interest. Pass `--sites false` for bare terrain.
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        sites: bool,
        /// Directory holding the tuning tables.
        #[arg(long, default_value = "project/data")]
        data: PathBuf,
        #[arg(long, default_value = "target/preview.png")]
        out: PathBuf,
    },
}

/// Runs the command line given an argument list.
pub fn run_from_args<I, T>(argv: I) -> Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    // `--help` and `--version` arrive as clap "errors". They are not failures:
    // clap renders them itself and exits 0. Anything else is a real parse error,
    // returned so the caller and the tests can see it.
    let cli = Cli::try_parse_from(argv).map_err(|error| match error.kind() {
        clap::error::ErrorKind::DisplayHelp
        | clap::error::ErrorKind::DisplayVersion
        | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => error.exit(),
        _ => anyhow::Error::from(error),
    })?;

    match cli.command {
        Command::Preview {
            seed,
            centre,
            radius,
            step,
            sites,
            data,
            out,
        } => {
            anyhow::ensure!(radius > 0, "radius must be positive, got {radius}");
            anyhow::ensure!(step > 0, "step must be positive, got {step}");
            let world = World::new(read_rules(&data)?, seed);
            let shot = Shot {
                // clap guarantees two values for `centre`, so this cannot be short.
                centre: IVec2::new(centre[0], centre[1]),
                radius,
                step,
                sites,
            };
            write_preview(&world, shot, &out)?;
            println!(
                "seed {seed:#x}: {0} by {0} pixels covering {1} tiles, written to {2}",
                shot.side(),
                2 * radius,
                out.display()
            );
            Ok(())
        }
    }
}

/// Renders and writes one image, creating the output directory if needed.
pub fn write_preview(world: &World, shot: Shot, out: &Path) -> Result<()> {
    let image = paint::render(world, shot);
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    image
        .save(out)
        .with_context(|| format!("writing {}", out.display()))
}

/// Reads every table from a directory and validates them together.
pub fn read_rules(dir: &Path) -> Result<WorldRules> {
    let read = |name: &str| {
        let path = dir.join(name);
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))
    };
    let (world, tiers, materials, biomes, site_classes, sites) = (
        read("world.tsv")?,
        read("tiers.tsv")?,
        read("materials.tsv")?,
        read("biomes.tsv")?,
        read("site_classes.tsv")?,
        read("sites.tsv")?,
    );
    worldgen::parse(Tables {
        world: &world,
        tiers: &tiers,
        materials: &materials,
        biomes: &biomes,
        site_classes: &site_classes,
        sites: &sites,
    })
    // `worldgen::Error` is not `anyhow`-compatible on its own, and its message is
    // the useful part: it names the table, the row and the field.
    .map_err(|e| anyhow::anyhow!("{e}"))
    .with_context(|| format!("the tables in {} are not usable", dir.display()))
}

/// Accepts `0x` hex as well as decimal, because a seed is usually written in hex.
fn parse_seed(raw: &str) -> Result<u64, String> {
    let trimmed = raw.trim();
    let parsed = match trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        Some(hex) => u64::from_str_radix(hex, 16),
        None => trimmed.parse(),
    };
    parsed.map_err(|e| format!("{raw:?} is not a seed: {e}"))
}
