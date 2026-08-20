# Decisions: Decoupled Movement And Aim

Where the spike landed, and what is still open. This drives the rewrite of
`2026_08_19_decoupled_movement_and_aim.md`, which predates every decision below
and does not yet agree with any of them.

Everything here was decided by playing the spike, not by argument.

## Settled

### The control scheme

| # | Decision | Value |
| --- | --- | --- |
| 1 | Aim comes from the mouse, movement from WASD | Both reach the simulation each frame |
| 2 | The body faces the cursor, not the way it walks | Everything that is not the player still turns the way it moves |
| 3 | **WASD points at the screen, not at the cursor** | `W` is up-screen whatever the aim. Eight directions |
| 4 | Turning is instant | One tunable rate, set high enough to finish inside a tick, so it can be slowed later without a redesign |
| 5 | Backing away is slower than running | `BACKWARD_SPEED = 0.55` of full speed |

Decision 3 matches Path of Exile 2, checked against its own forum rather than
assumed. A player there complains that "the character moves without facing the
direction of movement (for example, running backward)" and that they must
constantly reposition the cursor to line the two up. Movement and facing can
therefore disagree, which only happens if the keys are pinned to the screen.
Their movement is eight directional, and both they and Diablo IV work this way.

The alternative, walking toward the cursor, was built and played. It gives free
circle-strafing, but left and right invert the moment the aim passes horizontal,
because the character then faces the camera. That is the tank-controls fault and
no sign flip removes it. Rejected on feel, then confirmed against Path of Exile
2's own behaviour.

Backing away being slower is a combat decision, not a graphics one. It makes
retreat cost something, and it also fixes a foot slide: `walk_back` is authored
for a walking gait and read as frantic when dragged along at running speed.

### Accuracy is separate from the sprite

**Attacks read the raw cursor direction. The sprite pose is cosmetic.**

The snapshot publishes both: an exact unit aim, and that aim snapped to whatever
row count the atlas has. Anything that must be accurate reads the first.

This is the most load-bearing decision here. It means the direction count can
never cost the player a shot, so choosing it is a question about how the game
looks and never about how it plays.

### Direction count

**16 for the player**, confirmed by playing it. Diablo II shipped 16 for player
characters and 8 for monsters, confirmed both from Blizzard North's postmortem
and by reading all 3,511 of its shipped `.cof` files.

Two facts make this cheap to revisit:

- The renderer asks the atlas how many rows it has. Changing the count is a
  re-bake and no code change.
- A bake costs about **0.23 seconds per frame**, measured. 16 directions across
  three clips is 848 frames, about **3 minutes**. Direction count is not
  expensive; it was wrongly treated as though it were.

### The bake's stops stay evenly spaced in the world

> Wedge widths here are the screen width of the world sector centred on a pose,
> `2 * atan(tan(h) / 2)` near the horizon and its complement near vertical, for
> a half sector `h`. They cannot be found by halving the eight pose figures,
> because the projection is not linear in angle.

**Rejected: spacing them evenly on screen.** It was built, baked and played
against the world-uniform set on a toggle, and it felt worse.

The reasoning that made it look attractive was that the 2:1 projection turns
equal world wedges into unequal screen ones, 11.4 degrees where the aim is near
horizontal and 43.4 where it is near vertical at sixteen. Evening that out
looked free.

It is not free, because **redistribution is zero sum**. Evening the wedges
widens the sharpest region as much as it narrows the mushiest: the horizontal
band goes from 11.4 degrees to 22.5. That band is where a player spends most of
their aim, because the screen is wider than it is tall, so the change took
precision from where it was being used.

World-uniform is also the honest distribution. It gives equal resolution in the
*world*, and the screen unevenness is only the projection. Gameplay is in world
space. Diablo II ships this.

The lesson worth keeping: the answer to a pose being too coarse is **more
poses**, never a different arrangement of the same ones.

### Equipment layering is its own design

