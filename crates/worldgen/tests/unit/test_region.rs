use worldgen::{HEIGHT_RANGE, IVec2, Tables, World, WorldRules, height_at, parse, region_at};

/// A pitch far smaller than the shipped one, so a test can walk across several
/// regions without iterating millions of tiles.
const WORLD_TSV: &str = "region_pitch_tiles\tregion_jitter_pct\thome_bubble_tiles\n64\t60\t40\n";
const TIERS_TSV: &str = "\
tier\tinner_tiles\tharder_stray\teasier_stray\tstray_pct
0\t0\t0\t0\t0
1\t200\t1\t0\t0
2\t400\t1\t1\t0
3\t600\t1\t2\t0
";
const MATERIALS_TSV: &str = "\
material\tblocks_walk\tblocks_jump\tblocks_shot
soil\t0\t0\t0
stone\t0\t0\t0
rock\t1\t1\t1
";
const BIOMES_TSV: &str = "\
biome\ttier\tweight\tground\theight_amp\theight_period
low\t0\t10\tsoil\t3\t240
mid\t1\t10\tsoil\t4\t320
high\t2\t10\tstone\t6\t480
far\t3\t10\tstone\t8\t640
";

fn rules() -> WorldRules {
    parse(Tables {
        world: WORLD_TSV,
        tiers: TIERS_TSV,
        materials: MATERIALS_TSV,
        biomes: BIOMES_TSV,
        site_classes: "class\tspacing\tseparation\tfill_pct\tmin_from_spawn\ttier_lo\ttier_hi\ncamp\t400\t240\t35\t0\t0\t0\n",
        sites: "site\tclass\tweight\tfootprint\ncampfire\tcamp\t1\t3\n",
    })
    .unwrap()
}

fn world(seed: u64) -> World {
    World::new(rules(), seed)
}

#[test]
fn a_region_is_the_same_however_often_it_is_asked_for() {
    let w = world(7);
    let tile = IVec2::new(123, -456);
    assert_eq!(region_at(&w, tile), region_at(&w, tile));
}

#[test]
fn a_clone_of_a_world_generates_the_same_region() {
    // `Clone` rebuilds the noise from the seed rather than copying it, so this
    // is the test that the rebuild is equivalent.
    let w = world(7);
    let copy = w.clone();
    for tile in [IVec2::ZERO, IVec2::new(500, -500), IVec2::new(-9, 4321)] {
        assert_eq!(region_at(&w, tile), region_at(&copy, tile));
        assert_eq!(height_at(&w, tile), height_at(&copy, tile));
    }
}

#[test]
fn two_seeds_lay_the_regions_out_differently() {
    let (a, b) = (world(1), world(2));
    let differs = (0..400)
        .any(|i| region_at(&a, IVec2::new(i, 0)).point != region_at(&b, IVec2::new(i, 0)).point);
    assert!(differs, "the seed did not reach the region lattice");
}

#[test]
fn a_region_point_is_never_far_from_the_tiles_it_owns() {
    // The lattice searches a 3 by 3 block, which is only sound because `parse`
    // bounds jitter to half the pitch and the boundary warp is a small fraction
    // of it. If either grew, a tile would end up assigned to a point far outside
    // its own neighbourhood, which is what this catches.
    let w = world(11);
    let pitch = i64::from(w.rules().region_pitch());
    for i in 0..2000 {
        let tile = IVec2::new(i * 3 - 3000, i * 7 - 7000);
        let point = region_at(&w, tile).point;
        let dx = point.at.x - i64::from(tile.x);
        let dy = point.at.y - i64::from(tile.y);
        let distance = ((dx * dx + dy * dy) as f64).sqrt() as i64;
        assert!(
            distance < pitch * 2,
            "{tile:?} was given a point {distance} tiles away, over two pitches"
        );
    }
}

#[test]
fn a_region_boundary_is_not_a_straight_line() {
    // Raw Voronoi gives straight edges, which read as artificial. The boundary
    // warp is what bends them, so crossing the same boundary along two parallel
    // lines should not happen at the same place.
    let w = world(11);
    let crossing = |y: i32| {
        let start = region_at(&w, IVec2::new(-400, y)).point;
        (-400..400).find(|&x| region_at(&w, IVec2::new(x, y)).point != start)
    };
    let mut seen = std::collections::BTreeSet::new();
    for y in (-200..200).step_by(20) {
        if let Some(x) = crossing(y) {
            seen.insert(x);
        }
    }
    assert!(
        seen.len() > 3,
        "boundaries crossed at only {seen:?}, so the warp is not bending them"
    );
}

