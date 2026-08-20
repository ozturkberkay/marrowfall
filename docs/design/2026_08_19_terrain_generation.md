# Design: Terrain Generation

## Context & Problem

Marrowfall has no world. `crates/game/src/terrain.rs` paints a fixed 24 by 24
field of three ground variants so the survivor has something to stand on, and
`Sim::keep_player_on_the_field` clamps him inside it. `docs/concept.md` asks for
a world the player explores for tens of hours, where danger grows with distance
and every playthrough is a fresh discovery. Nothing in the current code can grow
into that: the field is a fixed array painted once at startup, positions are
`f32` which coarsens past roughly 20 km, and there is no notion of a biome, a
height, or a chunk. Until terrain exists, none of the systems that depend on it
have anywhere to attach.

## In Scope

- An effectively infinite world that is a pure function of a seed, streamed in
  chunks as the player walks, with no hitch on the simulation thread.
- Biome regions whose difficulty tier grows with distance from the world
  origin.
- One integer height per tile, so the world has plateaus, cliffs, drops and
  pits, and movement that respects them.
- Spreadsheet-editable spacing rules that decide where points of interest go.
- Spreadsheet-editable tables that drive the generator's tuning numbers.
- An offline tool that renders a region of a seed to a PNG.
- Position precision that survives the whole world, not only the first 20 km.

## Out of Scope

- **What a site puts on the ground:** the rules decide where a camp is and
  which kind it is. Stamping its footprint needs authored content, which is the
  terrain art design below, so a site is a position and a kind here and nothing
  is drawn in game.
- **Terrain art:** the existing pipeline is character shaped end to end, with a
  rig, an animation set and a direction ring. A ground tile has none of those, so
  a terrain asset path is its own design, and it is where the legibility
  requirement below is properly met. This one ships the existing three variant
  atlas as placeholder ground, with hard biome edges and no cliff sprites.
- **Instanced underground areas:** a cave mouth is a doorway into its own bounded
  map. Settled as a direction, unbuilt.
- **Rivers, roads and erosion:** these need a multi-pass generator, deferred.
- **Water:** a marsh is low, flat and muddy here. Swimming and shores are a
  feature, not a column.
- **Ground editing:** players will place objects but never raise, lower or
  flatten terrain.
- **Save and load:** terrain is never stored, only regenerated from the seed.
  That is why determinism is a hard requirement rather than a nice property, and
  it is the only part of persistence this design needs.

## Terminology

- **Chunk:** the 32 by 32 tile square that generation, streaming and painting all
  work in. At the project's 2560 by 1440 viewport a screen shows 400 tiles, so one
  chunk is about 2.56 screens of ground.
- **Ghost cells:** one extra ring of tiles around a chunk, copied from the
  neighbouring ground so the frontend can compare an edge tile against neighbours
  that may not be resident. The standard term, from the same pattern in domain
  decomposed simulation.
- **Region:** an irregular patch of world, about 1.5 km across, holding exactly
  one biome. Regions are what make direction meaningful.
- **Tier:** a difficulty band, 0 near the origin through 5 at the frontier. A
  region takes its tier from its own distance, not the player's. **Stray** is how
  far that tier may differ from the distance tier.
- **Terrace:** a flat plateau at one integer height, meeting its neighbours at
  vertical cliff faces rather than sloped ground.
- **Site:** one placed point of interest, and **class** is the group whose
  spacing rule placed it. **Spacing** is the lattice pitch, so the average gap;
  **separation** is the gap the placement guarantees. Minecraft's terms.
- **Position pure:** a value that is a function of the world seed and a coordinate
  only, reading no neighbour and no shared state.
- **Floating origin:** drawing everything relative to a point near the player, so
  screen coordinates stay small however far the player has walked.

## Key Decisions

### How is a chunk generated?

Generating chunk (i, j) must never require another chunk to exist. If it does,
generation cascades: chunk A reads B, which generates C, and one player step
triggers thousands of chunks.

#### ✅ Option 1: Position pure, with extent added later by a bounded lattice

Every tile is a function of position alone. Nothing reads a neighbouring chunk;
anything needing a neighbour's value calls the same function for it.

```rust
pub fn tile_at(world: &World, tile: IVec2) -> Tile;   // reads no chunk, ever
```

**Pros:** cascading generation is structurally impossible, not merely avoided,
and order independence makes parallel or out of order generation safe for free.
The ghost cells cost one extra call per border tile rather than a dependency on
a neighbour. And extent comes from a second position pure lattice layered on
top, which is exactly how the site lattice below works, so it needs no change
here.

**Cons:** no feature can follow a long path, so no rivers and no roads.

**Rationale:** it covers what the world needs now, and everything given up is
additive later, because neither a lattice nor a pass changes `tile_at`.

#### ❌ Option 2: A staged pipeline with a neighbour margin

Minecraft's model. Each stage may read the previous stage's output for itself
and its eight neighbours, which is what lets a feature cross a border.

```rust
// A chunk may enter stage N only once all eight neighbours reached N-1.
fn advance(chunk: ChunkCoord, to: Stage) { for n in chunk.ring() { ensure(n, to.prev()) } }
```

**Pros:** proven at enormous scale, and expresses rivers, roads and erosion.
**Cons:** the eight neighbour rule is real bookkeeping, and cascading generation
is a notorious bug class in exactly this design.

**Rationale:** Rejected. It adds only capability the world does not ask for yet,
while adding the one failure mode that freezes a game. Layered generation, where
each layer names which earlier layer it may read, is rejected for the same reason
at a higher cost: more machinery, and a Rust port that is single threaded with
negligible adoption.

### How does the world decide which biome a tile is in?

The world must be safest at the origin and grow more dangerous outward, and
biomes must read as progression.

#### ✅ Option 1: Regions, with the tier taken from the region

A jittered lattice of points about 1.5 km apart. Every tile belongs to its
nearest point, so the world is a patchwork of irregular regions, each holding
one biome. The region's own distance from the origin picks its tier.

```rust
let point = nearest_region_point(world, tile);   // jitter bounded to half the pitch
let tier  = tier_of(world, point);               // from the POINT's distance
let biome = pick_weighted(world.rules.biomes_in(tier), derive(.., point));
```

