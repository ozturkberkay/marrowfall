//! The stray dial: whether a region ignores its distance, and how far.
//!
//! Zero in the shipped data means none of this runs in a real world yet, which
//! is exactly why it needs testing here: the day someone raises `stray_pct` is
//! the wrong day to find out the arms were never exercised.

use worldgen::{IVec2, Tables, World, WorldRules, parse, region_at};

const WORLD_TSV: &str = "region_pitch_tiles\tregion_jitter_pct\thome_bubble_tiles\n64\t60\t0\n";
const MATERIALS_TSV: &str = "material\tblocks_walk\tblocks_jump\tblocks_shot\nsoil\t0\t0\t0\n";
const BIOMES_TSV: &str = "\
biome\ttier\tweight\tground\theight_amp\theight_period
t0\t0\t10\tsoil\t3\t240
t1\t1\t10\tsoil\t3\t240
t2\t2\t10\tsoil\t3\t240
t3\t3\t10\tsoil\t3\t240
";

/// Four bands, with the stray columns supplied per test.
fn tiers(harder: u8, easier: u8, pct: u8) -> String {
    let mut out = String::from("tier\tinner_tiles\tharder_stray\teasier_stray\tstray_pct\n");
    for (tier, inner) in [(0, 0), (1, 400), (2, 800), (3, 1200)] {
        // Band 0 never strays whatever the table says, because the home bubble
        // covers it. These tests sample band 1 and up.
        out.push_str(&format!("{tier}\t{inner}\t{harder}\t{easier}\t{pct}\n"));
    }
    out
}

fn rules(harder: u8, easier: u8, pct: u8) -> WorldRules {
    parse(Tables {
        world: WORLD_TSV,
        tiers: &tiers(harder, easier, pct),
        materials: MATERIALS_TSV,
        biomes: BIOMES_TSV,
        site_classes: "class\tspacing\tseparation\tfill_pct\tmin_from_spawn\ttier_lo\ttier_hi\ncamp\t400\t240\t35\t0\t0\t0\n",
        sites: "site\tclass\tweight\tfootprint\ncampfire\tcamp\t1\t3\n",
    })
    .unwrap()
}

/// Every tier seen among regions whose distance falls inside one band.
///
/// Filtered by distance rather than walked along a line: a diagonal changes its
/// distance from the origin as it goes, so it would leave the band under test
/// and the result would be about band boundaries instead of about stray.
fn tiers_seen(harder: u8, easier: u8, pct: u8, band: std::ops::Range<i64>) -> Vec<u8> {
    let world = World::new(rules(harder, easier, pct), 7);
    let mut seen = std::collections::BTreeSet::new();
    for y in -40..40 {
        for x in -40..40 {
            let tile = IVec2::new(x * 40, y * 40);
            let region = region_at(&world, tile);
            if band.contains(&region.point.distance_tiles()) {
                seen.insert(region.tier);
            }
        }
    }
    assert!(!seen.is_empty(), "no region sampled inside {band:?}");
    seen.into_iter().collect()
}

/// Band 2 with room on both sides, so a stray in either direction is visible.
const BAND_2: std::ops::Range<i64> = 800..1200;

#[test]
fn a_closed_dial_never_strays() {
    // The shipped configuration. Band 2 runs 800 to 1200, so sampling inside it
    // must give tier 2 and nothing else.
    assert_eq!(tiers_seen(1, 1, 0, BAND_2), vec![2]);
}

#[test]
fn an_open_dial_with_only_harder_allowed_strays_upward_only() {
    let seen = tiers_seen(1, 0, 100, BAND_2);
    assert!(seen.contains(&3), "nothing strayed harder: {seen:?}");
    assert!(!seen.contains(&1), "something strayed easier: {seen:?}");
}

#[test]
fn an_open_dial_with_only_easier_allowed_strays_downward_only() {
    let seen = tiers_seen(0, 1, 100, BAND_2);
    assert!(seen.contains(&1), "nothing strayed easier: {seen:?}");
    assert!(!seen.contains(&3), "something strayed harder: {seen:?}");
}

