# World preview

Look at a seed without launching the game.

```sh
cargo world preview                                    # the default seed, 40 km across
cargo world preview --radius 1200 --step 1             # one pixel per tile, close up
cargo world preview --seed 0x1234 --centre -8000 3000  # somewhere else
cargo world preview --data project/data --out /tmp/a.png
cargo world preview --sites false                      # terrain only, no markers
```

Iterating on a generator by playing it costs a rebuild, a launch and a walk.
This turns that into a rerun and a picture, which is the difference between
tuning terrain and hoping. The Minecraft seed-inspection tools exist for the
same reason.

## Reading the image

One pixel is one tile (or `--step` tiles), seen from straight above. It is a map,
not a game view: an isometric projection would only make the question harder to
answer.

- **Hue** is the difficulty tier, green near the origin through to bone white at
  the frontier. The gradient outward is what you are checking.
- **A shade shift within one hue** is a different biome in the same tier.
- **Mottling** is the terraced height. At a coarse `--step` one pixel spans a
  whole terrace, so it reads as speckle; drop to `--step 1` to see plateaus.
- **A bright cross** is a point of interest, one colour per site class in the
  order `site_classes.tsv` lists them. A class should look evenly scattered at
  its own density and never clumped; clumping would mean the separation is not
  being honoured.

Boundaries should be organic curves. Straight edges would mean the boundary warp
has stopped working.

## Layout

`paint.rs` is pure and tested: a world and a `Shot` in, an image out. `cli.rs`
holds the argument parsing and the file reading, and `main.rs` only calls it, so
the tests under `tests/` can reach everything that has logic in it.
