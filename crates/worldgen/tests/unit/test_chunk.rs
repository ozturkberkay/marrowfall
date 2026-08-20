use worldgen::{
    CHUNK_TILES, ChunkCoord, GHOST_WIDTH, IVec2, Tables, World, generate_chunk, parse, tile_at,
};

const WORLD_TSV: &str = "region_pitch_tiles\tregion_jitter_pct\thome_bubble_tiles\n256\t60\t128\n";
const TIERS_TSV: &str = "\
tier\tinner_tiles\tharder_stray\teasier_stray\tstray_pct
0\t0\t0\t0\t0
1\t600\t1\t0\t0
";
const MATERIALS_TSV: &str = "\
material\tblocks_walk\tblocks_jump\tblocks_shot
soil\t0\t0\t0
stone\t0\t0\t0
";
const BIOMES_TSV: &str = "\
biome\ttier\tweight\tground\theight_amp\theight_period
low\t0\t10\tsoil\t3\t240
mid\t1\t10\tstone\t5\t400
";

fn world(seed: u64) -> World {
    World::new(
        parse(Tables {
            world: WORLD_TSV,
            tiers: TIERS_TSV,
            materials: MATERIALS_TSV,
            biomes: BIOMES_TSV,
            site_classes: "class\tspacing\tseparation\tfill_pct\tmin_from_spawn\ttier_lo\ttier_hi\ncamp\t400\t240\t35\t0\t0\t0\n",
            sites: "site\tclass\tweight\tfootprint\ncampfire\tcamp\t1\t3\n",
        })
        .unwrap(),
        seed,
    )
}

#[test]
fn a_tile_belongs_to_the_chunk_that_floors_its_coordinate() {
    // Truncating division would put tile -1 in chunk 0 and make two chunks share
    // a row of tiles.
    assert_eq!(ChunkCoord::of(IVec2::new(0, 0)), ChunkCoord::new(0, 0));
    assert_eq!(ChunkCoord::of(IVec2::new(31, 31)), ChunkCoord::new(0, 0));
    assert_eq!(ChunkCoord::of(IVec2::new(32, 32)), ChunkCoord::new(1, 1));
    assert_eq!(ChunkCoord::of(IVec2::new(-1, -1)), ChunkCoord::new(-1, -1));
    assert_eq!(
        ChunkCoord::of(IVec2::new(-32, -32)),
        ChunkCoord::new(-1, -1)
    );
    assert_eq!(
        ChunkCoord::of(IVec2::new(-33, -33)),
        ChunkCoord::new(-2, -2)
    );
}

#[test]
fn a_chunk_origin_round_trips_through_its_coordinate() {
    for coord in [
        ChunkCoord::new(0, 0),
        ChunkCoord::new(3, -7),
        ChunkCoord::new(-100, 250),
    ] {
        assert_eq!(ChunkCoord::of(coord.origin()), coord);
    }
}

#[test]
fn the_interior_is_every_tile_the_chunk_owns_and_no_ghost_cells() {
    let view = generate_chunk(&world(7), ChunkCoord::new(2, -3));
    let coords: Vec<IVec2> = view.interior().map(|(local, _)| local).collect();
    assert_eq!(coords.len() as i32, CHUNK_TILES * CHUNK_TILES);
    assert_eq!(
        coords[0],
        IVec2::new(0, 0),
        "row major from the local origin"
    );
    assert_eq!(coords[1], IVec2::new(1, 0));
    assert_eq!(coords[CHUNK_TILES as usize], IVec2::new(0, 1));
    assert!(
        coords
            .iter()
            .all(|c| (0..CHUNK_TILES).contains(&c.x) && (0..CHUNK_TILES).contains(&c.y)),
        "a ghost cell leaked into the interior"
    );
}

#[test]
fn the_ghost_cells_are_reachable_and_outside_them_is_not() {
    let view = generate_chunk(&world(7), ChunkCoord::new(0, 0));
    assert!(view.tile(IVec2::new(-GHOST_WIDTH, -GHOST_WIDTH)).is_some());
    assert!(
        view.tile(IVec2::new(
            CHUNK_TILES + GHOST_WIDTH - 1,
            CHUNK_TILES + GHOST_WIDTH - 1
        ))
        .is_some()
    );
    assert!(view.tile(IVec2::new(-GHOST_WIDTH - 1, 0)).is_none());
    assert!(
        view.tile(IVec2::new(CHUNK_TILES + GHOST_WIDTH, 0))
            .is_none()
    );
}