In scope for the game, out of scope for this document. It decides which items
show, how layers stack and what draws in front of what at each angle, and it
should be designed when there are real items to test against. Diablo II needed a
draw order per direction *and* per frame, which is a design on its own.

### Four locomotion clips

**In scope: forward, backward, strafe left, strafe right.** The body always
matches the way he really travels, so he never appears to slide sideways or
moonwalk.

The stride comes from his travel direction measured against where he points, so
walking at the cursor runs forward, walking away from it plays the backward clip
while he still faces the threat, and stepping around it strafes.

Only `run` and a broken `walk_back` exist today, so the spike splits travel two
ways and sideways picks whichever is nearer. That is a stand-in for the four,
not a smaller design.

### Animation frame count

**20 frames stays.** Diablo II shipped 8 and the research argued for it, but 8
and 10 were judged unacceptable on screen and 12 only passable. Rejected by eye,
which beats the citation.

### Movement speed

The 2:1 diamond means world speed and screen speed cannot both be constant. One
of them is always uneven, and it shows up on whichever side is not pinned.

`ISO_SPEED_COMPENSATION` picks the point on that scale: `0.0` holds world speed
constant the way Diablo II does, `1.0` holds screen speed constant.

**Settled for now: `PLAYER_SPEED` 4.0, `BACKWARD_SPEED` 0.55, and
`ISO_SPEED_COMPENSATION` 0.5**, all judged by playing.

The compensation is the one to revisit, and the trigger is combat rather than
taste. Everything else in the game is measured in tiles: attack reach, area
radius, aggro range, dodge distance. Movement at anything above `0.0` is the one
exception, so how far a second of running carries him depends on the direction.
An area attack he can outrun upward can then kill him sideways, with nothing on
screen explaining why. That is worse than a visible quirk, because it is
invisible.

Against that, the unevenness at `0.0` is real and was spotted at once. Some of
it is the placeholder ground: a regular checkerboard is a strong cue for how
fast tiles pass, and terrain art with irregular detail hides most of it. That is
why Diablo II ships `0.0` without complaint.

So: hold `0.5`, and try `0.0` again once real terrain and the first ranged
attack exist, which is when the invisible failure would start to bite.

### Attacking roots him

**He stops to swing.** No clip ever combines locomotion with an attack.

Diablo II works this way, and so does Dark Souls. It costs nothing to build,
because the combination never has to exist. More importantly it is what
`docs/concept.md` already asks for: attacks and dodges spend stamina and the
combat is meant to be punishing. Committing to a swing is the risk the stamina
system exists to create, and free movement during an attack quietly removes it.
The dodge roll is the way out, not walking.

This is the one place where sprites constrain gameplay rather than looks, so it
is worth being explicit that the constraint was accepted rather than suffered.
Path of Exile 2 gets movement during attacks by blending animations additively
at runtime, legs walking while the upper body shoots. That is skeletal 3D, and
Godot's `BlendSpace2D` will not interpolate sprite frames.

The escape hatch, if a playtest disagrees: the same blend can be done once in
Blender, masking the lower body to the locomotion clip and the upper body to the
attack, and baked as its own clip. The pipeline already moves actions onto the
character's armature and composes per-bone corrections, so it extends rather
than replaces. It costs locomotion states times attacks, about 20 clips and 25
minutes of baking for a four-by-five set, and it multiplies again with weapon
classes. Unproven, so it needs a spike before it is relied on.

A hybrid is the likely landing point if this ever changes: light attacks allow
movement, heavy ones root, and only the combinations actually wanted get baked.

### The renderer stays 2D

Real-time 3D was seriously considered and rejected for now. It would make
direction count, the facing-crossed-with-travel matrix and per-item equipment
bakes all disappear, and the 3D source assets already exist. It was rejected
because the sprite pipeline works today and the look is already established.

Revisit if any of these becomes true, rather than waiting to discover it:

- The equipment catalogue passes roughly 20 visually distinct pieces
- A rotatable camera is wanted
- The clip count passes about 12

## Open

Ordered by how much they block the design.

