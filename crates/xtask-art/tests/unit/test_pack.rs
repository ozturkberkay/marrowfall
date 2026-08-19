use image::RgbaImage;
use xtask_art::pack::*;

use image::Rgba;

/// Opaque rectangle on a transparent canvas.
fn stamped(w: u32, h: u32, rect: Rect) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let inside =
            x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height;
        if inside {
            Rgba([255, 0, 0, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    })
}

/// A frame whose content is horizontally centred on the canvas, as the
/// bake camera always produces.
fn frame(direction: &'static str, index: u32, rect: Rect) -> Frame {
    Frame {
        direction,
        index,
        image: stamped(64, 64, centred(64, rect)),
    }
}

/// Re-centres a rect on the canvas' rotation axis.
fn centred(canvas: u32, rect: Rect) -> Rect {
    Rect {
        x: canvas / 2 - rect.width / 2,
        ..rect
    }
}

#[test]
fn bounds_find_the_opaque_region() {
    let rect = Rect {
        x: 3,
        y: 5,
        width: 4,
        height: 6,
    };
    assert_eq!(content_bounds(&stamped(32, 32, rect), 1), Some(rect));
}

#[test]
fn fully_transparent_image_has_no_bounds() {
    assert_eq!(content_bounds(&RgbaImage::new(16, 16), 1), None);
}

#[test]
fn union_covers_both_rects() {
    let a = Rect {
        x: 2,
        y: 2,
        width: 2,
        height: 2,
    };
    let b = Rect {
        x: 8,
        y: 1,
        width: 2,
        height: 6,
    };
    assert_eq!(
        a.union(b),
        Rect {
            x: 2,
            y: 1,
            width: 8,
            height: 6
        }
    );
}

/// The bug this guards: scaling each animation to its own bounds makes the
/// character shrink when he switches to a taller animation.
#[test]
fn all_clips_of_a_character_share_one_scale() {
    // A short idle and a tall run, as a real character produces.
    let idle = vec![frame(
        "s",
        0,
        Rect {
            x: 24,
            y: 30,
            width: 16,
            height: 30,
        },
    )];
    let run = vec![frame(
        "s",
        0,
        Rect {
            x: 20,
            y: 10,
            width: 24,
            height: 50,
        },
    )];

    let scale = character_scale([idle.as_slice(), run.as_slice()], 100).unwrap();
    let (_, idle_layout) =
        pack_animation(&idle, &["s"], "idle.png".into(), 12, true, &scale).unwrap();
    let (_, run_layout) = pack_animation(&run, &["s"], "run.png".into(), 12, true, &scale).unwrap();

    // The idle is 30px of content and the run 50px, so at one shared
    // scale the run's cell must be proportionally taller, never equal,
    // which would mean one of them was squashed to fit.
    let ratio = f64::from(run_layout.cell_height) / f64::from(idle_layout.cell_height);
    assert!(
        (ratio - 50.0 / 30.0).abs() < 0.05,
        "animations must share a scale; got ratio {ratio}"
    );
}

/// `sprite_height` must mean the character's standing height, so two
/// characters authored at the same value read the same size in game
/// regardless of how far their animations reach.
#[test]
fn scale_is_keyed_to_the_first_clip_not_the_union() {
    let idle = vec![frame(
        "s",
        0,
        Rect {
            x: 0,
            y: 40,
            width: 16,
            height: 20,
        },
    )];
    // A leap reaching well above the idle silhouette.
    let leap = vec![frame(
        "s",
        0,
        Rect {
            x: 0,
            y: 5,
            width: 16,
            height: 55,
        },
    )];

    let alone = character_scale([idle.as_slice()], 100).unwrap();
    let with_leap = character_scale([idle.as_slice(), leap.as_slice()], 100).unwrap();

    let idle_alone = pack_animation(&idle, &["s"], "i.png".into(), 12, true, &alone)
        .unwrap()
        .1;
    let idle_with = pack_animation(&idle, &["s"], "i.png".into(), 12, true, &with_leap)
        .unwrap()
        .1;
    let leap_with = pack_animation(&leap, &["s"], "l.png".into(), 12, true, &with_leap)
        .unwrap()
        .1;

    assert_eq!(idle_alone.cell_height, 100, "idle stands 100px tall");
    assert_eq!(
        idle_with.cell_height, 100,
        "adding a leap must not shrink the idle"
    );
    assert!(
        leap_with.cell_height > 100,
        "the leap's own cell is taller, at the same scale"
    );
}