#[test]
fn a_region_holds_together_across_its_middle() {
    // Contiguity is what stops biome soup. Walking away from a point should stay
    // in its region for a while rather than flickering between neighbours.
    let w = world(3);
    let centre = region_at(&w, IVec2::ZERO);
    let same = (0..16)
        .filter(|&i| region_at(&w, IVec2::new(i, 0)).point == centre.point)
        .count();
    assert!(
        same >= 8,
        "only {same} of 16 adjacent tiles shared a region"
    );
}

#[test]
fn nothing_inside_the_home_bubble_is_above_tier_zero() {
    // The guarantee the whole tier rule exists to keep, and it only holds
    // because the bubble is enforced on the region with a pitch of slack: a tile
    // just inside the bubble can belong to a region whose point sits outside it.
    // Many seeds, because a per-seed lattice could pass by luck.
    let bubble = rules().home_bubble();
    for seed in 0..64 {
        let w = world(seed);
        let mut tile = IVec2::new(-(bubble as i32), -(bubble as i32));
        while tile.y <= bubble as i32 {
            tile.x = -(bubble as i32);
            while tile.x <= bubble as i32 {
                let inside = i64::from(tile.x).pow(2) + i64::from(tile.y).pow(2) <= bubble * bubble;
                if inside {
                    let region = region_at(&w, tile);
                    assert_eq!(
                        region.tier, 0,
                        "seed {seed}: {tile:?} is inside the bubble but tier {}",
                        region.tier
                    );
                }
                tile.x += 7;
            }
            tile.y += 7;
        }
    }
}

#[test]
fn tier_never_falls_as_you_walk_outward() {
    // With the dial at zero a region's tier is purely its distance, so sampling
    // along a ray must be monotonic. This is the property the dial trades away
    // when it opens, which is why it ships closed.
    let w = world(5);
    for (dx, dy) in [(1, 0), (0, 1), (-1, 0), (0, -1), (1, 1), (-1, 1)] {
        let mut highest = 0;
        for step in 0..900 {
            let tier = region_at(&w, IVec2::new(dx * step, dy * step)).tier;
            assert!(
                tier >= highest,
                "tier fell from {highest} to {tier} at step {step} along ({dx}, {dy})"
            );
            highest = tier;
        }
    }
}

#[test]
fn the_frontier_tier_is_reached_and_then_held() {
    let w = world(5);
    let max = w.rules().max_tier();
    let far = region_at(&w, IVec2::new(20_000, 20_000)).tier;
    assert_eq!(far, max, "the outermost band must run forever");
}

#[test]
fn a_biome_always_comes_from_its_own_tier() {
    let w = world(9);
    for step in 0..600 {
        let region = region_at(&w, IVec2::new(step, step / 2));
        assert!(
            w.rules().biomes_in(region.tier).contains(&region.biome),
            "tier {} was given a biome from another tier",
            region.tier
        );
    }
}

#[test]
fn height_stays_inside_the_range_movement_assumes() {
    // Movement subtracts two heights, so a value outside this range would make
    // the `i8` arithmetic in the step rule overflow.
    let w = world(13);
    for y in -200..200 {
        for x in -200..200 {
            let h = height_at(&w, IVec2::new(x * 3, y * 3));
            assert!(HEIGHT_RANGE.contains(&h), "height {h} out of range");
        }
    }
}

#[test]
fn height_is_not_flat() {
    let w = world(13);
    let mut seen = std::collections::BTreeSet::new();
    for i in 0..500 {
        seen.insert(height_at(&w, IVec2::new(i, i / 3)));
    }
    assert!(seen.len() > 2, "the height field produced only {seen:?}");
}

#[test]
fn height_is_flat_within_a_terrace() {
    // Terraced, not sloped: a change of height is a step, so neighbouring tiles
    // nearly always share one. Measured at the shipped amplitude and period this
    // is 95 percent, giving plateaus about 23 tiles across. Below about 88 the
    // ground has become gravel and the period wants raising.
    let w = world(13);
    const SAMPLES: i32 = 2000;
    let same = (0..SAMPLES)
        .filter(|&i| height_at(&w, IVec2::new(i, 0)) == height_at(&w, IVec2::new(i + 1, 0)))
        .count();
    let pct = same * 100 / SAMPLES as usize;
    assert!(
        pct >= 88,
        "only {pct} percent of neighbours shared a height"
    );
}
