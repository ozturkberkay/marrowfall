use std::collections::BTreeSet;

use worldgen::{IVec2, Site, Tables, World, WorldRules, parse, site_at, sites_near};

const WORLD_TSV: &str = "region_pitch_tiles\tregion_jitter_pct\thome_bubble_tiles\n700\t60\t400\n";
const TIERS_TSV: &str = "\
tier\tinner_tiles\tharder_stray\teasier_stray\tstray_pct
0\t0\t0\t0\t0
1\t3000\t0\t0\t0
2\t6000\t0\t0\t0
";
const MATERIALS_TSV: &str = "material\tblocks_walk\tblocks_jump\tblocks_shot\nsoil\t0\t0\t0\n";
const BIOMES_TSV: &str = "\
biome\ttier\tweight\tground\theight_amp\theight_period
a\t0\t10\tsoil\t3\t240
b\t1\t10\tsoil\t4\t320
c\t2\t10\tsoil\t5\t400
";

fn rules(site_classes: &str, sites: &str) -> WorldRules {
    parse(Tables {
        world: WORLD_TSV,
        tiers: TIERS_TSV,
        materials: MATERIALS_TSV,
        biomes: BIOMES_TSV,
        site_classes,
        sites,
    })
    .unwrap()
}

/// One class filling every cell, so a test sees the placement rather than the
/// fill roll.
fn always() -> World {
    World::new(
        rules(
            "class\tspacing\tseparation\tfill_pct\tmin_from_spawn\ttier_lo\ttier_hi\n\
             camp\t400\t240\t100\t0\t0\t2\n",
            "site\tclass\tweight\tfootprint\ncampfire\tcamp\t1\t3\n",
        ),
        7,
    )
}

fn class_zero() -> worldgen::SiteClassId {
    worldgen::SiteClassId(0)
}

/// Every site of class 0 across a block of cells.
fn placed(world: &World, cells: i32) -> Vec<Site> {
    (-cells..=cells)
        .flat_map(|y| (-cells..=cells).map(move |x| IVec2::new(x, y)))
        .filter_map(|cell| site_at(world, class_zero(), cell))
        .collect()
}

#[test]
fn a_cell_gives_the_same_site_every_time() {
    let world = always();
    let cell = IVec2::new(3, -5);
    assert_eq!(
        site_at(&world, class_zero(), cell),
        site_at(&world, class_zero(), cell)
    );
}

#[test]
fn a_full_class_places_one_site_per_cell() {
    let world = always();
    // The origin cell is inside `min_from_spawn = 0` and tier 0, so every cell in
    // the block qualifies.
    assert_eq!(placed(&world, 3).len(), 7 * 7);
}

#[test]
fn no_two_sites_of_a_class_come_closer_than_its_separation() {
    // The guarantee the margin exists for, and the reason `separation` is not
    // just a hopeful average.
    let world = always();
    let separation = world.rules().site_class(class_zero()).separation;
    let sites = placed(&world, 6);
    for (i, a) in sites.iter().enumerate() {
        for b in &sites[i + 1..] {
            let offset = a.at - b.at;
            let gap = offset.x.abs().max(offset.y.abs());
            assert!(
                gap > separation,
                "{:?} and {:?} are {gap} apart, under a separation of {separation}",
                a.at,
                b.at
            );
        }
    }
}

#[test]
fn a_site_sits_inside_its_own_cell() {
    let world = always();
    let spacing = world.rules().site_class(class_zero()).spacing;
    for cell in [IVec2::ZERO, IVec2::new(4, -9), IVec2::new(-11, 6)] {
        let site = site_at(&world, class_zero(), cell).unwrap();
        let local = site.at - cell * spacing;
        assert!(
            (0..spacing).contains(&local.x) && (0..spacing).contains(&local.y),
            "{:?} escaped cell {cell:?}",
            site.at
        );
    }
}

#[test]
fn the_fill_percentage_thins_the_lattice() {
    let world = World::new(
        rules(
            "class\tspacing\tseparation\tfill_pct\tmin_from_spawn\ttier_lo\ttier_hi\n\
             camp\t400\t240\t25\t0\t0\t2\n",
            "site\tclass\tweight\tfootprint\ncampfire\tcamp\t1\t3\n",
        ),
        7,
    );
    let cells = 13 * 13;
    let count = placed(&world, 6).len();
    let share = count as f64 / f64::from(cells);
    assert!(
        (0.15..0.35).contains(&share),
        "a 25 percent fill placed {count} of {cells}, a share of {share}"
    );
}

#[test]
fn a_distance_gate_keeps_a_class_away_from_the_spawn() {
    let world = World::new(
        rules(
            "class\tspacing\tseparation\tfill_pct\tmin_from_spawn\ttier_lo\ttier_hi\n\
             camp\t400\t240\t100\t2000\t0\t2\n",
            "site\tclass\tweight\tfootprint\ncampfire\tcamp\t1\t3\n",
        ),
        7,
    );
    for site in placed(&world, 10) {
        let d = f64::from(site.at.x).hypot(f64::from(site.at.y));
        assert!(
            d >= 2000.0,
            "{:?} is {d} from the origin, inside the gate",
            site.at
        );
    }
}