**Pros:** progression is guaranteed, because tier comes from distance. Each
biome is one contiguous patch of controllable size, so fragmented biome soup
cannot happen. Directions differ, so a hand drawn map carries real knowledge and
the Cartography skill in `docs/concept.md` has something to be about.

**Cons:** one more concept than plain rings, and the stray dial needs
playtesting.

**Rationale:** it strictly contains the ring model, since with stray at zero
tier is purely distance driven, and regions stay contiguous where noise blending
would fragment. **Jitter must be bounded to half the pitch**, or the nearest
point is not always in the surrounding 3 by 3 block and the partition stops
being a Voronoi diagram, which is what the contiguity claim rests on.

#### ❌ Option 2: Distance rings wobbled by noise

```rust
let tier = tier_for(dist(tile) + wobble(tile));  // no region, no identity
```

**Pros:** one rule the player learns in an hour, and permadeath fairness is
strongest. **Cons:** every direction is equivalent, so a map tells the player
nothing, and every seed has the same shape.

**Rationale:** Rejected as the primary model, retained as option 1's zero stray
setting.

#### ❌ Option 3: A multi noise parameter space

```rust
let biome = nearest_in_climate_space(temperature(t), humidity(t), erosion(t), ..);
```

**Pros:** believable geography. **Cons:** no concept of distance from the origin,
which is why Minecraft has no difficulty gradient, and adding distance as a sixth
axis makes the space unpredictable without a visualiser tool.

**Rationale:** Rejected. It fights the core requirement instead of serving it.

### How far may a region's tier stray, and what protects the spawn?

Familiarity near the origin, surprise further out, so the dial is a curve. The two
directions do different jobs: harder than its distance is a threat and carries the
fairness cost, easier is a safe pocket and carries none. Frequency matters more
than magnitude, because at half of all regions straying the player learns that
distance means nothing.

| Distance | Tier | Harder | Easier | Frequency |
| --- | --- | --- | --- | --- |
| 0 to 2 km | 0 | never | none | 0 percent |
| 2 to 5 km | 1 | plus 1 | none | 8 percent |
| 5 to 8 km | 2 | plus 1 | minus 1 | 15 percent |
| 8 to 11 km | 3 | plus 1 | minus 1 | 15 percent |
| 11 to 15 km | 4 | plus 1 | minus 2 | 20 percent |
| 15 km and out | 5 | plus 1 | minus 2 | 20 percent |

**The home bubble is enforced on the region, not the tile.** A per tile radius
cannot be guaranteed by a per region rule when the radius is smaller than the
region pitch: a tile 900 m out can belong to a region whose point sits at 1.6
km and inherit that point's tier. So the rule is

```rust
// Bubble plus one region radius, or the guarantee leaks at the bubble's edge.
if point.distance_tiles() <= world.rules.home_bubble + world.rules.region_pitch / 2 {
    return Tier(0);   // no stray, whatever the dial says
}
```

**Rationale:** a region rule, so the "one patch, one tier" property survives.
Taking the minimum of the tile and region distances would split a region's tier
at the bubble boundary.

**Requirement that falls out of this decision.** A surprise is unfair only when
it is invisible, so **biome boundaries must be unmistakable**, which is colour,
light and ambient sound work in the art design. Until that lands the dial is met
the cheap way, by the shape of the table itself: tier 0 never strays, so a new
character cannot be ambushed, and the easier direction is the generous one, since
a breather costs nothing in fairness. Setting every `stray_pct` to 0 collapses
this to the pure ring model, which is the fallback if playtesting says the
frequencies above are too high.

### Where do points of interest go?

Discovery beats authored places here, so a site's position is a roll and the
authored part is the rule constraining it. Two numbers per class do that, and
they are **Minecraft's `spacing` and `separation`**, from its structure sets,
because the problem is the same one: scatter something at a readable density
without ever letting two land on top of each other.

A class owns a lattice of `spacing` tile cells. Whether a cell holds a site is a
`fill_pct` roll on the cell's hash, and where inside the cell is a second roll
constrained to leave a margin of half the `separation` at each edge.

```rust
let margin = row.separation / 2;
let free = row.spacing - row.separation;    // parse refuses free <= 0
let at = cell * row.spacing + roll_in(free) + IVec2::splat(margin);
```

Two sites in neighbouring cells are then at least `separation + 1` apart. That is
a **guarantee, not an average**, which is what makes the dial usable: a low
separation reads as a loose scatter, one near the spacing as a grid.

**Rationale:** the lattice is what keeps the placement position pure. A cell is
answered from its own coordinates alone, so a chunk finds every site reaching it
by walking a fixed window of cells, in any order and on any thread, with no
rejection sampling and no global list. Two tables and not one, because
`site_classes.tsv` holding the rule and `sites.tsv` holding the kinds means
adding a third kind of ruin changes no spacing.

**Rejected: a Poisson disc scatter.** The better looking distribution, and the
one Horizon Zero Dawn uses for vegetation, but it is iterative: a candidate is
kept or dropped by comparing it against the points already accepted, which is
exactly the cascading dependency this design forbids everywhere else.

### Who resolves which sprite a tile draws?

Neighbours must be compared: a cliff face exists only relative to a lower tile,
and a biome edge only relative to a different material.

Godot's own autotiling is unusable: `set_cells_terrain_connect` is not a bulk
call, the fast path needs resolved atlas coordinates, and Godot terrains have no
fallback tile, so a missing combination silently draws the wrong sprite.

#### ✅ Option 1: Publish semantics with a one tile ring of ghost cells

`worldgen` produces a 34 by 34 slab. `render` walks the neighbours and picks
every sprite.

```rust
/// The chunk plus one ring of ghost cells, row major over 34 by 34. The ghost
/// cells come from the same pure function as the interior, so they always agree
/// with the neighbour they describe.
pub struct ChunkView { pub coord: ChunkCoord, pub tiles: [Tile; GRID_AREA] }
```

**Pros:** every art fact stays in `render`, so a new tileset never edits game
logic. `worldgen` fills the ring for free because `tile_at` is pure, where
`render` could not since the neighbour chunk may not be resident, at a cost of
12.9 percent more data per chunk. The slab's corners supply the diagonals, so
inner corner cliff pieces resolve from one ring. `render` already has this
shape: `draw.rs` and `iso.rs` are pure and unit tested.

**Cons:** an array whose middle is the real payload invites off by one mistakes.

