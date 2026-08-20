//! Turning a generated chunk into the bytes Godot's tilemap reads.
//!
//! Painting a chunk cell by cell would cross the extension boundary a thousand
//! times and marshal a value each time. `TileMapLayer` accepts its whole
//! contents as one byte array instead, so the entire chunk goes over in a single
//! call. This module builds that array, which is why it is pure and testable
//! while `bridge.rs` only hands the result to Godot.
//!
//! The layout is not in Godot's class reference. It was read from
//! `set_tile_map_data_from_array` in `scene/2d/tile_map_layer.cpp`: a two byte
//! little endian format version, currently 0, then one twelve byte record per
//! cell. Every field is little endian.

use worldgen::{CHUNK_TILES, ChunkView, MaterialId};

/// The format version `TileMapLayer` writes and expects. Godot refuses anything
/// above what it knows, so a future version will fail loudly rather than be
/// misread.
const DATA_FORMAT: u16 = 0;

/// Bytes per cell record: x, y, source, atlas x, atlas y, alternative.
const RECORD: usize = 12;

/// Which tileset source the ground atlas is, matching `ground.tileset.tres`.
const GROUND_SOURCE: u16 = 0;

/// How many ground variants the placeholder atlas actually holds.
///
/// Terrain art is a later design, so every material currently maps onto this
/// handful of tiles and biomes are told apart in the preview tool rather than in
/// game. When a real atlas lands, this and [`atlas_column`] are what change.
pub const PLACEHOLDER_VARIANTS: u16 = 3;

/// The whole chunk as one `tile_map_data` array.
///
/// Local coordinates, `0..CHUNK_TILES`, because each chunk gets its own layer.
/// That is not tidiness: `TileMapLayer` serialises a coordinate as an `i16`, so
/// world coordinates would silently wrap past 32767.
#[must_use]
pub fn tile_map_data(view: &ChunkView) -> Vec<u8> {
    let cells = (CHUNK_TILES * CHUNK_TILES) as usize;
    let mut out = Vec::with_capacity(2 + cells * RECORD);
    out.extend_from_slice(&DATA_FORMAT.to_le_bytes());
    for (local, tile) in view.interior() {
        // The interior is bounded by CHUNK_TILES, far inside i16.
        out.extend_from_slice(&(local.x as i16).to_le_bytes());
        out.extend_from_slice(&(local.y as i16).to_le_bytes());
        out.extend_from_slice(&GROUND_SOURCE.to_le_bytes());
        out.extend_from_slice(&atlas_column(tile.material).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    out
}

/// Which atlas column a material draws as.
///
/// A modulo while the atlas is a placeholder, so a new material never indexes
/// past the art that exists. It means two materials can share a look, which the
/// preview tool exists to see through.
#[must_use]
pub fn atlas_column(material: MaterialId) -> u16 {
    u16::from(material.0) % PLACEHOLDER_VARIANTS
}
