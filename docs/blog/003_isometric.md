# The Isometric View: My Understanding

The simulation thinks in a flat square grid. The screen shows a tilted world of
diamonds. This is how one becomes the other.

## 1. Why tiles exist

The floor needs drawing. Painting one enormous image of the whole map fails
immediately: a 24x24 world would need a 3000-pixel-wide picture, and a real
world is thousands of tiles across.

So instead: **draw one small image many times**. 576 draws of three tiny
pictures.

That leaves two separate questions, and they are answered in different places:

| Question | Answered by |
|---|---|
| Which picture goes in each cell? | `crates/game` (terrain generation) |
| Where on screen does cell (x,y) go? | the tileset file (isometric layout) |

---

## 2. What isometric means

Stop hovering directly above the floor. Stand at a corner of the world and look
down at an angle, like viewing a chessboard from the corner of the table.

A square tile stops looking square. It becomes a **diamond**.

```text
   seen from directly above          seen from a corner

        +-----------+                       /\
        |           |                     /    \
        |           |                   /        \
        |           |                   \        /
        |           |                     \    /
        +-----------+                       \/
```

That is the whole trick. Isometric is a **2D drawing that reads as 3D**,
because the eye interprets diamonds as tilted squares.

The important part: **the world is still a flat square grid.** Tile (5,3) is
still tile (5,3). Only the drawing changed.

---

## 3. Finding where a tile goes

Do not start from the formula. Start from one step.

> I am standing on tile (0,0). I step to tile (1,0). How far did I move on
> screen?

Here is a 4x4 grid with each label sitting exactly where that tile lands:

```text
                 (0,0)
             (0,1)   (1,0)
         (0,2)   (1,1)   (2,0)
     (0,3)   (1,2)   (2,1)   (3,0)
         (1,3)   (2,2)   (3,1)
             (2,3)   (3,2)
                 (3,3)
```

### One step in x

Find (0,0), then (1,0). It is **below and to the right**. In pixels:

```text
one step in x  =  64 right,  32 down
```

Our tile is 128 wide and 64 tall. So 64 is **half the width** and 32 is **half
the height**.

### Why half, and not a whole tile

Suppose a step moved a whole tile width across and nothing down. Two diamonds
would touch only at their pointy corners:

```text
        /\        /\
      /    \    /    \
      \    /    \    /
        \/        \/
           ^
      they meet at one point,
      leaving triangular holes
```

That leaves **gaps**. The floor would have holes in it.

Diamonds cannot sit in neat rows. They have to **interlock**, each one nestled
into the notch between two others, which is a pattern you already know from a
**brick wall**: every row shifted by half a brick. Isometric tiles do it in
both directions at once.

Half a width across and half a height down is the only offset where the edges
line up exactly. That is where the halves come from. It is not a maths trick.

### One step in y

From (0,0), find (0,1). It is **below and to the LEFT**.

```text
one step in y  =  64 LEFT,  32 down
```

Same distances. The only difference is the direction sideways. The two axes
have to spread apart, or they would land on top of each other and give a line
instead of a grid:

```text
                 (0,0)
                /     \
        y-step /       \ x-step
              /         \
          (0,1)         (1,0)
```

Which one leans left is a convention. Godot's `DIAMOND_DOWN` picks this one.

### The two facts that are the whole formula

| Step | Horizontal | Vertical |
|---|---|---|
| one step in **x** | **+64** (right) | **+32** (down) |
| one step in **y** | **-64** (left) | **+32** (down) |

The vertical column is identical. Only the horizontal differs, and only in
sign.

### Walking several steps: just add them up

Where is tile (3,2)? That is 3 x-steps and 2 y-steps.

```text
3 x-steps:  right 192,  down 96      (3 x 64, 3 x 32)
2 y-steps:  left  128,  down 64      (2 x 64, 2 x 32)

horizontally:  192 right - 128 left  =  64 right
vertically:     96 down  +  64 down  = 160 down
```

Tile (3,2) is at pixel **(64, 160)**. No geometry, just counting.

### The formula is that addition, written short

Take `tile_x` steps in x and `tile_y` steps in y:

```text
horizontally:  (tile_x x 64) - (tile_y x 64)  =  (tile_x - tile_y) x 64
vertically:    (tile_x x 32) + (tile_y x 32)  =  (tile_x + tile_y) x 32
```

So:

```text
pixel_x = (tile_x - tile_y) x 64
pixel_y = (tile_x + tile_y) x 32
```

Every piece has a plain meaning:

| Piece | What it is |
|---|---|
| **64** | half the tile width (128 / 2) |
| **32** | half the tile height (64 / 2) |
| **the minus** | y-steps go left while x-steps go right, so they cancel |
| **the plus** | both steps go down, so they pile up |

### Two checks you can do in your head

**If x and y are equal, the tile is straight down from the origin.** Because
`x - y = 0`, no sideways movement. In the picture, (0,0), (1,1), (2,2) and
(3,3) form a straight vertical line.

**The bigger `x + y`, the further down.** Every tile whose numbers add to the
same total sits on the same row. In the picture's fourth row: 0+3, 1+2, 2+1,
3+0, all equal 3.

> The sum tells you the row. The difference tells you the column.

We never write this formula ourselves. `map_to_local` in `bridge.rs` asks Godot,
so the camera stays correct if the tile size ever changes.

---

## 4. Why the tile is 128 x 64

Two questions in one. The **ratio** is forced. The **size** is a preference.

### The 2:1 ratio is forced

A screen has no diagonal lines, only square pixels in rows. Drawing a diamond's
sloped edge means a **staircase**: fill some pixels, drop a row, fill more.

With a 2:1 tile, the edge moves 64 across while rising 32, which is exactly
**2 pixels across per row**:

```text
##
  ##
    ##
      ##
        ##
```

Steps of 2, 2, 2, 2. Perfectly regular, and a pixel artist can draw it clean.

True geometric isometric uses a 30 degree angle, which needs a **sqrt(3) : 1**
ratio, roughly 1.732 : 1. For a tile 64 pixels tall that means a width of:

```text
64 x 1.732 = 110.851 pixels
```

Not a whole number, which is fatal. And the staircase becomes irregular:

```text
cumulative:  0, 2, 3, 5, 7, 9, 10, 12, 14, 16
steps:          2, 1, 2, 2, 2,  1,  2,  2,  2
```

```text
##
  ##
   ##      <- shifted only 1 this time
     ##
       ##
```

The eye reads that as a **wobble** in what should be a straight line, on every
tile edge in the game.

| | True isometric | 2:1 |
|---|---|---|
| edge angle | 30.000 deg | **26.565 deg** |
| pixels across per row | 1.7321 | **2.0000** |
| staircase | irregular | regular |
| width for a 64-tall tile | 110.851 px | **128 px** |

So 2:1 is 3.4 degrees flatter than true isometric, and nobody has ever
noticed. Pedants call it *dimetric*. Everyone in games says isometric.

### The absolute size follows the character

64x32, 128x64 and 256x128 all have identical geometry. What changes is detail
versus coverage.

The rule is: **you do not pick the tile size, you pick the character size and
the tile follows.** The player looks at the character all game. Floor tiles are
background.

Our art pipeline bakes the survivor at **160 px tall** with a **63 px** idle
footprint. Against that:

| Tile | Character is | How it reads |
|---|---|---|
| 32x16 | 5.00x tile width | towers over the floor, toy-like |
| 64x32 | 2.50x | still looming |
| **128x64** | **1.25x** | slightly taller than a tile is wide, human-scale |
| 256x128 | 0.62x | smaller than a tile, world feels empty |

128x64 is the size that makes a 160 px character read as a person standing on
ground. The footprint is 0.49x the tile width, so two characters on adjacent
tiles do not overlap.

---

## 5. What our world looks like

`WORLD_SIZE = 24`, so tiles run (0,0) to (23,23). The four extremes:

| Tile | pixel_x | pixel_y | Position |
|---|---|---|---|
| (0, 0) | 0 | 0 | topmost |
| (23, 0) | +1472 | 736 | rightmost |
| (0, 23) | -1472 | 736 | leftmost |
| (23, 23) | 0 | 1472 | bottommost |

The four corners of a square grid become the four points of a diamond:

```text
                         (0,0)
                       /       \
                     /           \
        (0,23)                     (23,0)
                     \           /
                       \       /
                        (23,23)
```

**A square region always renders as a diamond**, the same shape as one tile,
just 24 times bigger.

One detail: `map_to_local` returns the **centre** of a tile, not a corner. So
the drawn area extends half a tile in every direction beyond the anchors:

```text
anchor x range:   -1472 .. +1472      drawn:  -1536 .. +1536
anchor y range:       0 .. +1472      drawn:    -32 .. +1504
```

**Total: 3072 x 1536 pixels.** Note the top edge sits at y = -32, above the
origin, because tile (0,0) pokes half a tile height upward.