**Rationale:** ghost cells are a standard pattern under a standard name, since
Minecraft's pipeline requires a neighbour margin, domain decomposed simulations
exchange halos, image filters pad borders, and this repository already writes a
two pixel atlas gutter for the same reason. Renderer resolved sprites is the
dominant practice too: Godot TileMap terrains, Unity rule tiles as art assets,
Tiled terrain sets, and Minecraft resolving connected textures at mesh build
time.

#### ❌ Option 2: Publish a neighbour difference bitmask

```rust
pub struct Tile { material: MaterialId, height: i8, edges: EdgeMask }
```

**Pros:** no ghost cells, and the comparing happens where the world lives.
**Cons:** the byte is a guess about future art. An inner corner cliff needs
diagonals, which a four side byte cannot express, so a purely artistic change
edits both crates and the shared format.

**Rationale:** Rejected as unnecessary, kept as the fallback if the ghost cells
prove awkward.

#### ❌ Option 3: Publish resolved atlas coordinates

```rust
pub struct Tile { atlas: Vector2i }   // art facts inside the no-pixels crate
```

**Pros:** the frontend becomes a loop over bytes. **Cons:** puts atlas layout
and variant counts inside the crate whose stated purpose is having no pixels.

**Rationale:** Rejected. `terrain.rs` already leaks a little of this, and this
would make it structural.

### How do chunks reach their two consumers, and where does generation run?

Two consumers need every chunk: `game` for collision, and `render` for painting.
Each must arrive exactly once, and the latest wins buffers in `README.md` are
allowed to drop values. `Arc` below is not part of the choice: **every option
needs it**, because `Box<ChunkView>` is single ownership and could reach only one
consumer, so the alternative is generating every chunk twice.

#### ✅ Option 1: A worker pool in `host`, delivering by `Arc`

```rust
pub enum ChunkMessage { Ready(Arc<ChunkView>), Dropped(ChunkCoord) }

// Workers take requests and hand finished chunks back. Nothing is shared but an
// `Arc<World>`, which is immutable, so there is no lock on the hot path.
for coord in residency.entered(centre) {
    requests.send(Request { coord, generation })?;
}
// Drained before each tick. `generation` is what makes a chunk that finished
// after its own eviction discardable rather than paintable.
for done in completions.try_iter() {
    if !residency.still_wants(done.coord, done.generation) { continue }
    sim.insert_chunk(Arc::clone(&done.view));        // collision
    let _ = chunks.send(ChunkMessage::Ready(done.view)); // painting
}
```

**Pros:**

- **It is what the industry does, and for a reason that applies here.** Minecraft
  and essentially every production open-world engine generate on a pool. The
  reason is not throughput today, it is that per chunk cost grows with content:
  a hash plus noise now, structures and cave carving later. A radius 3 residency
  is 49 chunks and about 24 ms today, so ten times the content freezes the
  simulation for a quarter second on world entry.
- **It is safe here for a specific architectural reason.** Generation is position
  pure, so completion order cannot change the result. That is the hard part of
  parallel generation, and the ordering test already guarantees it.
- Throughput scales with cores, so a sprinting player cannot outrun the
  generator however fast movement gets later.
- Residency is computed on the simulation thread from the player position it
  already owns, so passability cannot disagree with what is painted.

**Cons:** a pool, two queues, and a generation counter for in-flight
cancellation. The coverage gate also needs one line: `cargo_coverage.sh` runs
`--lib --test unit`, so pool code exercised only by the integration tier would
count as uncovered against the 96 percent gate.

**Rationale:**

- A reliable channel is the rule `README.md` already states, and the canonical
  producer and consumer shape: Minecraft ships chunks this way even in single
  player.
- **In-flight cancellation is not optional.** A worker can finish a chunk after
  `host` dropped it, so completions carry a generation counter and are filtered
  against current residency before forwarding. Without it, `render` receives
  `Dropped` then `Ready` for the same coordinate and paints outside residency,
  and a coordinate that leaves and re-enters gets two `Ready` messages whose
  Godot node names collide.

#### ❌ Option 2: Generate on the simulation thread under a per tick budget

```rust
for coord in residency.entered(centre).take(MAX_CHUNKS_PER_TICK) {
    let view = Arc::new(worldgen::generate_chunk(&world, coord));
    sim.insert_chunk(Arc::clone(&view));
    let _ = chunks.send(ChunkMessage::Ready(view));
}
```

**Pros:** almost no machinery and no cancellation to get right, and measured
demand is under 1 ms per second of play. **Cons:** it answers "is there a
throughput problem now" rather than "where should generation live", spreads the
cold start burst rather than removing it, and tightens with every increment of
per chunk content.

**Rationale:** Rejected. Sufficient today is not the same as right, and position
purity means the pool needs no new correctness argument, so now is the cheap
moment to build it.

#### ❌ Option 3: Chunks in the snapshot

```rust
pub struct RenderSnapshot { /* .. */ pub chunks: Vec<ChunkView> }
```

**Pros:** no new transport. **Cons:** the snapshot is latest wins and cloned
every frame, so chunks would be re-cloned at frame rate and silently dropped.

**Rationale:** Rejected. Buffers carry state, channels carry events, and a chunk
arriving is an event.

### What type holds a world position?

`f32` carries 24 bits, so the representable step grows with magnitude: 0.98 mm at
8 km, 15.6 mm at 131 km. Simulation positions survive, but rendering does not.
`bridge.rs` places sprites and the camera at absolute screen positions, which grow
107.3 px per tile, so by about **19.5 km** they exceed 2 million pixels and tiles
snap to a coarsening grid. The tier table puts the frontier at 15 km with no outer
edge, so players cross that line. A floating origin in `render` is required
regardless, and is the standard fix, used by Kerbal Space Program, Star Citizen
and Unity's own guidance.

#### ✅ Option 1: `f64` positions behind a `WorldVec` alias, added beside `Vec2`

```rust
/// A position or displacement in the world. f64, because the world outlives f32.
pub type WorldVec = glam::DVec2;
/// Kept: a direction, always near unit length, and the type `Input` takes.
pub use glam::Vec2;
```

**Pros:** it removes the precision question permanently, the way Minecraft's
`f64` entity positions and Unreal 5 do. Now is also the cheapest moment, since
movement, facing and the snapshot are the only systems touching a position. The
cost is 32 bytes per entity instead of 16, scalar `f64` runs at effectively the
same speed, and glam documents bit for bit identical results for both widths.

