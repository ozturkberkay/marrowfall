use worldgen::{HEIGHT_RANGE, Tables, TileFlags, parse};

/// The tables the game ships, so every test starts from something valid and
/// overrides exactly the one table it is about.
const WORLD: &str = "region_pitch_tiles\tregion_jitter_pct\thome_bubble_tiles\n1500\t60\t1000\n";
const TIERS: &str = "\
tier\tinner_tiles\tharder_stray\teasier_stray\tstray_pct
0\t0\t0\t0\t0
1\t2000\t1\t0\t8
2\t5000\t1\t1\t15
";
const MATERIALS: &str = "\
material\tblocks_walk\tblocks_jump\tblocks_shot
dead_grass\t0\t0\t0
stone\t0\t0\t0
rock\t1\t1\t1
";
const BIOMES: &str = "\
biome\ttier\tweight\tground\theight_amp\theight_period
ashen_lowland\t0\t10\tdead_grass\t3\t140
blackweald\t1\t10\tdead_grass\t4\t120
scoured_rock\t2\t10\tstone\t7\t560
";

const SITE_CLASSES: &str = "\
class\tspacing\tseparation\tfill_pct\tmin_from_spawn\ttier_lo\ttier_hi
camp\t400\t240\t35\t0\t0\t2
";
const SITES: &str = "\
site\tclass\tweight\tfootprint
campfire\tcamp\t1\t3
";

/// Overrides one table and leaves the rest valid. Two extra helpers below cover
/// the site tables, so a caller never has to spell out all six.
fn tables<'a>(world: &'a str, tiers: &'a str, materials: &'a str, biomes: &'a str) -> Tables<'a> {
    Tables {
        world,
        tiers,
        materials,
        biomes,
        site_classes: SITE_CLASSES,
        sites: SITES,
    }
}

fn with_site_classes(site_classes: &str) -> Tables<'_> {
    Tables {
        site_classes,
        ..tables(WORLD, TIERS, MATERIALS, BIOMES)
    }
}

fn with_sites(sites: &str) -> Tables<'_> {
    Tables {
        sites,
        ..tables(WORLD, TIERS, MATERIALS, BIOMES)
    }
}

fn valid() -> Tables<'static> {
    tables(WORLD, TIERS, MATERIALS, BIOMES)
}

