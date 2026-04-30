use game::{GROUND_VARIANTS, TerrainGrid};

#[test]
fn generation_is_deterministic() {
    assert_eq!(
        TerrainGrid::generate(7, 16, 16),
        TerrainGrid::generate(7, 16, 16)
    );
}

#[test]
fn all_variants_are_in_range() {
    let grid = TerrainGrid::generate(7, 32, 32);
    assert!(grid.iter().all(|(_, _, v)| v < GROUND_VARIANTS));
}

#[test]
fn iter_visits_every_tile_in_row_major_order() {
    let grid = TerrainGrid::generate(7, 3, 2);
    let coords: Vec<(u32, u32)> = grid.iter().map(|(x, y, _)| (x, y)).collect();
    assert_eq!(coords, vec![(0, 0), (1, 0), (2, 0), (0, 1), (1, 1), (2, 1)]);
}

#[test]
fn variant_accessor_matches_iteration() {
    let grid = TerrainGrid::generate(11, 8, 8);
    assert!(grid.iter().all(|(x, y, v)| grid.variant(x, y) == v));
}

#[test]
fn a_grid_reports_the_dimensions_it_was_generated_with() {
    let grid = TerrainGrid::generate(7, 24, 16);
    assert_eq!(grid.width(), 24);
    assert_eq!(grid.height(), 16);
}