**Cons:** it ripples further than it looks. `PLAYER_SPEED`, `TICK_DT_F32` (which
becomes dead, a genuine simplification), `Facing::from_direction` and its
`SECTOR_EDGE` constant, `locomotion_of`, the finiteness asserts, and roughly 73
`Vec2` and `f32` uses across `test_sim.rs` all change. Widening `SECTOR_EDGE`
changes its value, so a test pinned to a sector boundary is rewritten.

**Rationale:**

- **`Vec2` must stay exported.** `render/src/iso.rs` says why: "`Vec2` is
  `game`'s re-export, so this crate never picks its own `glam`." Removing it
  forces `render` to add its own `glam`, which that comment exists to prevent.
- **`alpha` becomes `f64` end to end.** `host::alpha_for` already computes in
  `f64` and throws it away with `as f32`, and `bridge.rs` widens it straight
  back. Making it `f64` deletes both casts.
- What is genuinely hard to change later is not the code, which is a compiler
  guided refactor, but replays: changing float width changes every arithmetic
  result. That costs nothing today and accrues.

#### ❌ Option 2: Keep `f32` and rely on the floating origin alone

```rust
pub use glam::Vec2;   // unchanged
```

**Pros:** no change to existing code, and rendering is fixed regardless. **Cons:**
leaves a distance at which sub tile movement coarsens, in a game whose combat is
built on exact positioning.

**Rationale:** Rejected on timing, not capability.

### What format drives the tuning numbers?

#### ✅ Option 1: Tab separated tables, related by keys

```text
# project/data/biomes.tsv, displayed aligned here, single tabs in the file
biome           tier weight ground      height_amp height_period
ashen_lowland   0    10     dead_grass  3          140
blackweald      1    10     dark_soil   4          120
```

**Pros:** editable in any spreadsheet, one row per line makes diffs exact, and
Godot's resource pack override lets a mod shadow a shipped table cheaply.
Nesting dissolves by normalising into related tables, which is how `Levels.txt`
references `LvlMaze.txt` in Diablo 2, and why that modding scene is still alive.

**Cons:** a typo in a key is a load error rather than a compile error, so
validation at the boundary must be thorough.

**Rationale:** validation follows the pattern `crates/sprites` sets, where
`parse` is the only way in and checks every invariant so later readers have no
panic path, and a test over the shipped tables makes a broken table fail the
build. In Rust the code cost is identical to JSON, since serde handles both, so
the whole difference is editing and diffing, and JSON has no comments by
specification.

#### ❌ Option 2: RON, already a dependency

```ron
biomes: [(name: "ashen_lowland", tier: 0, weight: 10, ground: "dead_grass")]
```

**Pros:** nesting is natural, comments work, it maps to Rust types, and it needs
no new dependency. **Cons:** not a spreadsheet, so sorting, filtering and bulk
editing are gone, which is the workflow this data is for.

**Rationale:** Rejected for the tables, and it stays the right answer for the
first genuinely nested data, which is authored prefab layouts.

### Where does the code live, and how is the generator iterated on?

#### ✅ Option 1: `crates/worldgen`, plus a `crates/xtask-world` preview command

```rust
// crates/xtask-world: worldgen, clap and image. Nothing else.
fn main() { render_region(seed, centre, radius).save("target/preview.png") }
```

**Pros:** hash, noise and lattice code does not belong inside the ECS crate, and
`render` needs the tile types without needing the ECS. A separate xtask also keeps
the preview out of `xtask-art`, whose manifest would drag `reqwest`, `tokio` and
`wiremock` into a tool that draws a PNG. And it turns iteration from tune,
rebuild, launch Godot, walk 3 km into tune, rerun, look at a picture: the
Minecraft seed inspection ecosystem exists for this reason, and `image` is already
a workspace dependency.

**Cons:** two new crates and a second `.cargo/config.toml` alias.

**Rationale:** Accepted. The reason for the crate split is module hygiene plus
keeping the tool's dependency tree small, not the `crates/sprites` precedent,
which exists because `xtask-art` must not link into the shipped library.

#### ❌ Option 2: A module in `crates/game`, iterated on in the running game

```rust
mod worldgen;   // inside game, so every consumer takes the ECS
```

**Pros:** no new crate. **Cons:** the preview tool would pull in `hecs`, and
every tuning change would cost a rebuild and a walk to observe.

**Rationale:** Rejected.

## Architecture Overview

```text
project/data/*.tsv            spreadsheet editable tuning tables
      |
      | FileAccess in render, std::fs in the preview tool
      v
crates/worldgen               pure: no engine, no I/O, no threads
  World { rules, seed, noise } one seeded handle, so the seed cannot disagree
  WorldRules::parse(Tables)   validates every key, range and cross reference
  tile_at(&World, IVec2)      position pure: height, material, flags
  generate_chunk(&World, ..)  a 34 by 34 ChunkView, ghost cells included
      |
      +-----------------+-------------------------+
      v                 v                         v
crates/game       crates/host               crates/xtask-world
  resident chunks   residency set              region to PNG, sites marked
  terrace movement  worker pool
                    Arc to both consumers
                          |
                          v
                    crates/render
                      floating origin
                      sprite resolution from the ghost cells
                      one TileMapLayer per chunk under one y sorted parent
```

Boundary transports, extending the three in `README.md`:

```text
render -> sim   commands     crossbeam channel   every message arrives
render -> sim   held input   triple buffer       latest wins
sim -> render   snapshot     triple buffer       latest wins
sim -> render   chunks       crossbeam channel   every message arrives   [new]
```

## Third Party Dependencies