| # | Decision | Options | Notes |
| --- | --- | --- | --- |
| 1 | Where the strafe pair is bought | Several suppliers, to be compared in practice | The clips themselves are settled scope. Only the supplier is open, and it is deferred on purpose: done at the end with the human, trying real candidates rather than choosing on paper. Meshy has no upright strafe |
| 2 | Replace `walk_back` | Its last frame does not match its first, so he hops every time the loop restarts | Same session as 1 |
| 3 | Revisit `ISO_SPEED_COMPENSATION` | Hold `0.5`, or move to Diablo II's `0.0` | Deliberately held, not undecided. Retry once real terrain and a ranged attack exist |
| 4 | Whether a turn rate is ever slowed | The constant exists and is set to instant | Pays off once attacks commit the character to a direction |
| 5 | Whether 32 poses are worth twice the art | 16 is settled and shipped; 32 is proven in the pipeline and a 12 minute re-bake | Only open because 32 was never judged. At 32 the wedges are 5.6 degrees at the sharpest and 22.3 at the mushiest |

## Attacking while moving

Path of Exile 2 blends animations additively at runtime: the legs play the walk
cycle while the upper body shoots or casts, per their own developer interview.
That is a skeletal 3D technique. Sprites cannot blend at runtime, and Godot's
`BlendSpace2D` will not interpolate sprite frames, it snaps to the nearest.

There are two honest answers, and the choice is about combat rather than
rendering.

**Root him on attack.** Diablo II does this, and so does Dark Souls. It costs
nothing: no combination is ever needed, because he is never running and swinging
in the same frame. It also matches `docs/concept.md`, which puts attacks and
dodges on a stamina budget and calls the combat punishing. Committing to a swing
*is* the risk the stamina system exists to create, and free movement during
attacks quietly removes it.

**Or blend at bake time.** The blending Path of Exile 2 does per frame can be
done once, offline: mask the lower body to the locomotion clip and the upper
body to the attack, then render the result as its own clip. The pipeline already
moves actions onto the character's armature and already composes per-bone
corrections, so this extends it rather than replacing it. Unproven, so it needs
a spike before it is relied on.

The cost is that clips become locomotion states times attacks. Four states and
five attacks is 20 clips, about 6,400 frames at 16 directions, roughly 25
minutes of baking. That is affordable, but it multiplies again with weapon
classes, which is the axis that made Diablo II's asset count explode.

Rooting is the recommendation, with bake-time blending as the escape hatch for
specific attacks later. A hybrid, where light attacks allow movement and heavy
ones do not, costs only the combinations actually wanted.

## Sourcing the strafe clips

The clips are in scope. This is only about where they come from, and it is the
one part of this work with a real cost.

Meshy's public catalogue has backward walks and two backward diagonal runs, and
**no upright strafe at all**: the nearest entries are crouched or hold a weapon.
So the strafe pair needs a second source and a retarget onto the 24 bone rig.
The same trip should replace `walk_back`.

Alternatives were surveyed with licences and prices, and that comparison belongs
in the design document. The choice itself is deliberately made at the end, by
trying real candidates in the pipeline rather than picking one from a table.

## What the spike changed

Throwaway, but it is currently on the branch and every unit test passes (277).
It is the reference for what the real implementation should do, not the
implementation itself.

- `game`: `Input` carries an aim; an `Aim` component; facing follows it;
  `Locomotion` gained a backward stride; the snapshot publishes the raw aim
- `render`: cursor sampled once a frame with a dead zone; the sprite row comes
  from the raw aim quantised to the atlas; the keys convert to tile space
  through one function, with the isometric speed trade named in it
- `xtask-art` and `tools/blender`: 16 direction support, and a comment corrected
  that called 8 "the Diablo II layout" when 8 is its *monster* layout

Two faults were found by feel rather than by test, and both now have tests. Two
code paths converted the keys with different maths, so fixing one left the other
wrong. And a direction longer than unit length is silently clamped by `Input`,
which quietly undoes any speed compensation applied before it.
