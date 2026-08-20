use render::tiles::{self, PLACEHOLDER_VARIANTS, atlas_column};
use worldgen::{CHUNK_TILES, ChunkCoord, MaterialId, Tables, World, generate_chunk, parse};

fn view() -> worldgen::ChunkView {
    let rules = parse(Tables {
        world: "region_pitch_tiles\tregion_jitter_pct\thome_bubble_tiles\n256\t60\t128\n",
        tiers: "tier\tinner_tiles\tharder_stray\teasier_stray\tstray_pct\n0\t0\t0\t0\t0\n",
        materials: "material\tblocks_walk\tblocks_jump\tblocks_shot\nsoil\t0\t0\t0\n",
        biomes: "biome\ttier\tweight\tground\theight_amp\theight_period\nlow\t0\t10\tsoil\t3\t240\n",
        site_classes: "class\tspacing\tseparation\tfill_pct\tmin_from_spawn\ttier_lo\ttier_hi\ncamp\t400\t240\t35\t0\t0\t0\n",
        sites: "site\tclass\tweight\tfootprint\ncampfire\tcamp\t1\t3\n",
    })
    .unwrap();
    generate_chunk(&World::new(rules, 7), ChunkCoord::new(1, -2))
}

#[test]
fn the_blob_is_a_header_plus_one_twelve_byte_record_per_cell() {
    let data = tiles::tile_map_data(&view());
    let cells = (CHUNK_TILES * CHUNK_TILES) as usize;
    assert_eq!(data.len(), 2 + cells * 12);
}

#[test]
fn the_header_is_the_format_version_godot_expects() {
    // Godot refuses a version above what it knows, so a wrong header here would
    // fail loudly rather than paint nonsense.
    let data = tiles::tile_map_data(&view());
    assert_eq!(u16::from_le_bytes([data[0], data[1]]), 0);
}

#[test]
fn coordinates_are_chunk_local_and_stay_inside_an_i16() {
    // `TileMapLayer` serialises a coordinate as an i16 and wraps silently past
    // 32767, which is why each chunk gets its own layer with local coordinates.
    let data = tiles::tile_map_data(&view());
    for record in data[2..].chunks_exact(12) {
        let x = i16::from_le_bytes([record[0], record[1]]);
        let y = i16::from_le_bytes([record[2], record[3]]);
        assert!((0..CHUNK_TILES as i16).contains(&x), "x {x} is not local");
        assert!((0..CHUNK_TILES as i16).contains(&y), "y {y} is not local");
    }
}

#[test]
fn the_first_record_is_the_local_origin_in_row_major_order() {
    let data = tiles::tile_map_data(&view());
    assert_eq!(i16::from_le_bytes([data[2], data[3]]), 0);
    assert_eq!(i16::from_le_bytes([data[4], data[5]]), 0);
    // Second record is one step along x, not y.
    assert_eq!(i16::from_le_bytes([data[14], data[15]]), 1);
    assert_eq!(i16::from_le_bytes([data[16], data[17]]), 0);
}

#[test]
fn every_record_names_the_ground_source_and_no_transform() {
    let data = tiles::tile_map_data(&view());
    for record in data[2..].chunks_exact(12) {
        assert_eq!(u16::from_le_bytes([record[4], record[5]]), 0, "source");
        assert_eq!(u16::from_le_bytes([record[8], record[9]]), 0, "atlas y");
        // Bits 12, 13 and 14 of the alternative carry flip and transpose. A
        // flipped diamond mirrors its lighting, so terrain never sets them.
        assert_eq!(
            u16::from_le_bytes([record[10], record[11]]),
            0,
            "alternative"
        );
    }
}

#[test]
fn no_material_indexes_past_the_art_that_exists() {
    // The atlas is a placeholder, so a material added to the table must not
    // point at a tile that was never drawn.
    for id in 0..=u8::MAX {
        assert!(atlas_column(MaterialId(id)) < PLACEHOLDER_VARIANTS);
    }
}