#[test]
fn every_tile_in_a_chunk_equals_the_same_tile_asked_for_directly() {
    // The chunk is only a cache of `tile_at`. If these ever disagreed, the
    // frontend and the simulation would be looking at different worlds.
    let w = world(11);
    let coord = ChunkCoord::new(-4, 9);
    let view = generate_chunk(&w, coord);
    let origin = coord.origin();
    for y in -GHOST_WIDTH..=CHUNK_TILES {
        for x in -GHOST_WIDTH..=CHUNK_TILES {
            let local = IVec2::new(x, y);
            if let Some(tile) = view.tile(local) {
                assert_eq!(tile, tile_at(&w, origin + local), "at {local:?}");
            }
        }
    }
}

#[test]
fn a_chunks_ghost_cells_agree_with_its_neighbours_interior() {
    // The invariant the whole ghost ring exists for. Both sides come from the same
    // pure function rather than from each other, so they cannot drift.
    let w = world(13);
    let here = ChunkCoord::new(5, -2);
    let view = generate_chunk(&w, here);
    for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, -1)] {
        let neighbour = ChunkCoord::new(here.x + dx, here.y + dy);
        let other = generate_chunk(&w, neighbour);
        // Walk the whole grid including its ghost cells and compare wherever the two overlap.
        for y in -GHOST_WIDTH..=CHUNK_TILES {
            for x in -GHOST_WIDTH..=CHUNK_TILES {
                let mine = IVec2::new(x, y);
                let Some(tile) = view.tile(mine) else {
                    continue;
                };
                // The same world tile, expressed in the neighbour's local frame.
                let theirs = mine + here.origin() - neighbour.origin();
                if let Some(same) = other.tile(theirs) {
                    assert_eq!(tile, same, "{here:?} {mine:?} vs {neighbour:?} {theirs:?}");
                }
            }
        }
    }
}

#[test]
fn generating_in_a_shuffled_order_changes_nothing() {
    // The guard against cascading generation and against any accidental shared
    // state. If a chunk ever read a neighbour, this is the test that would fail.
    let w = world(17);
    let coords: Vec<ChunkCoord> = (-2..3)
        .flat_map(|y| (-2..3).map(move |x| ChunkCoord::new(x, y)))
        .collect();
    let sequential: Vec<_> = coords.iter().map(|&c| generate_chunk(&w, c)).collect();

    // A fixed shuffle, so the test is deterministic while still not being the
    // order the world was built in.
    let mut shuffled = coords.clone();
    shuffled.sort_by_key(|c| (c.x * 7 + c.y * 13).rem_euclid(11));
    let out_of_order: Vec<_> = shuffled.iter().map(|&c| generate_chunk(&w, c)).collect();

    for view in out_of_order {
        let expected = sequential
            .iter()
            .find(|v| v.coord == view.coord)
            .expect("same set of coordinates");
        assert_eq!(
            &view, expected,
            "{:?} depended on generation order",
            view.coord
        );
    }
}

#[test]
fn generating_the_same_chunk_twice_gives_the_same_bytes() {
    let w = world(19);
    let coord = ChunkCoord::new(-9, 4);
    assert_eq!(generate_chunk(&w, coord), generate_chunk(&w, coord));
}

/// FNV-1a over the slab. Hand rolled rather than `DefaultHasher`, whose output
/// is explicitly not stable across Rust versions, which would make a pinned
/// value meaningless.
fn fnv1a(view: &worldgen::ChunkView) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for tile in view.tiles() {
        for byte in [
            tile.material.0,
            tile.height.to_le_bytes()[0],
            tile.flags.bits(),
        ] {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

#[test]
fn a_generated_chunk_is_pinned() {
    // Changing this value means every world from every seed changed. If it
    // fails, that is the question to answer rather than the number to update.
    let view = generate_chunk(&world(0x4D61_7272_6F77), ChunkCoord::new(0, 0));
    assert_eq!(fnv1a(&view), 0xAFDC_EDCE_0237_131A);
}

#[test]
fn the_pinned_chunk_is_the_same_on_a_second_pass_in_one_process() {
    // Run twice in one binary, which is what catches a randomly seeded hasher
    // having leaked into generation. A single pass always agrees with itself.
    let first = fnv1a(&generate_chunk(&world(3), ChunkCoord::new(1, 1)));
    let second = fnv1a(&generate_chunk(&world(3), ChunkCoord::new(1, 1)));
    assert_eq!(first, second);
}
