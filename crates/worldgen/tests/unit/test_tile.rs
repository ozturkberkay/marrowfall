use std::hint::black_box;

use worldgen::{MaterialId, Tile, TileFlags};

/// Hides a flag set from the optimiser.
///
/// Every reader on `TileFlags` is a `const fn`, so `BLOCKS_WALK.blocks_walk()` is
/// a wholly constant expression that LLVM is free to fold at compile time. When
/// it does, no code runs and coverage records the function as never executed,
/// which is how this file read as 60 percent covered on one machine and 100 on
/// another. Routing the value through `black_box` forces the call to happen.
fn opaque(flags: TileFlags) -> TileFlags {
    black_box(flags)
}

#[test]
fn a_tile_with_no_flags_permits_everything() {
    let flags = opaque(TileFlags::NONE);
    assert!(!flags.blocks_walk());
    assert!(!flags.blocks_jump());
    assert!(!flags.blocks_shot());
}

#[test]
fn each_flag_reads_back_on_its_own() {
    assert!(opaque(TileFlags::BLOCKS_WALK).blocks_walk());
    assert!(opaque(TileFlags::BLOCKS_JUMP).blocks_jump());
    assert!(opaque(TileFlags::BLOCKS_SHOT).blocks_shot());
}

#[test]
fn a_flag_does_not_imply_any_other() {
    // A low obstacle blocks walking and nothing else, which is what makes it
    // jumpable and shootable over.
    let low = opaque(TileFlags::BLOCKS_WALK);
    assert!(low.blocks_walk());
    assert!(!low.blocks_jump());
    assert!(!low.blocks_shot());
}

#[test]
fn flags_combine_and_both_read_back() {
    // A full wall: blocked on foot and in the air.
    let wall = opaque(TileFlags::BLOCKS_WALK)
        .with(opaque(TileFlags::BLOCKS_JUMP))
        .with(opaque(TileFlags::BLOCKS_SHOT));
    assert!(wall.blocks_walk());
    assert!(wall.blocks_jump());
    assert!(wall.blocks_shot());
}

#[test]
fn combining_a_flag_twice_changes_nothing() {
    let once = opaque(TileFlags::BLOCKS_WALK);
    assert_eq!(once.with(opaque(TileFlags::BLOCKS_WALK)), once);
}

#[test]
fn a_tile_is_three_bytes() {
    // The whole world is made of these, and they cross the boundary once per
    // streamed chunk, so every byte added here is 1156 bytes per chunk. Three
    // one-byte fields with no padding is the floor until a field has to grow.
    assert_eq!(size_of::<Tile>(), 3);
}

#[test]
fn a_tile_reports_what_it_was_built_with() {
    let tile = Tile {
        material: MaterialId(3),
        height: -2,
        flags: opaque(TileFlags::BLOCKS_WALK),
    };
    assert_eq!(tile.material, MaterialId(3));
    assert_eq!(tile.height, -2);
    assert!(tile.flags.blocks_walk());
}

#[test]
fn the_raw_bits_carry_every_flag_that_is_set() {
    // `bits` feeds the world hash and, later, a save file, so it has to report
    // all three and nothing more.
    assert_eq!(opaque(TileFlags::NONE).bits(), 0);
    let wall = opaque(TileFlags::BLOCKS_WALK)
        .with(opaque(TileFlags::BLOCKS_JUMP))
        .with(opaque(TileFlags::BLOCKS_SHOT));
    assert_eq!(wall.bits(), 0b111);
    assert_eq!(opaque(TileFlags::BLOCKS_JUMP).bits(), 0b010);
}
