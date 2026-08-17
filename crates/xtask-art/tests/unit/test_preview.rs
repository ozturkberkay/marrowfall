use image::RgbaImage;
use xtask_art::preview::*;

use image::Rgba;

fn solid(w: u32, h: u32, colour: [u8; 4]) -> RgbaImage {
    RgbaImage::from_pixel(w, h, Rgba(colour))
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("marrowfall_preview_{tag}_{}", std::process::id()))
}

/// Guards against a grid that only gets the canvas size right: the images
/// must actually land in distinct cells.
#[test]
fn grid_places_each_image_in_its_own_cell() {
    let dir = temp_dir("grid");
    let dest = dir.join("grid.png");
    let red = solid(10, 20, [255, 0, 0, 255]);
    let blue = solid(10, 20, [0, 0, 255, 255]);

    write_grid(&[red, blue], 2, &dest).unwrap();

    let sheet = image::open(&dest).unwrap().to_rgba8();
    assert_eq!((sheet.width(), sheet.height()), (20, 20));
    assert_eq!(sheet.get_pixel(5, 10).0[..3], [255, 0, 0]);
    assert_eq!(sheet.get_pixel(15, 10).0[..3], [0, 0, 255]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn grid_wraps_onto_multiple_rows() {
    let dir = temp_dir("wrap");
    let dest = dir.join("grid.png");
    let images: Vec<RgbaImage> = (0..3).map(|_| solid(8, 8, [1, 2, 3, 255])).collect();

    write_grid(&images, 2, &dest).unwrap();

    let sheet = image::open(&dest).unwrap();
    assert_eq!(sheet.width(), 16);
    assert_eq!(sheet.height(), 16, "3 images at 2 columns needs 2 rows");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn transparent_areas_show_the_backdrop() {
    let dir = temp_dir("backdrop");
    let dest = dir.join("grid.png");
    write_grid(&[RgbaImage::new(4, 4)], 1, &dest).unwrap();

    let sheet = image::open(&dest).unwrap().to_rgba8();
    assert_eq!(sheet.get_pixel(2, 2).0, BACKDROP.0);
    let _ = std::fs::remove_dir_all(&dir);
}
