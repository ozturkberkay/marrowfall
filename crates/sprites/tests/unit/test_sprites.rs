use sprites::{AnimationAtlas, Error, FrameRect};

/// The committed manifest the game loads. Embedded rather than read, so the
/// contract between the pipeline and the game is checked at build time.
const SURVIVOR: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../project/assets/characters/survivor/character.ron"
));

/// A two-direction, two-frame manifest. Small enough to break one field at a
/// time, which is how each invariant is pinned separately.
const VALID: &str = r#"CharacterAssets(
    name: "dummy",
    animations: {
        "idle": AnimationAtlas(
            file: "idle.png",
            directions: ["s", "e"],
            frames: 2,
            fps: 8,
            loops: true,
            cell_width: 10,
            cell_height: 20,
            anchor: Anchor(x: 5, y: 19),
            rects: [
                FrameRect(x: 0, y: 0, w: 4, h: 8, off_x: 1, off_y: 2),
                FrameRect(x: 6, y: 0, w: 4, h: 8, off_x: 1, off_y: 2),
                FrameRect(x: 0, y: 10, w: 4, h: 8, off_x: 1, off_y: 2),
                FrameRect(x: 6, y: 10, w: 4, h: 8, off_x: 1, off_y: 2),
            ],
        ),
    },
)"#;

fn only_atlas(text: &str) -> AnimationAtlas {
    let assets = sprites::parse(text).unwrap();
    assert_eq!(assets.animations.len(), 1);
    assets.animations.into_values().next().unwrap()
}

fn atlas_of(text: &str, animation: &str) -> AnimationAtlas {
    sprites::parse(text)
        .unwrap()
        .animations
        .remove(animation)
        .unwrap()
}

#[test]
fn a_manifest_round_trips_field_for_field() {
    let assets = sprites::parse(VALID).unwrap();
    assert_eq!(assets.name, "dummy");

    let atlas = only_atlas(VALID);
    assert_eq!(atlas.file, "idle.png");
    assert_eq!(atlas.directions, ["s", "e"]);
    assert_eq!((atlas.frames, atlas.fps, atlas.loops), (2, 8, true));
    assert_eq!((atlas.cell_width, atlas.cell_height), (10, 20));
    assert_eq!((atlas.anchor.x, atlas.anchor.y), (5, 19));
    assert_eq!(atlas.rects.len(), 4);
}

/// Both failures reach a person through `Display`, so both have to say
/// something usable.
#[test]
fn text_that_is_not_a_manifest_is_a_syntax_error() {
    let error = sprites::parse("not a manifest at all").unwrap_err();
    assert!(matches!(error, Error::Syntax(_)), "{error:?}");
    assert!(error.to_string().contains("manifest"), "{error}");
}