| Capability | Chosen | Alternatives | Why |
| --- | --- | --- | --- |
| Noise | `fastnoise-lite` 1.1.1, `default-features = false`, `features = ["std"]` | `noise`, `libnoise`, `simdnoise`, `bracket-noise` | Portability is its stated goal, and it was **verified rather than assumed**: its sources route all float maths through one trait and use only `sqrt`, `trunc`, `abs`, `min` and `max`, every one of which Rust specifies exactly. No `sin`, `cos`, `powf` or `powi` anywhere. `noise` depends on a two major version stale `rand`, is std only, and has an open unanswered report of a malformed 4D gradient table. `simdnoise` dispatches on CPU features at runtime, so output depends on the host. `fastnoise-lite` last released in March 2024, which for a frozen algorithm is a feature, and its four language ports of one spec give an external regression oracle. |
| TSV parsing | `csv` 1.4 | hand rolled split | A spreadsheet on Windows emits CRLF and sometimes a byte order mark, which a hand rolled splitter gets wrong. `csv` 1.4 dropped `bstr`, so the tree is `csv-core`, `itoa`, `ryu`, `serde_core`. |
| Random numbers | none, keep the position hash | `rand`, `fastrand`, `oorandom` | `rand` documents that value breaking changes are permitted in minor versions and has made them in 0.8, 0.9 and 0.10, and its `StdRng` and `SmallRng` are explicitly non portable. A seed defined world cannot take that dependency. A stateless hash of a coordinate is also the stronger primitive: it needs no fixed evaluation order. |
| Threads | `std::thread` plus the existing `crossbeam-channel` | `rayon` | Generation is a set of independent tasks, so there is no work to steal and no reduction to order. Avoiding `rayon` keeps parallel reduction order out of the determinism argument entirely, and `crossbeam-channel` already carries the request and completion queues. |

`cargo deny check` passes with both additions. Worth recording: `csv` is
`Unlicense OR MIT` and clears the allowlist only through its MIT arm, so a
future Unlicense-only crate would fail.

Two rules on existing dependencies. `glam`'s `fast-math` feature must never be
enabled, which its own documentation says may produce platform specific
results. `fastnoise-lite`'s `std` feature must stay on and `libm` off, because
`libm` routes the same operations through a different implementation, and its
`f64` feature would change every noise value.

## Structure

```text
crates/worldgen/
  src/lib.rs        re-exports, and the crate doc stating the three purity rules
  src/hash.rs       domain separated position hash
  src/tile.rs       MaterialId, TileFlags, Tile
  src/rules.rs      WorldRules, the row types, Tables, parse and validate
  src/world.rs      World: rules, seed and the seeded noise handles together
  src/region.rs     region lattice, tier with the home bubble, biome choice
  src/height.rs     terraced height field
  src/site.rs       the point of interest lattice, spacing and separation
  src/chunk.rs      ChunkCoord, ChunkView, the ghost cell layout, generate_chunk
  tests/unit/       determinism, ordering, validation, bubble, ghost cell agreement

crates/game/
  src/chunks.rs     resident chunk lookup for collision and queries
  src/sim.rs        terrace movement; the field clamp and WORLD_SIZE deleted

crates/host/
  src/stream.rs     residency set, worker pool, generation counter, chunk channel
  tests/integration/mod.rs   the tier's first target

crates/render/
  src/origin.rs     floating origin and rebasing, pure
  src/tiles.rs      sprite resolution from the ghost cells, pure
  src/bridge.rs     node lifecycle and property writes only

crates/xtask-world/
  src/paint.rs      tiles and site markers to pixels, pure
  src/cli.rs        the preview command, reachable from the tests
  src/main.rs       argument forwarding only

project/data/
  world.tsv  tiers.tsv  biomes.tsv  materials.tsv
  site_classes.tsv  sites.tsv
```

## Specs & Standards

- **Isometric projection.** A 2 to 1 diamond needs a camera elevation of exactly
  30 degrees, since the width to height ratio is `1 / sin(elevation)`. The
  comments at `tools/blender/src/framing.py:27` and `bake_sprites.py:36` claiming
  `atan(0.5) = 26.57` for a true 2 to 1 diamond are wrong: that value is the
  diamond's on screen edge slope. True isometric, `atan(1/sqrt(2)) = 35.264`,
  yields 1.732 to 1.
- **Rust RFC 3514, float semantics** (`rust-lang/rfcs`,
  `text/3514-float-semantics.md`). Addition, subtraction, multiplication,
  division, remainder, `sqrt`, `mul_add`, comparisons, casts and the rounding
  functions match IEEE 754-2019 exactly, and the strict semantics preclude
  contracting `a*b + c` into a fused operation. Not the IETF RFC of the same
  number, which is an April Fools joke.
- **Standard library float precision** (`std` docs for `f32`, `f64`). The
  precision of `sin`, `cos`, `tan`, `exp`, `ln`, `powf`, **`powi`**, `hypot`,
  `to_radians`, `to_degrees` and the inverse and hyperbolic variants "varies by
  platform, Rust version, and can even differ within the same execution from one
  invocation to the next". None may be called from `worldgen`. `sqrt` is
  separately documented as the exactly rounded IEEE `squareRoot`, so it is safe.
- **Hash map iteration order** (`std::collections::HashMap` docs). Randomly
  seeded per process, with documented arbitrary iteration order; `hashbrown`'s
  default hasher generates a random per hasher seed. No generation output may
  depend on either.
- **Godot `tile_map_data` layout** (Godot 4.7, `scene/2d/tile_map_layer.cpp`,
  `set_tile_map_data_from_array`). A two byte little endian format version,
  currently 0, then 12 byte records of x, y, source id, atlas x, atlas y,
  alternative tile, all little endian 16 bit. Bits 12, 13 and 14 of the
  alternative tile carry horizontal flip, vertical flip and transpose. Verified
  by reading that function: it clears the layer and rewrites every cell, which is
  what makes it right for an arriving chunk and wrong for one tile.
- **Godot tile coordinate range** (same file). Coordinates serialise as 16 bit
  signed integers and are truncated. Community documentation disagrees on whether
  the effect is a wrap or a clamp; the conclusion is the same either way, so chunk
  local coordinates are mandatory rather than tidy.
- **Godot y sorted quadrants** (Godot 4.7 `TileMapLayer` class reference,
  `rendering_quadrant_size`). "The quadrant size does not apply on a Y-sorted
  TileMapLayer, as tiles are grouped by Y position instead in that case."

## Interfaces

