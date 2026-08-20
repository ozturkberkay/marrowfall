# World generation

The shape of the world: what a tile is made of, how high it sits, and which
biome it belongs to. No engine, no I/O, no threads.

```sh
cargo world preview                 # look at the default seed
cargo nextest run -p worldgen --test unit
```

## Three rules

Everything the streaming world relies on comes from these, and breaking any one
of them breaks something far away.

1. **Position pure.** Every value is a function of the world seed and a
   coordinate. Nothing reads a neighbouring chunk, so generating one chunk can
   never trigger generating another, and chunks can be produced in any order.
   `tests/unit/test_chunk.rs` enforces it by generating a block of chunks in a
   shuffled order and comparing against sequential.
2. **No floats in the result.** Intermediate maths may use them; what gets
   stored is integral. A world whose stored form holds no float cannot drift by
   a rounding difference on someone else's machine.
3. **No non-deterministic calls.** No wall clock, no OS randomness, and none of
   the standard library's transcendental functions, whose precision Rust
   documents as varying by platform, by compiler version, and even between two
   calls in one run. `sqrt`, `floor`, `round` and the four arithmetic operators
   are exactly specified and safe.

## Domain tags are permanent

`derive(seed, domain, x, y)` is the only randomness. The domain tag is what
stops two systems that hash the same coordinate getting the same number: without
it, elevation and moisture would be perfectly correlated.

**The numbers in `Domain` are permanent.** Renumbering a variant changes every
world ever generated from every seed. Adding a new variant is free; reordering is
not.

A tag also takes a **variant**, which is a second input mixed in the same way, so
one tag can serve a family of independent fields. The site lattice uses it for the
class id. The variant is multiplied in rather than xor-ed into a coordinate, and
that is not a style choice: xor is a bijection, so `x ^ class` would make class
A at one cell bit-identical to class B at a shifted cell, and two classes would
place their sites in lockstep.

The same applies to each noise instance's seed, which is folded down to the
`i32` the noise library takes under its own tag. Two instances folding to the
same `i32` would produce identical fields, which is the same defect one layer
down.

## Layout

| File | Holds |
| --- | --- |
| `hash.rs` | The position hash and the domain tags |
| `tile.rs` | What one square of ground is |
| `rules.rs` | The tuning tables, and the one function that validates them |
| `world.rs` | Rules, seed and the noise built from the seed, in one value |
| `region.rs` | The region lattice, the tier rule and the biome choice |
| `height.rs` | The terraced height field |
| `site.rs` | The point of interest lattice, and the spacing rules |
| `chunk.rs` | Chunk coordinates, the stored grid, and `generate_chunk` |

## Ghost cells

A chunk carries one tile more than it owns on every side. The frontend needs a
tile's neighbours to pick its sprite, and at a chunk's edge those neighbours
belong to a chunk that may not be resident yet. Generating the ring here is free
because `tile_at` is pure, and it means one chunk's ghost cells and its neighbour's
interior always agree: both come from the same function rather than from each
other.

One ring is enough for every comparison including diagonals, because the ring's
corners are part of it.

## Things that look like details and are not

- **Region jitter is capped at half the pitch.** Past that, the nearest lattice
  point is not always inside the searched 3 by 3 block, and biomes stop being
  contiguous. `parse` refuses a larger value.
- **The home bubble is enforced on the region, not the tile.** A tile just inside
  the bubble can belong to a region whose point sits outside it, so the rule adds
  one region radius of slack. Checking the tile's own distance instead would
  split a region's tier down the middle and break the one-patch-one-tier
  property everything else assumes.
- **Height amplitude and period are not independent.** A terrace is about
  `period / (4 * amp)` tiles across, so raising amplitude alone turns plateaus
  into gravel. `project/data/README.md` has the numbers.
- **A site class's separation must stay below its spacing.** The placement rolls
  a position inside `spacing - separation`, so equality divides by zero and a
  larger separation would wrap into a huge modulus that quietly destroys the gap
  guarantee. `parse` refuses both.