/// Each invariant on its own, because [`sprites::frame_at`] and friends are
/// only total once every one of them holds.
#[test]
fn each_invariant_is_rejected_on_its_own() {
    let cases = [
        ("frames: 2", "frames: 0", "frames"),
        ("fps: 8", "fps: 0", "fps"),
        (r#"["s", "e"]"#, "[]", "directions"),
        (
            "FrameRect(x: 6, y: 10, w: 4, h: 8, off_x: 1, off_y: 2),",
            "",
            "rects",
        ),
        ("y: 19", "y: 20", "anchor"),
    ];

    for (from, to, mentions) in cases {
        let broken = VALID.replace(from, to);
        assert_ne!(broken, VALID, "the {mentions} case changed nothing");

        match sprites::parse(&broken).unwrap_err() {
            Error::Invalid { animation, detail } => {
                assert_eq!(animation, "idle");
                assert!(
                    detail.contains(mentions),
                    "{mentions:?} not named by {detail:?}"
                );
            }
            other => panic!("{from:?} to {to:?} should be invalid, got {other:?}"),
        }
    }
}

#[test]
fn an_invalid_manifest_says_which_animation_is_wrong() {
    let error = sprites::parse(&VALID.replace("fps: 8", "fps: 0")).unwrap_err();
    assert!(error.to_string().contains("idle"), "{error}");
}

/// The contract test between the art pipeline and the game: these numbers come
/// out of `cargo art` and the frontend cannot ask for a clip that is not here.
#[test]
fn the_committed_survivor_manifest_is_valid() {
    let assets = sprites::parse(SURVIVOR).unwrap();
    assert_eq!(assets.name, "survivor");

    let expected = [
        ("idle", 15, 8, (94, 240), (47, 239)),
        ("run", 20, 24, (238, 260), (119, 241)),
        ("walk_back", 18, 20, (170, 254), (85, 236)),
    ];
    assert_eq!(assets.animations.len(), expected.len());

    for (name, frames, fps, cell, anchor) in expected {
        let atlas = assets.animations.get(name).unwrap();
        assert_eq!(atlas.file, format!("{name}.png"));
        assert_eq!((atlas.frames, atlas.fps), (frames, fps));
        assert_eq!((atlas.cell_width, atlas.cell_height), cell);
        assert_eq!((atlas.anchor.x, atlas.anchor.y), anchor);
        assert!(atlas.loops, "{name} should loop");
        assert_eq!(atlas.rects.len(), 8 * frames as usize);
    }
}

#[test]
fn every_compass_direction_has_a_row_in_the_survivors_atlas() {
    let atlas = atlas_of(SURVIVOR, "run");
    for (row, direction) in ["s", "se", "e", "ne", "n", "nw", "w", "sw"]
        .into_iter()
        .enumerate()
    {
        assert_eq!(sprites::row_for(&atlas, direction), Some(row));
    }
}

/// How a four-direction atlas answers "se": the caller leaves the sprite on
/// the frame it had rather than guessing a row.
#[test]
fn a_direction_the_atlas_lacks_has_no_row() {
    let atlas = only_atlas(VALID);
    assert_eq!(sprites::row_for(&atlas, "se"), None);
    assert_eq!(sprites::row_for(&atlas, ""), None);
}

#[test]
fn a_clip_starts_on_its_first_frame() {
    let atlas = only_atlas(VALID);
    assert_eq!(sprites::frame_at(&atlas, 0.0), 0);
    // The seed snapshot published before any tick has run stamps time 0, and
    // the frontend walks back one tick from whatever it is given.
    assert_eq!(sprites::frame_at(&atlas, -1.0), 0);
}

#[test]
fn a_looping_clip_wraps_at_its_length() {
    let atlas = only_atlas(VALID);
    // 2 frames at 8 fps: a quarter second is one full cycle.
    assert_eq!(sprites::frame_at(&atlas, 0.125), 1);
    assert_eq!(sprites::frame_at(&atlas, 0.25), 0);
    assert_eq!(sprites::frame_at(&atlas, 10.0), 0);
}

#[test]
fn a_clip_that_does_not_loop_holds_its_last_frame() {
    let atlas = only_atlas(&VALID.replace("loops: true", "loops: false"));
    assert_eq!(sprites::frame_at(&atlas, 0.125), 1);
    assert_eq!(sprites::frame_at(&atlas, 10.0), 1);
}

/// `frames: 0` cannot reach here through [`sprites::parse`], but the fields are
/// public, so the guard is what keeps the modulo and the subtraction safe.
#[test]
fn a_frameless_atlas_answers_zero_instead_of_panicking() {
    let mut atlas = only_atlas(VALID);
    atlas.frames = 0;
    atlas.rects.clear();

    assert_eq!(sprites::frame_at(&atlas, 0.0), 0);
    assert_eq!(sprites::frame_at(&atlas, 10.0), 0);
    assert!(sprites::frame(&atlas, 0, 0).is_none());
}

#[test]
fn a_frame_is_indexed_direction_major() {
    let atlas = only_atlas(VALID);
    let rect = |row, frame| sprites::frame(&atlas, row, frame).map(|r: &FrameRect| (r.x, r.y));

    assert_eq!(rect(0, 0), Some((0, 0)));
    assert_eq!(rect(0, 1), Some((6, 0)));
    assert_eq!(rect(1, 0), Some((0, 10)));
    assert_eq!(rect(1, 1), Some((6, 10)));
}

/// A row taken from one atlas can outrun another, which is what a clip change
/// does when the two have different direction counts.
#[test]
fn a_cell_this_atlas_lacks_has_no_frame() {
    let atlas = only_atlas(VALID);
    for (row, frame) in [(2, 0), (0, 2), (usize::MAX, usize::MAX)] {
        assert!(
            sprites::frame(&atlas, row, frame).is_none(),
            "row {row} frame {frame} is not in a 2x2 atlas"
        );
    }
}
