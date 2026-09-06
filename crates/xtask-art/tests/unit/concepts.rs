//! The negative concept views, each built from a committed one by moving
//! pixels.
//!
//! Every fixture starts from `art/characters/survivor/concept/*.png`, the
//! only concept art this repository has, and breaks one thing about it at
//! coordinates written down here. Nothing calls a measurement to decide what
//! to break, so no fixture can agree with the gate it is meant to fail.

use std::path::Path;

use image::{Rgb, RgbImage, imageops};

/// Where the arms of the committed front and back views sit, read off both
/// once by hand and written down: on each, the gap beside the ribcage runs
/// from x = 232 to 378 on the left and 624 to 795 on the right, over rows
/// 384 to 677 on the front and 384 to 668 on the back. So these two columns
/// cover every gap with a margin, and the rows cover both bands.
const ARM_GAPS: [(u32, u32); 2] = [(225, 385), (620, 805)];
const ARM_ROWS: (u32, u32) = (380, 680);

/// And where the neck sits on the same two, read off once by hand: the head
/// ends and the shoulders begin between these rows.
const NECK_ROWS: (u32, u32) = (250, 300);

/// One committed view, decoded, ready to be broken on purpose.
pub struct Concept {
    image: RgbImage,
}

impl Concept {
    /// One of the four committed views.
    pub fn view(name: &str) -> Self {
        let path = crate::support::repo_root()
            .join("art/characters/survivor/concept")
            .join(format!("{name}.png"));
        let image = image::open(&path)
            .unwrap_or_else(|error| panic!("reading {}: {error:#}", path.display()))
            .to_rgb8();
        Self { image }
    }

    /// A ramp across the width, `percent` of the full 0 to 255 range from one
    /// edge to the other. The prompt asks for a fill with no gradient in it.
    pub fn with_a_gradient(mut self, percent: f64) -> Self {
        let width = f64::from(self.image.width() - 1);
        for (x, _, pixel) in self.image.enumerate_pixels_mut() {
            let added = (f64::from(x) / width * percent / 100.0 * 255.0) as u8;
            pixel.0 = pixel.0.map(|level| level.saturating_add(added));
        }
        self
    }

    /// The same character twice, side by side, each at half size.
    pub fn beside_a_second_figure(self) -> Self {
        let (width, height) = (self.image.width(), self.image.height());
        let half = imageops::resize(
            &self.image,
            width / 2,
            height / 2,
            imageops::FilterType::Triangle,
        );
        let mut canvas = RgbImage::from_pixel(width, height, Rgb(self.backdrop()));
        for column in 0..2 {
            imageops::replace(
                &mut canvas,
                &half,
                i64::from(column * width / 2),
                i64::from(height / 4),
            );
        }
        Self { image: canvas }
    }

    /// The gaps between the arms and the ribcage filled in with skin, which
    /// is what a generator returns when it paints the arms onto the body.
    pub fn with_the_arms_on_the_ribcage(mut self) -> Self {
        let skin = *self.image.get_pixel(self.image.width() / 2, ARM_ROWS.0);
        for (left, right) in ARM_GAPS {
            for y in ARM_ROWS.0..=ARM_ROWS.1 {
                for x in left..=right {
                    self.image.put_pixel(x, y, skin);
                }
            }
        }
        self
    }

    /// The whole left half of the arm rows stretched outward from the middle
    /// of the frame, so the left arm reaches `scale` times as far from the
    /// axis. The right half is untouched, which is the asymmetry the mirror
    /// rule reads.
    pub fn with_the_left_half_stretched(mut self, scale: f64) -> Self {
        let axis = f64::from(self.image.width() / 2);
        let source = self.image.clone();
        for y in ARM_ROWS.0..=ARM_ROWS.1 {
            for x in 0..self.image.width() / 2 {
                let from = axis - (axis - f64::from(x)) / scale;
                self.image
                    .put_pixel(x, y, *source.get_pixel(from as u32, y));
            }
        }
        self
    }

    /// The whole view at `scale`, centered in a frame of the same size, so
    /// the figure it holds is that much shorter than the other three.
    pub fn scaled_by(self, scale: f64) -> Self {
        let (width, height) = (self.image.width(), self.image.height());
        let smaller = imageops::resize(
            &self.image,
            (f64::from(width) * scale) as u32,
            (f64::from(height) * scale) as u32,
            imageops::FilterType::Triangle,
        );
        let mut canvas = RgbImage::from_pixel(width, height, Rgb(self.backdrop()));
        imageops::replace(
            &mut canvas,
            &smaller,
            i64::from((width - smaller.width()) / 2),
            i64::from((height - smaller.height()) / 2),
        );
        Self { image: canvas }
    }

    /// A second, darker backdrop tone over the top-left fifth of the frame,
    /// which holds no figure. The border fill stops at the seam, so the patch
    /// is read as a second piece of silhouette.
    pub fn with_a_patch_on_the_wall(mut self) -> Self {
        let tone = Rgb(self.backdrop().map(|level| level.saturating_sub(60)));
        for y in 0..self.image.height() / 5 {
            for x in 0..self.image.width() / 5 {
                self.image.put_pixel(x, y, tone);
            }
        }
        self
    }

    /// The rows across the neck painted the backdrop's own color, which is a
    /// hood the color of the wall. It reaches the backdrop on both sides, so
    /// the fill eats it and the head is left as a piece of its own.
    pub fn with_the_neck_the_color_of_the_wall(mut self) -> Self {
        let tone = Rgb(self.backdrop());
        for y in NECK_ROWS.0..=NECK_ROWS.1 {
            for x in 0..self.image.width() {
                self.image.put_pixel(x, y, tone);
            }
        }
        self
    }

    /// A corner pixel, which is the fill the generator was asked for, so a
    /// canvas this view is pasted onto keeps the same background.
    fn backdrop(&self) -> [u8; 3] {
        self.image.get_pixel(2, 2).0
    }

    /// The PNG, encoded in memory. Nothing here is ever committed.
    pub fn to_png(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        self.image
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .expect("encoding a concept view");
        bytes
    }

    /// And the same bytes on disk, where the gate reads its views from.
    pub fn write(&self, path: &Path) {
        std::fs::create_dir_all(path.parent().expect("a view has a parent")).expect("mkdir");
        std::fs::write(path, self.to_png()).expect("writing a concept view");
    }
}