#[test]
fn a_tier_gate_keeps_a_class_out_of_the_wrong_bands() {
    // Tier 0 only, so nothing may appear past the first band.
    let world = World::new(
        rules(
            "class\tspacing\tseparation\tfill_pct\tmin_from_spawn\ttier_lo\ttier_hi\n\
             camp\t400\t240\t100\t0\t0\t0\n",
            "site\tclass\tweight\tfootprint\ncampfire\tcamp\t1\t3\n",
        ),
        7,
    );
    for site in placed(&world, 15) {
        let tier = worldgen::region_at(&world, site.at).tier;
        assert_eq!(tier, 0, "{:?} landed in tier {tier}", site.at);
    }
}

#[test]
fn two_classes_do_not_place_in_lockstep() {
    // The defect a class id folded into a coordinate would cause: two classes
    // filling and offsetting identically, so every ruin sat beside a camp.
    let world = World::new(
        rules(
            "class\tspacing\tseparation\tfill_pct\tmin_from_spawn\ttier_lo\ttier_hi\n\
             camp\t400\t240\t50\t0\t0\t2\n\
             ruin\t400\t240\t50\t0\t0\t2\n",
            "site\tclass\tweight\tfootprint\n\
             campfire\tcamp\t1\t3\n\
             watchtower\truin\t1\t3\n",
        ),
        7,
    );
    let mut agree = 0;
    let mut total = 0;
    for y in -8..=8 {
        for x in -8..=8 {
            let cell = IVec2::new(x, y);
            let a = site_at(&world, worldgen::SiteClassId(0), cell);
            let b = site_at(&world, worldgen::SiteClassId(1), cell);
            total += 1;
            if a.map(|s| s.at) == b.map(|s| s.at) {
                agree += 1;
            }
        }
    }
    // Two independent 50 percent fills agree on emptiness about a quarter of the
    // time and never on an identical position, so anything near total agreement
    // means they are correlated.
    assert!(
        agree * 2 < total,
        "the two classes agreed on {agree} of {total} cells"
    );
}

#[test]
fn every_kind_in_a_class_gets_placed() {
    let world = World::new(
        rules(
            "class\tspacing\tseparation\tfill_pct\tmin_from_spawn\ttier_lo\ttier_hi\n\
             ruin\t400\t240\t100\t0\t0\t2\n",
            "site\tclass\tweight\tfootprint\n\
             watchtower\truin\t3\t9\n\
             chapel\truin\t2\t13\n\
             barrow\truin\t1\t7\n",
        ),
        7,
    );
    let kinds: BTreeSet<_> = placed(&world, 12).into_iter().map(|s| s.kind).collect();
    assert_eq!(kinds.len(), 3, "only {} of 3 kinds appeared", kinds.len());
}

#[test]
fn sites_near_finds_everything_inside_its_radius_and_nothing_outside() {
    let world = always();
    let centre = IVec2::new(1234, -567);
    let radius = 1500;
    let found = sites_near(&world, centre, radius);
    for site in &found {
        let offset = site.at - centre;
        assert!(
            offset.x.abs() <= radius && offset.y.abs() <= radius,
            "{:?} is outside the radius",
            site.at
        );
    }
    // Cross-check against a brute force over a wider block of cells.
    let spacing = world.rules().site_class(class_zero()).spacing;
    let reach = radius / spacing + 3;
    let home = IVec2::new(centre.x.div_euclid(spacing), centre.y.div_euclid(spacing));
    let mut expected = 0;
    for dy in -reach..=reach {
        for dx in -reach..=reach {
            if let Some(site) = site_at(&world, class_zero(), home + IVec2::new(dx, dy)) {
                let offset = site.at - centre;
                if offset.x.abs() <= radius && offset.y.abs() <= radius {
                    expected += 1;
                }
            }
        }
    }
    assert_eq!(found.len(), expected, "the window missed a site");
}

/// The shipped tables hold one placeholder class, because the real places are not
/// chosen yet. This checks the lattice is wired to real data and actually
/// produces sites, so the machinery cannot rot while the content is undecided.
/// If it ever fails after a table edit, the question is whether the new tables
/// were meant to place nothing.
#[test]
fn the_shipped_site_tables_place_something() {
    let rules = parse(Tables {
        world: include_str!("../../../../project/data/world.tsv"),
        tiers: include_str!("../../../../project/data/tiers.tsv"),
        materials: include_str!("../../../../project/data/materials.tsv"),
        biomes: include_str!("../../../../project/data/biomes.tsv"),
        site_classes: include_str!("../../../../project/data/site_classes.tsv"),
        sites: include_str!("../../../../project/data/sites.tsv"),
    })
    .unwrap();
    let world = World::new(rules, 0x4D61_7272_6F77);
    let found = sites_near(&world, IVec2::ZERO, 4000);
    assert!(
        !found.is_empty(),
        "the shipped tables placed nothing near spawn"
    );
}