```rust
// crates/worldgen

pub const CHUNK_TILES: i32 = 32;
pub const GRID_SIDE: usize = 34;
pub const GRID_AREA: usize = GRID_SIDE * GRID_SIDE;
/// How many levels a character may climb or fall in one step. Symmetric, so a
/// terrace can never become an inescapable basin.
pub const STEP_LIMIT: i8 = 1;
/// Bounds every generated height, so `i8` subtraction in movement cannot
/// overflow and the render side can size its cliff art.
pub const HEIGHT_RANGE: RangeInclusive<i8> = -32..=32;

/// The table texts. Callers supply the strings, so this crate does no I/O.
pub struct Tables<'a> { pub world: &'a str, pub tiers: &'a str,
                        pub biomes: &'a str, pub materials: &'a str,
                        pub site_classes: &'a str, pub sites: &'a str }

/// Reads a table set. The only way in, so every later reader is total.
///
/// # Errors
/// The first broken invariant, naming the table, the row and the field: an
/// unresolvable key, a number out of range, a tier with no biomes, a duplicate
/// name, an empty trailing column, or a height amplitude outside `HEIGHT_RANGE`.
pub fn parse(tables: Tables<'_>) -> Result<WorldRules, Error>;

/// Rules, seed and seeded noise in one value, so the seed cannot disagree with
/// the noise built from it. `Send + Sync`, and cheap to share by reference.
pub struct World { /* .. */ }
impl World { pub fn new(rules: WorldRules, seed: u64) -> Self; }

/// One tile, as a pure function of the world and its coordinate.
pub fn tile_at(world: &World, tile: IVec2) -> Tile;

/// A chunk plus one ring of ghost cells. The ghost cells come from the same
/// function as the interior, so they agree with the neighbour tile for tile.
pub fn generate_chunk(world: &World, coord: ChunkCoord) -> ChunkView;

/// Which chunk a tile belongs to. Floor division, so negatives behave.
pub fn chunk_of(tile: IVec2) -> ChunkCoord;

/// One placed point of interest: where it is, and which kind.
pub struct Site { pub kind: SiteId, pub class: SiteClassId, pub at: IVec2 }

/// The site in one lattice cell of one class, if that cell holds one. Pure, so
/// it can be asked in any order and on any thread.
pub fn site_at(world: &World, class: SiteClassId, cell: IVec2) -> Option<Site>;

/// Every site of every class whose centre is within `radius` tiles. A bounded
/// walk over a window of cells per class, never a search.
pub fn sites_near(world: &World, tile: IVec2, radius: i32) -> Vec<Site>;
```

```rust
// crates/host

/// Finished chunks and evictions. `Arc`, because both `game` and `render` need
/// the same chunk and generating it twice would be the same work twice.
pub enum ChunkMessage { Ready(Arc<ChunkView>), Dropped(ChunkCoord) }

/// Everything that completed since the last call. A `Vec` and not an iterator:
/// an iterator borrowed from `&mut self` would stop the caller touching any
/// other field, which is the trap `bridge.rs` already documents for `Frame`.
pub fn take_chunks(&mut self) -> Vec<ChunkMessage>;
```

Caller obligations, spelled out rather than assumed. `render` must drain the
chunk messages every frame or the queue grows without bound, must free a chunk's
nodes on `Dropped` before painting a later `Ready` for the same coordinate, and
must cap how many chunks it paints per frame, because the first residency fill is
tens of thousands of cells. `host` must filter completions against the current
residency set before forwarding. `game` treats a tile in a non resident chunk as
impassable, except the tile the player already occupies, which must stay passable
or a streaming gap freezes him. A failure to read or parse the tables refuses to
start the simulation with one loud error rather than booting an empty world:
`FileAccess::get_file_as_string` returns an empty string for a missing file, so
the open must be checked explicitly.

## Existing Code & Reuse

| Existing | Disposition |
| --- | --- |
| `game/src/terrain.rs` | Deleted. `TerrainGrid` assumes a fixed finite grid painted once. Its `splitmix64` is reimplemented with domain separation in `worldgen/src/hash.rs`. |
| `game/src/sim.rs` `keep_player_on_the_field`, `WORLD_SIZE`, `TICK_DT_F32` | All deleted **in the streaming task**, not later: the clamp reads `terrain.width()` and confines the player to 23 tiles, so he could not leave the first chunk to prove streaming works. `TICK_DT_F32` exists only because space was `f32`. |
| `game/src/lib.rs` `pub use glam::Vec2` | **Kept**, with `WorldVec` added beside it. `render/src/iso.rs` depends on the re-export existing. |
| `game/src/components.rs` `Facing::from_direction`, `SECTOR_EDGE` | Widened to `f64`. The constant's value changes, so the sector boundary test is rewritten. |
| `game/src/snapshot.rs` `EntityView` | Gains the tile height the entity stands on. Without it no character can be drawn on a terrace, and `render` cannot derive it because it may not hold the chunk the simulation used. |
| `host/src/lib.rs` `alpha_for`, `Frame::alpha` | Widened to `f64`, deleting a narrowing cast here and a widening cast in `bridge.rs`. |
| `host::spawn` doc comment | Rewritten. "Terrain and anything else read off `Sim` directly has to be taken before this call" stops being true once terrain streams. |
| `render/src/bridge.rs` `paint_ground`, `#[init(node = "../Ground")]` | Both deleted. Terrain is no longer one grid painted at `ready`. |
| `project/scenes/main.tscn` | Restructured. `Ground` and `Entities` are siblings today and `bridge.rs` records the consequence: "entities draw on top of flat ground, which occludes nothing". Chunk layers and entity sprites move under one y sorted parent, which is what lets a cliff occlude a character. |
| `render/src/iso.rs` | Gains an origin parameter and `f64` tile inputs, subtracting the origin **before** narrowing to `Vector2`, which is what makes the `f64` decision pay off. `HEIGHT_STEP_PX` belongs here, not in `worldgen`. |
| `render/src/draw.rs` `reconcile` | Reused as the model for chunk node reconciliation, which is the same added and removed set problem. |
| `crates/sprites` | Reused as the pattern for `parse`: one validating entry point so later readers are total. |
| `tools/blender/src/framing.py` | `CAMERA_ELEVATION_DEG` becomes a `Framing` field rather than a module constant, since two elevations now exist. `Framing` is frozen with `extra="forbid"`, so its test changes too. |
| `scripts/src/git_hooks/cargo_coverage.sh` | Gains `--test integration`, so the new tier counts toward the gate instead of being invisible to it. |

## Logic

Height is terraced, so the noise is quantised and only the integer survives:

```rust
pub fn height_at(world: &World, tile: IVec2) -> i8 {
    let row = world.rules.biome(region_at(world, tile).biome);
    // Frequency comes from a designer facing period in tiles, so the table holds
    // "features about 140 tiles across" rather than 0.00714.
    let n = world.noise(Domain::Height).get_noise_2d(
        tile.x as f32 / row.height_period as f32,
        tile.y as f32 / row.height_period as f32,
    );
    // Integer coordinates are exact in f32 up to 2^24, so f64 world positions
    // never reach the noise API. Quantise here: nothing float valued is stored.
    (n * f32::from(row.height_amp)).round() as i8
}
```

Tier, with the home bubble enforced on the region rather than the tile:

```rust
fn tier_of(world: &World, point: RegionPoint) -> u8 {
    let r = &world.rules;
    // One region radius of slack, or a tile inside the bubble can inherit the
    // tier of a region whose point sits outside it.
    if point.distance_tiles() <= r.home_bubble + r.region_pitch / 2 { return 0; }
    let base = r.tier_for(point.distance_tiles());
    let band = r.band_of(base);
    match stray_roll(world, point, band) {
        Stray::None => base,
        Stray::Harder => (base + 1).min(r.max_tier()),
        Stray::Easier => base.saturating_sub(band.easier_stray),
    }
}
```

Terrace movement, replacing the field clamp. Symmetric, so no basin can trap a
player in a game with no jump yet:

```rust
fn can_step(chunks: &Chunks, from: IVec2, to: IVec2) -> bool {
    let Some(b) = chunks.tile(to) else { return false };
    // The tile underfoot stays passable even if its chunk was evicted, or a
    // streaming gap freezes the player permanently.
    let a = chunks.tile(from).unwrap_or(b);
    if b.flags.blocks_walk() { return false; }
    // i16, so the subtraction cannot overflow at the ends of HEIGHT_RANGE.
    if (i16::from(b.height) - i16::from(a.height)).abs() > i16::from(STEP_LIMIT) {
        return false;
    }
    // A diagonal must clear both orthogonal neighbours, or the player squeezes
    // through a cliff corner. Depth is capped at two: each call below shares an
    // axis with `from`, so the guard is false immediately.
    if from.x != to.x && from.y != to.y {
        return can_step(chunks, from, IVec2::new(to.x, from.y))
            && can_step(chunks, from, IVec2::new(from.x, to.y));
    }
    true
}
```

A refused step shortens the move on the blocked axis only, leaving `previous`
alone, exactly as `components.rs` documents for the clamp it replaces, so the
player slides along a cliff rather than stopping dead.

**How height reaches the screen, and why there is no custom sort key.** Godot y
sorts terrain by cell position and entities by node position, and the prior design
forbids `z_index` because inside a y sorted parent it overrides the sort rather
than refining it. So height is applied by moving things up the screen:

```rust
// render/src/iso.rs. Origin subtracted in f64, then narrowed once.
pub fn tile_to_screen(tile: WorldVec, height: i8, origin: WorldVec) -> Vector2 {
    let local = tile - origin;
    Vector2::new(
        ((local.x - local.y) * HALF_WIDTH) as f32,
        ((local.x + local.y) * HALF_HEIGHT) as f32 - f32::from(height) * HEIGHT_STEP_PX,
    )
}
```

Cliff tiles then carry a per tile `y_sort_origin` so they sort by their base
rather than their visual centre. That is the mechanism; the art that needs it is
the next design.

## Edge Cases & Constraints

- **Cascading generation.** The failure this design is shaped to prevent. Any
  future code that reads a neighbouring chunk during generation reintroduces it,
  and the ordering test is the guard.
- **Domain collision.** Two systems hashing one coordinate under one tag get the
  same number, a live defect in `terrain.rs`. Every derivation carries a tag, tags
  are permanent, and **a tag is never folded into a coordinate**: xor is a
  bijection, so `x ^ salt` makes two purposes identical at shifted coordinates.
  A site class is a multiplied variant of its tag for exactly this reason.
- **Noise seeds are tags too.** `fastnoise-lite` takes an `i32` seed, so a `u64`
  world seed is folded per instance. Two instances folding to the same `i32`
  correlate, which is the same defect one layer down, so each noise instance takes
  its seed from `derive` under its own tag.
- **Region jitter is bounded to half the pitch**, which is what makes a 3 by 3
  search provably enough: the own cell point is at most 1.06 pitches away and a
  cell two out is at least 1.25.
- **Godot coordinate range and y sorted quadrants.** Chunk local coordinates keep
  every value inside 0 to 31, so the 16 bit serialisation limit never binds. With
  y sorting on, Godot keys a quadrant per distinct y baseline, so changing one
  cell rebuilds every tile on its diagonal; one layer per chunk bounds that to 32.
- **Bulk paint versus deltas.** The bulk call clears and rewrites the layer, so
  it is right for an arriving chunk and wrong for one tile. Never
  `update_internals` in a loop.
- **Disable what Godot does not own.** `collision_enabled`, `navigation_enabled`
  and `occlusion_enabled` are false on every layer; the simulation owns collision
  and pathfinding, and each is a separate subsystem in Godot's per change update.
- **In flight evictions.** A chunk can finish after `host` dropped it, so
  completions are filtered against current residency before forwarding, or
  `render` paints outside residency and node names collide.
- **The player spawns at the world origin**, which every tier band and the home
  bubble measure from.
- **Non finite values and float free storage.** A non finite position spreads
  through every later tick and makes a snapshot unequal to itself, which is why
  `Input::new` already guards it. Every stored tile field is an integer.
- **Trailing tabs.** The `trailing-whitespace` hook excludes only vendored files
  and markdown, so a TSV row ending in an empty field would be silently stripped
  of a column. `parse` rejects an empty trailing column rather than the hook being
  weakened. Aligned columns in this document are for reading only.

## Test Plan

Unit, in `crates/worldgen/tests/unit`, engine free and I/O free:

- **Golden hash.** A fixed seed and region hash to a recorded value, using a hand
  rolled hash rather than `DefaultHasher`, whose output is not stable across Rust
  versions. The single test that catches most drift. It runs twice in one binary
  too, which catches a randomly seeded hasher leaking in.
- **Order independence.** Chunks generated in a shuffled fixed order are byte
  identical to the same chunks generated sequentially.
- **Domain independence.** Every pair of tags shows correlation near zero, and
  one tag is uncorrelated with its own neighbour.
- **The home bubble holds.** Over many seeds, no tile within the bubble is above
  tier 0. This is the test the region rule exists to pass.