Sanity check: 3072 / 1536 = 2. The whole world has the same 2:1 ratio as one
tile.

---

## 6. Setting up the camera

The world is 3072 px wide. The window is 1280 px wide. Two knobs:

### Position

`bridge.rs` asks for the centre of tile (24/2, 24/2) = (12,12):

```text
pixel_x = (12 - 12) x 64 = 0
pixel_y = (12 + 12) x 32 = 768
```

The true centre of the drawn extent is (0, **736**). So the camera is **32 px
too low**, exactly half a tile height, because the real middle of 0..23 is
11.5 and integer division rounds 24/2 up to 12.

Tile (11.5, 11.5) would land at (0, 736) exactly. Harmless at this scale, but
worth knowing if the world ever looks slightly high.

### Zoom

The one Godot convention to accept: **zoom below 1 shows more world**, because
it scales everything down.

```text
zoom = 0.4  ->  1280 / 0.4 = 3200 px wide
                 720 / 0.4 = 1800 px tall
```

Where does 0.4 come from? Work backwards from fitting 3072 x 1536:

```text
to fit the width :  1280 / 3072 = 0.41667
to fit the height:   720 / 1536 = 0.46875
```

Two answers, so **use the smaller one**, or the sides get cropped. That is a
general rule: fitting one rectangle inside another means taking the more
restrictive axis.

`0.4` is the roundest number below 0.41667, leaving 128 px spare horizontally
(exactly one tile) and 264 px vertically. So it is not arbitrary: **it is the
zoom at which the whole world is visible.**

That makes it a development setting. A gameplay zoom nearer 1.0 would show
about 20 tiles across, which is a plausible view distance for the genre.

`Camera2D`, not `Camera3D`, because there is no 3D here. The game looks
three-dimensional but everything is flat images placed cleverly.

**When the camera follows a player**, it must follow the *drawn* interpolated
position, not the *simulated* one. Otherwise the player jitters against the
background.

---

## 7. The atlas

Three floor pictures. Three files, or one file with three pictures side by
side?

**One**, because of how GPUs work. Switching which image is bound is expensive.
Three files would mean the GPU switching repeatedly across 576 tiles. One file
means it binds once. That is a **texture atlas**.

Ours is **384 x 64**, which at 128 x 64 per tile is exactly three in a row:

```text
ground_atlas.png     384 x 64
+---------------+---------------+---------------+
|   variant 0   |   variant 1   |   variant 2   |
|   128 x 64    |   128 x 64    |   128 x 64    |
+---------------+---------------+---------------+
0             128             256             384
```

| `atlas_coords` | Pixels cropped |
|---|---|
| (0, 0) | x 0..127 |
| (1, 0) | x 128..255 |
| (2, 0) | x 256..383 |

And this is where terrain generation reconnects. `variant_for` returns 0, 1 or
2, used directly as the column:

```rust
.atlas_coords(Vector2i::new(i32::from(variant), 0))
```

**Variant number = atlas column.** No lookup table; the numbers were chosen to
line up.

### Two coordinate systems, easy to confuse

Both are `Vector2i`, and they mean completely different things:

| | Meaning |
|---|---|
| **map coords** | which cell of the *world*: `set_cell_ex(Vector2i::new(1, 0))` |
| **atlas coords** | which picture in the *image*: `.atlas_coords(Vector2i::new(0, 0))` |

Swap them and you get the wrong picture in the wrong place, with no error.

---

## 8. How Godot sees all this

Three things with confusingly similar names. The rubber stamp analogy:

| Godot thing | Analogy |
|---|---|
| **TileSetAtlasSource** | a sheet of **rubber stamps**: pictures, cut this size |
| **TileSet** | the **stamp kit**: stamps plus rules for how impressions line up |
| **TileMapLayer** | the **sheet of paper**: which stamp went in which cell |

Where the analogy breaks: real stamps carry their own shape, but here the shape
rules live on the TileSet, not on the pictures. The same atlas renders square
or isometric by changing one number.

One is a **file** (a shared `.tres` resource), one is a **node** in the scene.

### The file, decoded

```ini
[sub_resource type="TileSetAtlasSource" id="TileSetAtlasSource_ground"]
texture = ExtResource("1_atlas")
texture_region_size = Vector2i(128, 64)
0:0/0 = 0
1:0/0 = 0
2:0/0 = 0
```