/// The message a caller would actually read, so tests assert on what a person
/// sees rather than on an enum shape.
fn error(t: Tables<'_>) -> String {
    match parse(t) {
        Ok(_) => panic!("expected a rejection"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn the_shipped_tables_parse() {
    // Guards against a table being edited into an invalid state. The real files
    // are checked by their own test; these are their shape.
    assert!(parse(valid()).is_ok());
}

#[test]
fn world_scalars_read_back() {
    let rules = parse(valid()).unwrap();
    assert_eq!(rules.region_pitch(), 1500);
    assert_eq!(rules.home_bubble(), 1000);
}

#[test]
fn a_material_carries_the_flags_its_row_declares() {
    let rules = parse(valid()).unwrap();
    let grass = rules.material_named("dead_grass").unwrap();
    assert_eq!(rules.material(grass).flags, TileFlags::NONE);
    let rock = rules.material_named("rock").unwrap();
    assert!(rules.material(rock).flags.blocks_walk());
    assert!(rules.material(rock).flags.blocks_jump());
    assert!(rules.material(rock).flags.blocks_shot());
}

#[test]
fn a_biome_resolves_its_ground_to_a_material() {
    let rules = parse(valid()).unwrap();
    let biome = rules.biomes_in(2)[0];
    let ground = rules.biome(biome).ground;
    assert_eq!(rules.material(ground).name, "stone");
}

#[test]
fn distance_maps_to_the_tier_whose_band_contains_it() {
    let rules = parse(valid()).unwrap();
    assert_eq!(rules.tier_for(0), 0);
    assert_eq!(rules.tier_for(1999), 0);
    assert_eq!(
        rules.tier_for(2000),
        1,
        "a band starts at its own inner edge"
    );
    assert_eq!(rules.tier_for(4999), 1);
    assert_eq!(rules.tier_for(5000), 2);
    assert_eq!(rules.tier_for(i64::MAX), 2, "the last band runs forever");
}

#[test]
fn every_tier_has_at_least_one_biome_to_offer() {
    let rules = parse(valid()).unwrap();
    for tier in 0..=rules.max_tier() {
        assert!(!rules.biomes_in(tier).is_empty(), "tier {tier} is empty");
    }
}

#[test]
fn comments_and_blank_lines_are_ignored() {
    let commented = format!("# the starting bands\n{TIERS}\n");
    assert!(parse(tables(WORLD, &commented, MATERIALS, BIOMES)).is_ok());
}

#[test]
fn windows_line_endings_parse() {
    // A spreadsheet on Windows writes CRLF. This is why the reader is a library
    // rather than a call to `split('\n')`.
    let crlf = TIERS.replace('\n', "\r\n");
    assert!(parse(tables(WORLD, &crlf, MATERIALS, BIOMES)).is_ok());
}

#[test]
fn an_unresolvable_ground_names_the_table_the_row_and_the_value() {
    let broken = BIOMES.replace("dead_grass\t3", "drk_grass\t3");
    let message = error(tables(WORLD, TIERS, MATERIALS, &broken));
    assert!(message.contains("biomes.tsv"), "{message}");
    assert!(message.contains("drk_grass"), "{message}");
    assert!(message.contains("materials.tsv"), "{message}");
}

#[test]
fn a_tier_with_no_biomes_is_rejected() {
    let orphan = format!("{TIERS}3\t9000\t1\t1\t20\n");
    let message = error(tables(WORLD, &orphan, MATERIALS, BIOMES));
    assert!(message.contains("tier 3"), "{message}");
}

#[test]
fn a_duplicate_name_is_rejected() {
    let dupe = format!("{MATERIALS}rock\t1\t1\t1\n");
    let message = error(tables(WORLD, TIERS, &dupe, BIOMES));
    assert!(message.contains("rock"), "{message}");
    assert!(message.contains("materials.tsv"), "{message}");
}

#[test]
fn an_empty_table_is_rejected() {
    for (label, t) in [
        ("tiers", tables(WORLD, "", MATERIALS, BIOMES)),
        ("materials", tables(WORLD, TIERS, "", BIOMES)),
        ("biomes", tables(WORLD, TIERS, MATERIALS, "")),
        ("world", tables("", TIERS, MATERIALS, BIOMES)),
    ] {
        let message = error(t);
        assert!(message.contains(label), "{label}: {message}");
    }
}

#[test]
fn tiers_must_start_at_zero_and_run_contiguously() {
    // A gap would leave `tier_for` returning a tier with no band, and a start
    // above zero would leave the spawn with no tier at all.
    let gap = "\
tier\tinner_tiles\tharder_stray\teasier_stray\tstray_pct
0\t0\t0\t0\t0
2\t5000\t1\t1\t15
";
    assert!(error(tables(WORLD, gap, MATERIALS, BIOMES)).contains("tiers.tsv"));
}

#[test]
fn tier_bands_must_ascend() {
    let unsorted = "\
tier\tinner_tiles\tharder_stray\teasier_stray\tstray_pct
0\t5000\t0\t0\t0
1\t2000\t1\t0\t8
";
    let message = error(tables(WORLD, unsorted, MATERIALS, BIOMES));
    assert!(message.contains("tiers.tsv"), "{message}");
}

#[test]
fn the_first_band_must_start_at_the_origin() {
    let late = TIERS.replace("0\t0\t0\t0\t0", "0\t500\t0\t0\t0");
    assert!(error(tables(WORLD, &late, MATERIALS, BIOMES)).contains("tiers.tsv"));
}

#[test]
fn a_stray_percentage_above_one_hundred_is_rejected() {
    let over = TIERS.replace("1\t2000\t1\t0\t8", "1\t2000\t1\t0\t101");
    assert!(error(tables(WORLD, &over, MATERIALS, BIOMES)).contains("stray_pct"));
}

#[test]
fn a_zero_weight_biome_is_rejected() {
    // A weight of zero can never be picked, so it is a typo rather than a choice.
    let zero = BIOMES.replace("ashen_lowland\t0\t10", "ashen_lowland\t0\t0");
    assert!(error(tables(WORLD, TIERS, MATERIALS, &zero)).contains("weight"));
}

#[test]
fn a_height_amplitude_outside_the_range_is_rejected() {
    let huge = BIOMES.replace("\t3\t140", "\t99\t140");
    let message = error(tables(WORLD, TIERS, MATERIALS, &huge));
    assert!(message.contains("height_amp"), "{message}");
    assert!(
        *HEIGHT_RANGE.end() < 99,
        "the test constant must exceed the range"
    );
}

#[test]
fn a_zero_height_period_is_rejected() {
    // The period is a divisor, so zero would be a division by zero per tile.
    let zero = BIOMES.replace("\t3\t140", "\t3\t0");
    assert!(error(tables(WORLD, TIERS, MATERIALS, &zero)).contains("height_period"));
}

#[test]
fn jitter_beyond_half_the_pitch_is_rejected() {
    // Past half a pitch the nearest region point is not always in the
    // surrounding 3 by 3 block, so the partition stops being a Voronoi diagram
    // and the "one contiguous patch per biome" guarantee fails.
    let wild = WORLD.replace("\t60\t", "\t120\t");
    assert!(error(tables(&wild, TIERS, MATERIALS, BIOMES)).contains("region_jitter_pct"));
}

#[test]
fn a_zero_region_pitch_is_rejected() {
    let zero = WORLD.replace("1500\t", "0\t");
    assert!(error(tables(&zero, TIERS, MATERIALS, BIOMES)).contains("region_pitch"));
}

#[test]
fn a_row_with_a_missing_column_is_rejected() {
    // The trailing-whitespace hook strips a trailing tab, which would silently
    // drop a row's last column. Better to refuse than to read a default.
    let short = MATERIALS.replace("rock\t1\t1\t1", "rock\t1\t1");
    assert!(error(tables(WORLD, TIERS, &short, BIOMES)).contains("materials.tsv"));
}

#[test]
fn the_tables_the_game_ships_are_valid() {
    // `include_str!` rather than a file read, so a broken table fails the build
    // rather than the game.
    assert!(
        parse(Tables {
            world: include_str!("../../../../project/data/world.tsv"),
            tiers: include_str!("../../../../project/data/tiers.tsv"),
            materials: include_str!("../../../../project/data/materials.tsv"),
            biomes: include_str!("../../../../project/data/biomes.tsv"),
            site_classes: "class\tspacing\tseparation\tfill_pct\tmin_from_spawn\ttier_lo\ttier_hi\ncamp\t400\t240\t35\t0\t0\t0\n",
            sites: "site\tclass\tweight\tfootprint\ncampfire\tcamp\t1\t3\n",
        })
        .is_ok()
    );
}

#[test]
fn a_biome_cannot_use_an_impassable_material_as_its_ground() {
    // A whole region the player can see and never walk into. Caught by hand
    // during implementation, so it is a rule now rather than a habit.
    let unwalkable = BIOMES.replace(
        "ashen_lowland\t0\t10\tdead_grass",
        "ashen_lowland\t0\t10\trock",
    );
    let message = error(tables(WORLD, TIERS, MATERIALS, &unwalkable));
    assert!(message.contains("blocks walking"), "{message}");
}

#[test]
fn a_site_class_needs_a_separation_under_its_spacing() {
    // Equal would divide by zero in the placement, larger would underflow into a
    // modulus that quietly destroys the gap guarantee.
    for separation in ["400", "600"] {
        let text = format!(
            "class\tspacing\tseparation\tfill_pct\tmin_from_spawn\ttier_lo\ttier_hi\n\
             camp\t400\t{separation}\t35\t0\t0\t2\n"
        );
        assert!(
            error(with_site_classes(&text)).contains("separation must be below spacing"),
            "a separation of {separation} was accepted"
        );
    }
}

#[test]
fn a_site_class_needs_a_positive_separation() {
    let text = "class\tspacing\tseparation\tfill_pct\tmin_from_spawn\ttier_lo\ttier_hi\n\
                camp\t400\t0\t35\t0\t0\t2\n";
    assert!(error(with_site_classes(text)).contains("separation must be positive"));
}

#[test]
fn a_site_class_needs_a_usable_fill_percentage() {
    for fill in ["0", "101"] {
        let text = format!(
            "class\tspacing\tseparation\tfill_pct\tmin_from_spawn\ttier_lo\ttier_hi\n\
             camp\t400\t240\t{fill}\t0\t0\t2\n"
        );
        assert!(
            error(with_site_classes(&text)).contains("fill_pct must be 1 to 100"),
            "a fill of {fill} was accepted"
        );
    }
}

#[test]
fn a_site_class_cannot_start_behind_the_origin() {
    let text = "class\tspacing\tseparation\tfill_pct\tmin_from_spawn\ttier_lo\ttier_hi\n\
                camp\t400\t240\t35\t-1\t0\t2\n";
    assert!(error(with_site_classes(text)).contains("min_from_spawn"));
}

#[test]
fn a_site_class_needs_a_tier_window_that_makes_sense() {
    let text = "class\tspacing\tseparation\tfill_pct\tmin_from_spawn\ttier_lo\ttier_hi\n\
                camp\t400\t240\t35\t0\t2\t1\n";
    assert!(error(with_site_classes(text)).contains("tier_lo must not exceed tier_hi"));
}

#[test]
fn a_site_class_cannot_point_at_a_tier_that_does_not_exist() {
    let text = "class\tspacing\tseparation\tfill_pct\tmin_from_spawn\ttier_lo\ttier_hi\n\
                camp\t400\t240\t35\t0\t0\t9\n";
    assert!(error(with_site_classes(text)).contains("tier_hi 9"));
}

#[test]
fn two_site_classes_cannot_share_a_name() {
    let text = "class\tspacing\tseparation\tfill_pct\tmin_from_spawn\ttier_lo\ttier_hi\n\
                camp\t400\t240\t35\t0\t0\t2\n\
                camp\t800\t400\t35\t0\t0\t2\n";
    assert!(error(with_site_classes(text)).contains("duplicate class"));
}

#[test]
fn a_site_class_no_site_uses_would_place_nothing() {
    let text = "class\tspacing\tseparation\tfill_pct\tmin_from_spawn\ttier_lo\ttier_hi\n\
                camp\t400\t240\t35\t0\t0\t2\n\
                ruin\t800\t400\t35\t0\t0\t2\n";
    assert!(error(with_site_classes(text)).contains("has no site using it"));
}

#[test]
fn a_site_needs_a_class_that_exists() {
    let text = "site\tclass\tweight\tfootprint\ncampfire\tnowhere\t1\t3\n";
    assert!(error(with_sites(text)).contains("has no row in site_classes.tsv"));
}

#[test]
fn a_site_needs_a_positive_weight() {
    let text = "site\tclass\tweight\tfootprint\ncampfire\tcamp\t0\t3\n";
    assert!(error(with_sites(text)).contains("weight must be positive"));
}

#[test]
fn a_site_footprint_must_be_odd_and_positive() {
    for footprint in ["0", "4"] {
        let text = format!("site\tclass\tweight\tfootprint\ncampfire\tcamp\t1\t{footprint}\n");
        assert!(
            error(with_sites(&text)).contains("footprint must be"),
            "a footprint of {footprint} was accepted"
        );
    }
}

#[test]
fn a_site_wider_than_its_separation_would_overlap_its_neighbour() {
    let text = "site\tclass\tweight\tfootprint\ncampfire\tcamp\t1\t401\n";
    assert!(error(with_sites(text)).contains("wider than class"));
}

#[test]
fn two_sites_cannot_share_a_name() {
    let text = "site\tclass\tweight\tfootprint\n\
                campfire\tcamp\t1\t3\n\
                campfire\tcamp\t1\t5\n";
    assert!(error(with_sites(text)).contains("duplicate site"));
}

#[test]
fn the_shipped_site_tables_parse() {
    let rules = parse(Tables {
        world: include_str!("../../../../project/data/world.tsv"),
        tiers: include_str!("../../../../project/data/tiers.tsv"),
        materials: include_str!("../../../../project/data/materials.tsv"),
        biomes: include_str!("../../../../project/data/biomes.tsv"),
        site_classes: include_str!("../../../../project/data/site_classes.tsv"),
        sites: include_str!("../../../../project/data/sites.tsv"),
    })
    .unwrap();
    for class in rules.site_classes() {
        assert!(
            !rules.sites_in(class).is_empty(),
            "class {:?} places nothing",
            rules.site_class(class).name
        );
    }
}
