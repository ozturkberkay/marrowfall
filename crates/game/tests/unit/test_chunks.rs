use game::{Chunks, STEP_LIMIT};
use worldgen::{
    CHUNK_TILES, ChunkCoord, ChunkView, IVec2, MaterialId, Tables, Tile, TileFlags, World,
    generate_chunk, parse,
};

/// A flat, walkable world, so a test can put exactly the obstacle it is about
/// into an otherwise featureless chunk.
fn flat_world() -> World {
    let rules = parse(Tables {
        world: "region_pitch_tiles\tregion_jitter_pct\thome_bubble_tiles\n256\t0\t128\n",
        tiers: "tier\tinner_tiles\tharder_stray\teasier_stray\tstray_pct\n0\t0\t0\t0\t0\n",
        materials: "material\tblocks_walk\tblocks_jump\tblocks_shot\nsoil\t0\t0\t0\n",
        // Zero amplitude, so every tile is height 0 and a test can reason about
        // the one tile it changed.
        biomes: "biome\ttier\tweight\tground\theight_amp\theight_period\nlow\t0\t10\tsoil\t0\t240\n",
        site_classes: "class\tspacing\tseparation\tfill_pct\tmin_from_spawn\ttier_lo\ttier_hi\ncamp\t400\t240\t35\t0\t0\t0\n",
        sites: "site\tclass\tweight\tfootprint\ncampfire\tcamp\t1\t3\n",
    })
    .unwrap();
    World::new(rules, 7)
}

/// The origin chunk, with `edits` applied by local coordinate.
fn chunk_with(edits: &[(IVec2, Tile)]) -> Chunks {
    let view = generate_chunk(&flat_world(), ChunkCoord::new(0, 0));
    let mut tiles: Vec<Tile> = view.tiles().to_vec();
    let side = (CHUNK_TILES + 2) as usize;
    for (local, tile) in edits {
        let x = (local.x + 1) as usize;
        let y = (local.y + 1) as usize;
        tiles[y * side + x] = *tile;
    }
    let mut chunks = Chunks::default();
    chunks.insert(std::sync::Arc::new(ChunkView::from_tiles(
        view.coord, tiles,
    )));
    chunks
}

fn wall() -> Tile {
    Tile {
        material: MaterialId(0),
        height: 0,
        flags: TileFlags::BLOCKS_WALK,
    }
}

fn at_height(height: i8) -> Tile {
    Tile {
        material: MaterialId(0),
        height,
        flags: TileFlags::NONE,
    }
}

#[test]
fn flat_open_ground_is_walkable() {
    let chunks = chunk_with(&[]);
    assert!(chunks.can_step(IVec2::new(4, 4), IVec2::new(5, 4)));
}

#[test]
fn a_blocking_tile_is_refused() {
    let chunks = chunk_with(&[(IVec2::new(5, 4), wall())]);
    assert!(!chunks.can_step(IVec2::new(4, 4), IVec2::new(5, 4)));
}

#[test]
fn one_step_up_or_down_is_walkable_and_two_is_not() {
    // Symmetric, so terraced ground cannot produce a basin with no way out.
    for direction in [1, -1] {
        let one = at_height(direction * STEP_LIMIT);
        let two = at_height(direction * (STEP_LIMIT + 1));
        let chunks = chunk_with(&[(IVec2::new(5, 4), one), (IVec2::new(6, 4), two)]);
        assert!(
            chunks.can_step(IVec2::new(4, 4), IVec2::new(5, 4)),
            "a step of {direction} was refused"
        );
        assert!(
            !chunks.can_step(IVec2::new(4, 4), IVec2::new(6, 4)),
            "a step of {} was allowed",
            direction * (STEP_LIMIT + 1)
        );
    }
}

#[test]
fn a_diagonal_through_a_cliff_corner_is_refused() {
    // Both orthogonal neighbours blocked, destination open. Without the corner
    // rule an entity slips between two cliffs that visually meet.
    let chunks = chunk_with(&[(IVec2::new(5, 4), wall()), (IVec2::new(4, 5), wall())]);
    assert!(!chunks.can_step(IVec2::new(4, 4), IVec2::new(5, 5)));
}

#[test]
fn a_diagonal_past_a_single_wall_corner_is_also_refused() {
    // Strict, not lenient: both orthogonal neighbours must be clear, not just
    // one. That matches what movement can actually execute, since it resolves
    // one axis at a time, so a path built on this predicate is always walkable.
    // Going around the corner still works, in two steps.
    let chunks = chunk_with(&[(IVec2::new(5, 4), wall())]);
    assert!(!chunks.can_step(IVec2::new(4, 4), IVec2::new(5, 5)));
    assert!(chunks.can_step(IVec2::new(4, 4), IVec2::new(4, 5)));
    assert!(chunks.can_step(IVec2::new(4, 5), IVec2::new(5, 5)));
}

#[test]
fn a_destination_in_an_absent_chunk_is_refused() {
    // Not loaded is not open. Treating it as walkable would let the player step
    // into a streaming gap.
    let chunks = chunk_with(&[]);
    let outside = IVec2::new(CHUNK_TILES + 5, 0);
    assert_eq!(chunks.tile(outside), None);
    assert!(!chunks.can_step(IVec2::new(4, 4), outside));
}

#[test]
fn standing_on_an_absent_tile_does_not_trap_an_entity() {
    // The chunk underfoot can be evicted while the player stands on it. Refusing
    // to move from an unknown tile would freeze him where he stands, forever.
    let chunks = chunk_with(&[]);
    let nowhere = IVec2::new(-CHUNK_TILES - 5, 0);
    assert_eq!(chunks.tile(nowhere), None);
    assert!(chunks.can_step(nowhere, IVec2::new(0, 0)));
}

#[test]
fn a_chunk_can_be_dropped_and_stops_answering() {
    let mut chunks = chunk_with(&[]);
    assert!(chunks.tile(IVec2::new(4, 4)).is_some());
    chunks.remove(ChunkCoord::new(0, 0));
    assert!(chunks.is_empty());
    assert_eq!(chunks.tile(IVec2::new(4, 4)), None);
}

#[test]
fn inserting_a_chunk_twice_replaces_it_rather_than_duplicating() {
    let view = std::sync::Arc::new(generate_chunk(&flat_world(), ChunkCoord::new(0, 0)));
    let mut chunks = Chunks::default();
    chunks.insert(std::sync::Arc::clone(&view));
    chunks.insert(view);
    assert_eq!(chunks.len(), 1);
}