| Line | Meaning |
|---|---|
| `texture` | points at `ground_atlas.png` |
| `texture_region_size` | chop the image into 128x64 cells: 3 columns, 1 row |
| `0:0/0 = 0` | "the tile at column 0, row 0 **exists**" |

Those last three lines are the ones people miss. **Godot does not assume every
atlas cell is a usable tile**, you declare each one. Widen the atlas, bump
`GROUND_VARIANTS` to 4, forget to add `3:0/0 = 0`, and that tile **silently
does not draw**. Nothing checks this at build time.

```ini
[resource]
tile_shape = 1
tile_layout = 5
tile_size = Vector2i(128, 64)
sources/0 = SubResource("TileSetAtlasSource_ground")
```

| Line | Meaning |
|---|---|
| `tile_shape = 1` | **ISOMETRIC**. This single digit makes the game isometric. |
| `tile_layout = 5` | **DIAMOND_DOWN**: +x down-right, +y down-left |
| `tile_size` | the **on-screen** size, which drives `map_to_local` |
| `sources/0` | registers the atlas as source **0** |

`texture_region_size` and `tile_size` are both (128,64) here but mean different
things: how big a chunk to **cut from the image**, versus how big the tile is
**on screen**. They do not have to match.

### One call, all the way through

```rust
ground
    .set_cell_ex(Vector2i::new(1, 0))          // WHERE in the world
    .source_id(0)                              // WHICH atlas
    .atlas_coords(Vector2i::new(0, 0))         // WHICH picture in it
    .done();
```

```text
set_cell_ex(1, 0)
   |  record: cell (1,0) holds source 0, atlas (0,0)
   v
source_id(0) -> sources/0 -> the TileSetAtlasSource
   |
   v
atlas_coords(0,0) + texture_region_size(128,64)
   |  crop pixels x 0..127, y 0..63
   v
tile_shape=ISOMETRIC, tile_layout=DIAMOND_DOWN, tile_size=(128,64)
   |  pixel_x = (1-0) x 64 = 64
   |  pixel_y = (1+0) x 32 = 32
   v
draw that crop, centred at screen pixel (64, 32)
```

`.done()` is not optional. Leave it off and nothing happens: you built a
description of an action and threw it away. `#[must_use]` catches it.

---

## 9. Who knows what

| Knows about | `crates/game` | `crates/render` | tileset file |
|---|---|---|---|
| tile coordinates | **yes** | yes | no |
| variant numbers 0/1/2 | **yes** | passes through | no |
| pixels | no | asks Godot | **yes** |
| isometric | no | no | **yes** |
| the atlas image | no | column index only | **yes** |

**The simulation has no idea the game is isometric.** Change `tile_shape` from
1 to 0 and it renders top-down, with not one line of `crates/game` altered.

`Position` is a pair of f32 in tile units, so entities sit at continuous
positions like (3.7, 12.2). Nothing is grid-locked. **Tile size is a purely
reversible rendering decision.**

Two things worth remembering:

- **Nobody has decided what a tile means in metres.** `Velocity` is documented
  as tile units per second, but with no scale, no speed value can be
  sanity-checked. The art implies roughly 0.8 m per tile (a 160 px character is
  2.24 tile-edges tall, which lands near 1.8 m at that scale).
- **The 8 baked directions mean facing snaps to 45 degrees.** Move at 20
  degrees and the sprite shows the 45 degree pose. That is the Diablo 2 look
  and it is permanent, not a bug to fix later.

For context: Diablo 2 used 160x80 tiles, subdivided 5x5 into 32x16 subtiles for
collision, so gameplay ran finer than the art grid. We get that for free with
continuous positions. Modern AAA isometric games (Diablo 4, Path of Exile 2)
are fully 3D with a locked camera; our baked-sprite approach is the Diablo 1
and 2 technique, which buys cheap rendering and total art control at the price
of those fixed 8 directions.

---

## The whole thing in one flow

```text
seed + tile (x,y)
     |  splitmix64, then % 100
     v
variant 0, 1 or 2                        <- crates/game stops here
     |
     |  atlas_coords(variant, 0)
     v
column of a 384x64 atlas
     |  texture_region_size 128x64
     v
a 128x64 crop of floor art
     |  tile_shape = ISOMETRIC
     |  tile_layout = DIAMOND_DOWN
     v
pixel_x = (x - y) x 64
pixel_y = (x + y) x 32
     |
     v
drawn as a diamond, centred at that pixel
     |
     v
Camera2D at (0, 768), zoom 0.4
     |
     v
the whole 3072 x 1536 world on screen
```