- **Tier monotonicity.** Sampling outward along many rays, mean tier never
  decreases between bands.
- **Ghost cell agreement.** One chunk's ghost cells equal its neighbour's
  interior, tile for tile. The invariant they exist for.
- **Height stays in range**, so movement's `i8` arithmetic cannot overflow.
- **Rules validation.** Each malformed table produces its specific error, naming
  the table, row and field.
- **The separation guarantee holds.** Over a block of lattice cells, no two
  sites of one class are closer than their class's separation. Plus: the tier
  and distance gates are respected, two classes do not place in lockstep, and
  every kind in a class eventually appears.
- **Terrace movement.** One level either way is allowed, two is refused both
  ways, a diagonal through a cliff corner is refused, a blocked tile is refused,
  and the tile underfoot stays passable when its chunk is absent.

Unit, in `crates/render/tests/unit`: sprite resolution is a pure function of a
grid with ghost cells, covering interior, edge, outer corner and inner corner
cases; and the floating origin rebases when the player leaves the origin chunk,
keeping screen coordinates bounded across a simulated 100 km walk.

Integration, in `crates/host/tests/integration`, the tier's first target, which
needs a `[[test]] name = "integration"` stanza in the manifest and a line in the
workflow: a residency radius produces exactly the expected chunk set once each;
moving the centre produces the correct evictions with no leaks; a chunk evicted
while in flight is not forwarded; and **chunks generated on the pool are byte
identical to the same chunks generated on one thread**, which is the test that
the pool inherits position purity rather than assuming it.

CI:

- `cargo_coverage.sh` gains `--test integration`, so the pool's tests count
  toward the 96 percent gate instead of being invisible to it. `tiles.rs` and
  `origin.rs` are pure and measured; `bridge.rs` and the two `main.rs` shims stay
  excluded, on the reason they already carry.
- A `clippy.toml` `disallowed_methods` entry bans the standard library
  transcendentals. It resolves workspace wide, so the art pipeline gets a
  narrowly scoped allow where it legitimately needs trigonometry.
- Assert `getrandom` and `rayon` are absent from the `worldgen` and `game`
  dependency trees, and that `glam`'s `fast-math` is off.
- A test parses the shipped tables, so a broken table fails the build.
- The cross platform matrix is deferred with the rest of determinism's
  consumers: `.github/workflows/CLAUDE.md` asks for one runner reused, and a
  matrix defends replays and saves, which do not exist yet.

## Documentation Changes

- `README.md`: the monorepo layout gains `crates/worldgen` and
  `crates/xtask-world`, the transport summary becomes four, and the testing table
  gains integration coverage in `host`.
- `crates/worldgen/README.md`: new. The purity rules, and the permanent registry
  of domain tags with the warning that renumbering one changes every world.
- `project/data/README.md`: new. What each table means, the keys between them,
  the validated invariants, and how a mod overrides one.
- `crates/xtask-world/README.md`: new, and the `.cargo/config.toml` alias beside
  the existing `art` one.
- `docs/concept.md`: every section that contradicts the new world model is
  rewritten as part of this work. **World Structure**; **Environmental Zones
  (Examples)**, the six named zone table; **World Generation & Replayability**;
  **Enemy Scaling**, which describes fixed named zones; the **World gen** row of
  the comparison matrix, reading "Hybrid (fixed + proc)"; and the settlement
  claims in **Alone in the Dark** and **Normal Mode**. The file uses em-dashes
  throughout, against the current style rule; only the rewritten sections are
  normalised.

## Development Environment Changes

- `Cargo.toml` workspace members gain `crates/worldgen` and `crates/xtask-world`;
  workspace dependencies gain both path entries plus `fastnoise-lite` and `csv`.
- `.cargo/config.toml` gains a `world` alias beside `art`.
- `project/data/` is created. Godot exports resources by default and skips
  everything else, so `project/export_presets.cfg` is created with an
  `include_filter` of `*.tsv,*.ron`. The `*.ron` half fixes the same latent gap
  for `character.ron`.
- `crates/host/Cargo.toml` gains the integration test target, and
  `cargo_coverage.sh` gains `--test integration`.
- No `Brewfile` change, and no new environment variable: the preview command takes
  flags, which are discoverable where an environment variable is not.

## Tasks

| #   | Task Name | Task Description | Success Criteria | Dependencies |
| --- | --------- | ---------------- | ---------------- | ------------ |
| 1 | Rules, generation and a picture | `crates/worldgen` with the hash, tile types, TSV tables and validation, the region lattice with the home bubble, the terraced height field, the point of interest lattice, and `crates/xtask-world` rendering a region to a PNG with a marker on every site. | A PNG of a seed shows contiguous regions whose tier grows outward, shaded by height, with site markers respecting their spacing rules. Golden hash, bubble, monotonicity, separation, validation and order independence tests pass. A broken table fails the build. | none |
| 2 | Large world coordinates | `WorldVec` as `DVec2` beside `Vec2`, `alpha` widened to `f64`, and the floating origin with an origin parameter on the projection. | A scripted 100 km traversal keeps screen coordinates bounded with no jitter. Existing movement tests are rewritten to `f64` and pass, including the sector boundary case. | none |
| 3 | Chunk streaming | `generate_chunk` with its ghost cells; the residency set, worker pool and in-flight generation counter in `host`; `Arc` delivery to both `Sim` and `render`; per chunk `TileMapLayer` nodes under one y sorted parent in a restructured `main.tscn`; the integration tier wired up and `--test integration` added to the coverage hook; and deletion of `TerrainGrid`, `WORLD_SIZE` and the field clamp. | Walking in game streams chunks in and frees them behind, with no frame above 16.7 ms attributable to generation or painting. Ghost cell agreement and the integration tests pass. | 1, 2 |
| 4 | Terraced movement | The symmetric step limit, the diagonal corner rule, the slide on a refused step, and the height carried on `EntityView` and applied to the drawn position. | A one level step is walkable either way, a two level face is not, a cliff corner cannot be squeezed through, and the survivor draws at the right height on a plateau. | 3 |

```text
1  rules, generation and a picture ---+
                                      +--> 3  chunk streaming --> 4  terraced movement
2  large world coordinates -----------+
```

Tasks 1 and 2 are independent, so the generator and the coordinate migration
can proceed in parallel.