#[test]
fn an_open_dial_with_both_allowed_goes_both_ways() {
    // The coin flip the table does not spell out. Both arms must be reachable,
    // or half the dial is decoration.
    let seen = tiers_seen(1, 1, 100, BAND_2);
    assert!(seen.contains(&1) && seen.contains(&3), "only saw {seen:?}");
}

#[test]
fn a_dial_open_but_with_no_room_either_way_does_not_move() {
    // `stray_pct` says yes and both magnitudes say nowhere. The region stays put
    // rather than picking a direction by accident.
    assert_eq!(tiers_seen(0, 0, 100, BAND_2), vec![2]);
}

#[test]
fn straying_harder_cannot_climb_past_the_last_tier() {
    // The outermost band has nowhere above it, so a harder stray must clamp
    // rather than name a tier with no biomes and panic.
    let world = World::new(rules(1, 0, 100), 7);
    let max = world.rules().max_tier();
    for i in 0..400 {
        let tier = region_at(&world, IVec2::new(4000 + i, 4000 - i)).tier;
        assert!(tier <= max, "tier {tier} is above the last band");
    }
}

#[test]
fn straying_easier_cannot_fall_below_the_first_tier() {
    let world = World::new(rules(0, 3, 100), 7);
    for i in 0..400 {
        // Band 1, where a stray of 3 would underflow a smaller integer.
        let _ = region_at(&world, IVec2::new(500 + i, 500 - i)).tier;
    }
}

#[test]
fn no_jitter_puts_every_region_point_on_a_cell_centre() {
    // The `span == 0` path. Worth keeping working: it is the setting that makes
    // a world reproducible by hand when debugging the lattice.
    let no_jitter = WORLD_TSV.replace("\t60\t", "\t0\t");
    let rules = parse(Tables {
        world: &no_jitter,
        tiers: &tiers(0, 0, 0),
        materials: MATERIALS_TSV,
        biomes: BIOMES_TSV,
        site_classes: "class\tspacing\tseparation\tfill_pct\tmin_from_spawn\ttier_lo\ttier_hi\ncamp\t400\t240\t35\t0\t0\t0\n",
        sites: "site\tclass\tweight\tfootprint\ncampfire\tcamp\t1\t3\n",
    })
    .unwrap();
    let pitch = i64::from(rules.region_pitch());
    let world = World::new(rules, 7);
    for i in 0..200 {
        let point = region_at(&world, IVec2::new(i * 5, i * 3)).point;
        assert_eq!(
            (point.at.x - pitch / 2).rem_euclid(pitch),
            0,
            "point {point:?} is not on a cell centre"
        );
    }
}

#[test]
fn every_biome_in_a_tier_gets_picked_eventually() {
    // The weighted walk returns the last entry by falling out of its loop, so a
    // tier with several biomes exercises both the early return and the tail.
    let many = "\
biome\ttier\tweight\tground\theight_amp\theight_period
a\t0\t5\tsoil\t3\t240
b\t0\t5\tsoil\t3\t240
c\t0\t5\tsoil\t3\t240
";
    let rules = parse(Tables {
        world: WORLD_TSV,
        tiers: "tier\tinner_tiles\tharder_stray\teasier_stray\tstray_pct\n0\t0\t0\t0\t0\n",
        materials: MATERIALS_TSV,
        biomes: many,
        site_classes: "class\tspacing\tseparation\tfill_pct\tmin_from_spawn\ttier_lo\ttier_hi\ncamp\t400\t240\t35\t0\t0\t0\n",
        sites: "site\tclass\tweight\tfootprint\ncampfire\tcamp\t1\t3\n",
    })
    .unwrap();
    let world = World::new(rules, 7);
    let mut seen = std::collections::BTreeSet::new();
    for i in 0..500 {
        seen.insert(region_at(&world, IVec2::new(i * 37, i * 53)).biome);
    }
    assert_eq!(seen.len(), 3, "only {} of 3 biomes appeared", seen.len());
}

#[test]
fn a_world_prints_its_seed_when_debugged() {
    // The noise cannot be printed, so `Debug` is written out by hand. If it ever
    // stops naming the seed, a debug log stops identifying which world it is.
    let world = World::new(rules(0, 0, 0), 0xABCD);
    assert!(format!("{world:?}").contains("43981"), "{world:?}");
}
