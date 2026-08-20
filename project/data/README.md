# Tuning tables

The numbers that shape the world. Tab separated, so they open in any
spreadsheet, and one row per line so a change is one line of diff.

Read by `crates/worldgen`, which never touches a path: the frontend hands it the
text through Godot's `FileAccess`, and `cargo world` through `std::fs`. They
live under `project/` because that is what `res://` maps to.

`#` starts a comment. **A row must not end in an empty column**: the
`trailing-whitespace` hook strips a trailing tab, which would silently drop a
column, so `worldgen::parse` refuses one instead.

## world.tsv

One row. World scale, in tiles, where one tile is one metre.

| Column | Meaning |
| --- | --- |
| `region_pitch_tiles` | How far apart region centres sit, so roughly how wide one biome patch is |
| `region_jitter_pct` | How far a centre may wander from its cell, as a percentage of half the pitch. Capped at 100 |
| `home_bubble_tiles` | Radius that is always tier 0. Enforced with half a pitch of slack, so the real safe radius is a little larger |

The pitch has to be several times smaller than a tier band, or the bands read as
serrated rings instead of a patchwork. At 700 against the bands below, a band is
three to six regions thick and a region takes about three minutes to cross.

## tiers.tsv

One row per difficulty band, contiguous from tier 0, ascending by `inner_tiles`.
The last band runs forever, so the frontier never runs out of world.

| Column | Meaning |
| --- | --- |
| `inner_tiles` | Distance from the origin where this band starts. Row 0 must be 0 |
| `harder_stray` | How many tiers above its distance a region here may sit |
| `easier_stray` | How many below. A safe pocket in hostile land, which costs nothing in fairness |
| `stray_pct` | Chance in 100 that a region here strays at all |

**The stray columns are the discovery dial**, and they decide whether the world
reads as rings or as a patchwork. At 0 the tier is purely distance, so the map is
a set of clean rings. The shipped values open it further out: tier 0 never
strays, so nothing can ambush a new character, and beyond that a region may be
one band harder or up to two bands easier.

Harder and easier are not symmetric on purpose. A harder pocket is a threat and
carries the fairness cost, so it stays rare; an easier pocket is a breather and
costs nothing, so it is generous. When both directions are open, the choice
between them is a fair coin.

Raising `stray_pct` much past 20 starts to undo the gradient: past roughly half
of all regions straying, the player learns that distance means nothing.

## materials.tsv

Ground materials. **Row order is id order**, so inserting a row renumbers every
material after it and changes what the frontend paints.

`blocks_walk`, `blocks_jump` and `blocks_shot` are independent, because the
answers differ: a knee-high ruin wall stops walking but not a jump or an arrow.
A material that blocks walking is a cliff face, not a floor, and `parse` refuses
a biome that uses one as its ground.

## biomes.tsv

One row per biome. Every tier needs at least one, and `parse` refuses a table
where one has none. Two or three per tier is what keeps a band from looking
uniform.

| Column | Meaning |
| --- | --- |
| `tier` | Which band offers this biome |
| `weight` | Relative chance among its tier's biomes. Must be positive |
| `ground` | A row in `materials.tsv` |
| `height_amp` | Peak relief in whole steps; the field runs `-amp` to `+amp` |
| `height_period` | Tiles across one rise or fall |

**Amplitude and period are not independent.** A terrace is about
`period / (4 * amp)` tiles across, so raising amplitude without raising period
turns plateaus into gravel. Keeping `period` near `80 * amp` holds terraces
around 20 tiles, which is walkable while still reading as relief. Change one,
change the other.

## site_classes.tsv

One row per class of point of interest. A class is a spacing rule: how often
sites of that kind appear, how far apart they are guaranteed to be, and where in
the world they are allowed at all. **Row order is id order.**

| Column | Meaning |
| --- | --- |
| `spacing` | Lattice pitch in tiles, so also the average gap between two sites of this class |
| `separation` | The gap the placement guarantees, whatever the roll. Must be below `spacing` |
| `fill_pct` | Chance in 100 that a lattice cell holds a site at all. 1 to 100 |
| `min_from_spawn` | No site of this class closer to the origin than this |
| `tier_lo`, `tier_hi` | The bands this class may appear in, inclusive |

`spacing` and `separation` are Minecraft's terms, from its structure sets,
because the rule is the same one. The pair works like this: a cell is
`spacing` tiles across, and its site may sit anywhere inside except a margin of
half the `separation` at each edge. Two sites in neighbouring cells are then at
least `separation + 1` apart. Lower the separation for a loose scatter, raise it
towards the spacing for something closer to a grid.

**A class needs at least one row in `sites.tsv`**, or its lattice would place
nothing, and `parse` refuses that. `parse` also refuses a table with no rows at
all, which is why the placeholder row below exists.

## sites.tsv

One row per kind of site. **Row order is id order.**

| Column | Meaning |
| --- | --- |
| `class` | A row in `site_classes.tsv`. The class decides where, this row decides what |
| `weight` | Relative chance among its class's kinds. Must be positive |
| `footprint` | Side length in tiles. Odd, so the site has a centre tile to sit on, and no wider than its class's `separation` so two neighbours cannot overlap |

Splitting the two tables is what makes the rules reusable: adding another kind
to a class is one row and changes no spacing, and making a whole class rarer is
one number and touches no kind.

**Both tables ship with a single `placeholder` row.** What places the world holds
is not decided yet, and a table needs at least one row while a class needs at
least one kind. The placeholder keeps the tables valid and the lattice visible in
`cargo world preview`, so the machinery stays exercised. Replace it when the real
places are chosen.

## Checking a change

```sh
cargo nextest run -p worldgen --test unit   # the tables are parsed at build time
cargo world preview                         # then look at it
```

A broken table fails the build, not the game: the tests include the shipped
files through `include_str!`.