/// The other alignment bug: per-frame cropping makes the sprite jitter.
#[test]
fn every_frame_shares_one_crop_so_the_sprite_cannot_jitter() {
    // Two frames whose content ends on the same baseline but starts at
    // different heights, which is what a swinging limb looks like.
    let frames = vec![
        frame(
            "s",
            0,
            Rect {
                x: 20,
                y: 10,
                width: 8,
                height: 40,
            },
        ),
        frame(
            "s",
            1,
            Rect {
                x: 20,
                y: 14,
                width: 8,
                height: 36,
            },
        ),
    ];
    let scale_spec = character_scale([frames.as_slice()], 50).unwrap();
    let (_, layout) =
        pack_animation(&frames, &["s"], "c.png".into(), 12, true, &scale_spec).unwrap();

    assert_eq!(layout.frames, 2);
    // Trimming moves pixels in the atlas but never in the cell: both frames
    // still reach the same baseline, so the sprite cannot bob between frames.
    let baseline: Vec<u32> = layout.rects.iter().map(|r| r.off_y + r.h).collect();
    assert_eq!(
        baseline[0], baseline[1],
        "frames disagree about where the ground is: {baseline:?}"
    );
    assert_eq!(
        baseline[0], layout.cell_height,
        "the shared cell should end at the baseline"
    );
    // The taller frame is trimmed less than the shorter one.
    assert!(
        layout.rects[0].h > layout.rects[1].h,
        "trimming should track content, got {:?}",
        layout.rects
    );
}

/// A pose reaching the canvas edge means the render was cut off; packing
/// must refuse rather than ship a clipped character.
#[test]
fn clipped_frames_are_rejected() {
    let clipped = vec![Frame {
        direction: "s",
        index: 0,
        // Content running off the left edge: the render was cut off.
        image: stamped(
            64,
            64,
            Rect {
                x: 0,
                y: 10,
                width: 40,
                height: 40,
            },
        ),
    }];
    let error = character_scale([clipped.as_slice()], 100)
        .unwrap_err()
        .to_string();
    assert!(error.contains("clipped"), "got: {error}");
    assert!(
        error.contains("re-bake"),
        "error should say what to do: {error}"
    );
}

#[test]
fn atlas_has_one_row_per_direction() {
    let directions: &'static [&'static str] = &["s", "n"];
    let frames: Vec<Frame> = directions
        .iter()
        .flat_map(|d| {
            (0..3).map(move |i| {
                frame(
                    d,
                    i,
                    Rect {
                        x: 24,
                        y: 8,
                        width: 16,
                        height: 48,
                    },
                )
            })
        })
        .collect();

    let scale_spec = character_scale([frames.as_slice()], 32).unwrap();
    let (atlas, layout) =
        pack_animation(&frames, directions, "c.png".into(), 12, true, &scale_spec).unwrap();
    assert_eq!(layout.frames, 3);
    assert_eq!(layout.rects.len(), 6, "one rect per direction per frame");
    for rect in &layout.rects {
        assert!(
            rect.x + rect.w <= atlas.width() && rect.y + rect.h <= atlas.height(),
            "rect {rect:?} escapes the atlas"
        );
        assert!(
            rect.off_x + rect.w <= layout.cell_width && rect.off_y + rect.h <= layout.cell_height,
            "rect {rect:?} escapes its cell, so the anchor would not line up"
        );
    }
    // Block compressed textures are stored in 4x4 blocks. Godot pads to that
    // grid but keeps computing UVs from the unpadded size, so a misaligned
    // atlas samples progressively sideways.
    assert_eq!(atlas.width() % 4, 0, "atlas width must be block aligned");
    assert_eq!(atlas.height() % 4, 0, "atlas height must be block aligned");
}

#[test]
fn sprite_height_is_honoured_and_aspect_preserved() {
    let frames = vec![Frame {
        direction: "s",
        index: 0,
        image: stamped(
            256,
            256,
            centred(
                256,
                Rect {
                    x: 0,
                    y: 20,
                    width: 40,
                    height: 200,
                },
            ),
        ),
    }];
    let scale_spec = character_scale([frames.as_slice()], 160).unwrap();
    let (_, layout) =
        pack_animation(&frames, &["s"], "c.png".into(), 12, true, &scale_spec).unwrap();
    assert_eq!(
        layout.cell_height, 160,
        "a single animation's cell equals the sprite height"
    );
    assert_eq!(layout.cell_width, 32); // 40/200 * 160
}

