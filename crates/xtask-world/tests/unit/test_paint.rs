use worldgen::{IVec2, Tables, World, parse};

const WORLD_TSV: &str = "region_pitch_tiles\tregion_jitter_pct\thome_bubble_tiles\n256\t60\t128\n";
const TIERS_TSV: &str = "\
tier\tinner_tiles\tharder_stray\teasier_stray\tstray_pct
0\t0\t0\t0\t0
1\t400\t1\t0\t0
";
const MATERIALS_TSV: &str = "material\tblocks_walk\tblocks_jump\tblocks_shot\nsoil\t0\t0\t0\n";
const BIOMES_TSV: &str = "\
biome\ttier\tweight\tground\theight_amp\theight_period
low\t0\t10\tsoil\t3\t240
mid\t1\t10\tsoil\t5\t400
";

fn world() -> World {
    World::new(
        parse(Tables {
            world: WORLD_TSV,
            tiers: TIERS_TSV,
            materials: MATERIALS_TSV,
            biomes: BIOMES_TSV,
            // A dense, always-filled class, so a marker test does not depend on a
            // fill roll landing.
            site_classes: "class\tspacing\tseparation\tfill_pct\tmin_from_spawn\ttier_lo\ttier_hi\ncamp\t120\t60\t100\t0\t0\t1\n",
            sites: "site\tclass\tweight\tfootprint\ncampfire\tcamp\t1\t3\n",
        })
        .unwrap(),
        7,
    )
}

fn shot(radius: i32, step: i32) -> xtask_world::Shot {
    xtask_world::Shot {
        centre: IVec2::ZERO,
        radius,
        step,
        sites: false,
    }
}

#[test]
fn the_image_is_square_and_sized_by_radius_over_step() {
    assert_eq!(shot(100, 1).side(), 200);
    assert_eq!(shot(100, 4).side(), 50);
    assert_eq!(shot(1000, 20).side(), 100);
}

#[test]
fn a_step_larger_than_the_radius_still_makes_an_image() {
    // Better a one pixel picture than a panic inside the image crate.
    assert_eq!(shot(1, 1000).side(), 1);
}

#[test]
fn the_rendered_image_matches_the_requested_size() {
    let image = xtask_world::render(&world(), shot(64, 2));
    assert_eq!(image.width(), 64);
    assert_eq!(image.height(), 64);
}

#[test]
fn rendering_twice_gives_the_same_pixels() {
    let (w, s) = (world(), shot(48, 3));
    assert_eq!(
        xtask_world::render(&w, s).into_raw(),
        xtask_world::render(&w, s).into_raw()
    );
}

#[test]
fn the_picture_is_not_one_flat_colour() {
    // The whole point is seeing structure. A single colour would mean the tier or
    // the height never reached the pixels.
    let image = xtask_world::render(&world(), shot(600, 4));
    let first = image.as_raw()[..3].to_vec();
    assert!(
        image.as_raw().chunks(3).any(|px| px != first.as_slice()),
        "every pixel was the same colour"
    );
}

#[test]
fn the_outer_tiers_are_visibly_different_from_the_centre() {
    // The success criterion in prose: tier grows outward, and you can see it.
    let w = world();
    let centre = xtask_world::render(&w, shot(64, 1));
    let far = xtask_world::render(
        &w,
        xtask_world::Shot {
            centre: IVec2::new(3000, 3000),
            radius: 64,
            step: 1,
            sites: false,
        },
    );
    assert_ne!(
        centre.into_raw(),
        far.into_raw(),
        "the frontier looks identical to the home bubble"
    );
}

#[test]
fn markers_change_the_image_and_can_be_turned_off() {
    let w = world();
    let bare = xtask_world::render(&w, shot(200, 1));
    let marked = xtask_world::render(
        &w,
        xtask_world::Shot {
            sites: true,
            ..shot(200, 1)
        },
    );
    assert_ne!(
        bare.into_raw(),
        marked.into_raw(),
        "asking for site markers drew nothing"
    );
}

#[test]
fn a_marker_lands_on_its_own_site() {
    // The marker is only useful if it sits where the site is, so this checks the
    // pixel under one known site rather than just that something changed.
    let w = world();
    let s = xtask_world::Shot {
        sites: true,
        ..shot(600, 1)
    };
    let sites = worldgen::sites_near(&w, s.centre, s.radius);
    let site = sites
        .first()
        .expect("the fixture places sites near the origin");
    let image = xtask_world::render(&w, s);
    let bare = xtask_world::render(&w, shot(600, 1));
    let px = (site.at.x - s.centre.x + s.radius) as u32;
    let py = (site.at.y - s.centre.y + s.radius) as u32;
    assert_ne!(
        image.get_pixel(px, py),
        bare.get_pixel(px, py),
        "no marker at {:?}",
        site.at
    );
}

#[test]
fn a_marker_at_the_edge_does_not_run_off_the_image() {
    // A cross straddling the border would panic in `put_pixel` without the bounds
    // check, so this covers the clipping rather than the drawing.
    let w = world();
    for radius in [40, 41, 199, 601] {
        let _ = xtask_world::render(
            &w,
            xtask_world::Shot {
                sites: true,
                ..shot(radius, 1)
            },
        );
    }
}
