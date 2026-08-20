use std::path::Path;

use xtask_world::cli::{DEV_SEED, read_rules, run_from_args};

/// A valid table set on disk, so tests exercise the real reading path.
fn tables_in(dir: &Path) {
    let write = |name: &str, body: &str| std::fs::write(dir.join(name), body).unwrap();
    write(
        "world.tsv",
        "region_pitch_tiles\tregion_jitter_pct\thome_bubble_tiles\n256\t60\t128\n",
    );
    write(
        "tiers.tsv",
        "tier\tinner_tiles\tharder_stray\teasier_stray\tstray_pct\n0\t0\t0\t0\t0\n",
    );
    write(
        "materials.tsv",
        "material\tblocks_walk\tblocks_jump\tblocks_shot\nsoil\t0\t0\t0\n",
    );
    write(
        "biomes.tsv",
        "biome\ttier\tweight\tground\theight_amp\theight_period\nlow\t0\t10\tsoil\t3\t240\n",
    );
    write(
        "site_classes.tsv",
        "class\tspacing\tseparation\tfill_pct\tmin_from_spawn\ttier_lo\ttier_hi\n\
         camp\t120\t60\t40\t0\t0\t0\n",
    );
    write(
        "sites.tsv",
        "site\tclass\tweight\tfootprint\ncampfire\tcamp\t1\t3\n",
    );
}

fn run(args: &[&str]) -> anyhow::Result<()> {
    let mut argv = vec!["cargo-world"];
    argv.extend_from_slice(args);
    run_from_args(argv)
}

#[test]
fn a_preview_writes_a_png_where_it_was_told_to() {
    let dir = tempfile::tempdir().unwrap();
    tables_in(dir.path());
    let out = dir.path().join("nested/preview.png");
    run(&[
        "preview",
        "--radius",
        "32",
        "--step",
        "1",
        "--data",
        dir.path().to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ])
    .unwrap();
    // The directory did not exist, so this also proves it is created.
    assert!(out.is_file());
    let image = image::open(&out).unwrap();
    assert_eq!(image.width(), 64);
}

#[test]
fn the_shipped_tables_load_from_the_repository() {
    // The default `--data` path is relative to the repository root, so this also
    // pins that the tables live where the frontend expects them.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    assert!(read_rules(&root.join("project/data")).is_ok());
}

#[test]
fn a_missing_table_directory_says_which_file_it_wanted() {
    let error = read_rules(Path::new("/nonexistent/marrowfall")).unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("world.tsv"), "{message}");
}

#[test]
fn a_broken_table_reports_the_table_and_the_row() {
    let dir = tempfile::tempdir().unwrap();
    tables_in(dir.path());
    std::fs::write(
        dir.path().join("biomes.tsv"),
        "biome\ttier\tweight\tground\theight_amp\theight_period\nlow\t0\t10\tmissing\t3\t240\n",
    )
    .unwrap();
    let error = read_rules(dir.path()).unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("biomes.tsv"), "{message}");
    assert!(message.contains("missing"), "{message}");
}

#[test]
fn a_non_positive_radius_or_step_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    tables_in(dir.path());
    let data = dir.path().to_str().unwrap().to_owned();
    for (flag, value) in [("--radius", "0"), ("--step", "0"), ("--radius", "-5")] {
        let error = run(&["preview", flag, value, "--data", &data]).unwrap_err();
        assert!(
            format!("{error}").contains("must be positive"),
            "{flag} {value}: {error}"
        );
    }
}

#[test]
fn a_negative_centre_is_a_coordinate_and_not_a_flag() {
    // Half the world has negative coordinates, so this is not an edge case.
    let dir = tempfile::tempdir().unwrap();
    tables_in(dir.path());
    let out = dir.path().join("p.png");
    run(&[
        "preview",
        "--centre",
        "-500",
        "-500",
        "--radius",
        "16",
        "--step",
        "1",
        "--data",
        dir.path().to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ])
    .unwrap();
    assert!(out.is_file());
}

#[test]
fn a_seed_can_be_written_in_hex_or_decimal() {
    let dir = tempfile::tempdir().unwrap();
    tables_in(dir.path());
    let data = dir.path().to_str().unwrap().to_owned();
    let out = dir.path().join("p.png");
    let out = out.to_str().unwrap().to_owned();
    for seed in ["0x4D617272", "0X10", "1234"] {
        run(&[
            "preview", "--seed", seed, "--radius", "8", "--step", "1", "--data", &data, "--out",
            &out,
        ])
        .unwrap_or_else(|e| panic!("{seed} was rejected: {e}"));
    }
}

#[test]
fn a_seed_that_is_not_a_number_is_refused() {
    let error = run(&["preview", "--seed", "marrow"]).unwrap_err();
    assert!(format!("{error}").contains("not a seed"), "{error}");
}

#[test]
fn an_unknown_subcommand_is_an_error_rather_than_a_panic() {
    assert!(run(&["conjure"]).is_err());
}

#[test]
fn the_development_seed_is_the_one_the_game_boots_with() {
    // `bridge.rs` carries the same constant. If they drift, the preview shows a
    // different world from the one the game opens on.
    assert_eq!(DEV_SEED, 0x4D61_7272_6F77);
}