/// Guards the anchor against a naive `cell_width / 2`, which happens to
/// agree whenever the crop is symmetric about the axis.
#[test]
fn anchor_follows_the_rotation_axis_not_the_crop_centre() {
    // An asymmetric silhouette straddling the axis: a limb swung out to
    // one side makes the crop centre drift away from the rotation axis.
    let frames = vec![Frame {
        direction: "s",
        index: 0,
        image: stamped(
            100,
            100,
            Rect {
                x: 40,
                y: 20,
                width: 40,
                height: 60,
            },
        ),
    }];
    let scale_spec = character_scale([frames.as_slice()], 60).unwrap();
    let (_, layout) =
        pack_animation(&frames, &["s"], "c.png".into(), 12, true, &scale_spec).unwrap();

    assert_ne!(
        layout.anchor.x,
        layout.cell_width / 2,
        "anchor must track the rotation axis, not the crop centre"
    );
    assert_eq!(
        layout.anchor.y,
        layout.cell_height - 1,
        "a grounded animation anchors on its bottom row"
    );
}

#[test]
fn mismatched_frame_counts_between_directions_are_rejected() {
    let frames = vec![
        frame(
            "s",
            0,
            Rect {
                x: 24,
                y: 8,
                width: 8,
                height: 40,
            },
        ),
        frame(
            "s",
            1,
            Rect {
                x: 24,
                y: 8,
                width: 8,
                height: 40,
            },
        ),
        frame(
            "n",
            0,
            Rect {
                x: 24,
                y: 8,
                width: 8,
                height: 40,
            },
        ),
    ];
    let scale_spec = character_scale([frames.as_slice()], 40).unwrap();
    let error =
        pack_animation(&frames, &["s", "n"], "c.png".into(), 12, true, &scale_spec).unwrap_err();
    assert!(error.to_string().contains("frames"));
}

#[test]
fn direction_names_match_the_bake_order() {
    assert_eq!(direction_names(8).unwrap()[0], "s");
    assert_eq!(direction_names(8).unwrap().len(), 8);
    assert_eq!(direction_names(4).unwrap().len(), 4);
    assert!(direction_names(6).is_err());
}

#[test]
fn empty_clip_is_rejected() {
    let frames = vec![frame(
        "s",
        0,
        Rect {
            x: 10,
            y: 10,
            width: 8,
            height: 8,
        },
    )];
    let scale_spec = character_scale([frames.as_slice()], 40).unwrap();
    assert!(pack_animation(&[], &["s"], "c.png".into(), 12, true, &scale_spec).is_err());
}

/// A full direction ring of frames whose content sits at an exact position on
/// the canvas, not re-centred, so alignment failures can be provoked.
fn frames_at(x: u32, y: u32, width: u32, height: u32) -> Vec<Frame> {
    direction_names(8)
        .unwrap()
        .iter()
        .map(|direction| Frame {
            direction,
            index: 0,
            image: stamped(
                64,
                64,
                Rect {
                    x,
                    y,
                    width,
                    height,
                },
            ),
        })
        .collect()
}

#[test]
fn an_animation_whose_content_misses_the_rotation_axis_is_rejected() {
    // The reference is centred on the axis; this one sits entirely to the
    // right of it, which means the bake camera was not centred on the
    // character and the sprite would not line up with its tile.
    let reference = frames_at(24, 20, 16, 30);
    let offset = frames_at(40, 20, 12, 30);
    let character = character_scale([reference.as_slice()], 160).unwrap();

    let error = pack_animation(
        &offset,
        direction_names(8).unwrap(),
        "run.png".to_owned(),
        12,
        true,
        &character,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("rotation axis"), "got: {error}");
}

#[test]
fn an_animation_starting_below_the_reference_ground_line_is_rejected() {
    // The reference's feet set the ground line. This animation's content
    // starts lower down the canvas than that line, which means the reference
    // was not standing on the ground and nothing can be anchored to it.
    let reference = frames_at(24, 20, 16, 20);
    let floating = frames_at(24, 50, 16, 10);
    let character = character_scale([reference.as_slice()], 160).unwrap();

    let error = pack_animation(
        &floating,
        direction_names(8).unwrap(),
        "jump.png".to_owned(),
        12,
        false,
        &character,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("ground line"), "got: {error}");
}
