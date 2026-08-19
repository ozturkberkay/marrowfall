# Deterministic Simulation + Rendering: My Understanding

The game has two separate responsibilities: **the simulation** and **the
renderer**.

## 1. The simulation

The simulation is written in Rust and runs at a fixed **60 Hz**.

That means:

- There are **60 ticks per second**.
- Each tick represents **1/60th of a second = 16.67 ms of game time**.
- A tick is essentially one iteration of the simulation loop.

During each tick, the simulation takes the current game state and inputs, runs
the game logic, and produces the **next state of the world**.

For example:

- Move the player.
- Move monsters.
- Apply damage.
- Update health.
- Run AI.
- Spawn or destroy entities.
- Update cooldowns.
- Etc.

The important distinction is that **16.67 ms is the amount of game time the tick
represents, not how long the computation is allowed to take**.

The simulation might calculate a tick in 2 ms, 10 ms, or even 20 ms. If it takes
longer than 16.67 ms, the simulation starts falling behind real time.

---

## 2. The renderer

Godot is responsible for rendering the game.

The renderer is **not locked to 60 FPS**.

For example, the player's monitor might run at:

- 60 Hz
- 144 Hz
- 240 Hz
- Variable refresh rate

Therefore, Godot might render **144 frames per second** while the Rust
simulation only produces **60 simulation states per second**.

The simulation and renderer are therefore **decoupled**.

The Rust simulation determines:

> "This is the actual state of the game."

Godot determines:

> "Given the states I have, how should I visually display them?"

---

## 3. How the two halves talk

The simulation does not run inside the renderer. It runs on its **own thread**,
started when the game boots.

The reason is that Godot owns the main thread, and most Godot classes are not
safe to touch from any other thread. So the split is strict:

> The simulation thread never touches Godot.
>
> The main thread never touches the simulation's world directly.

Everything that crosses between them is plain data.

```text
       MAIN THREAD                              SIM THREAD
     (Godot owns this)                        (we own this)

  draws at the monitor rate               ticks at exactly 60 Hz
  60, 144, 240 Hz or variable             always 16.67 ms per step

         |                                          |
         |  ---------- commands, input --------->   |   a queue
         |                                          |   nothing is lost
         |                                          |
         |  <--------- RenderSnapshot ----------    |   a triple buffer
         |                                          |   latest wins
```

### Two directions, two different transports

The two directions do not want the same behaviour, so they do not use the same
mechanism.

**Into the simulation: a queue.**

Commands and player input go in through a channel. Every message matters. If
the player clicks and that click is dropped, the character does not move. So
messages wait in line until the simulation reads them, and nothing is lost.

**Out of the simulation: a latest-wins slot.**

Snapshots come out through a triple buffer, which only ever holds the newest
one. If the simulation produced three snapshots since the last frame, the
renderer wants the third. The first two describe a world that no longer
exists.

So superseded snapshots are overwritten rather than queued.

> Losing a command would be a bug.
>
> Keeping a stale snapshot would also be a bug.
>
> That is why the two directions are built differently.

### Why three buffers

The name comes from having three slots. At any moment:

- the simulation is writing into one,
- the renderer is reading from one,
- the third holds the most recent finished snapshot.

When the simulation finishes writing, it swaps its slot with the middle one.
When the renderer wants fresh data, it swaps its slot with the middle one.

Neither side ever waits for the other, and neither can ever see a half-written
snapshot.

That last part is the reason for the third slot. A snapshot part-way through
being written is not a valid picture of the world. The extra slot is what lets
the writer finish in private before anyone can look at it.

### The simulation paces itself

Nothing tells the simulation thread when to tick. It works that out alone:

1. Check the clock and work out when the next tick is due.
2. Sleep until then.
3. Run every tick that is now due, publishing a snapshot after each one.
4. Repeat.

Catch-up is capped at five ticks. If the thread wakes up a long way behind, say
because the machine was suspended, it runs five ticks and then resets its
schedule instead of trying to replay the missing minutes.

The effect is that a stall makes the simulation **run slow** rather than
freeze and then fast-forward.

### What this means in practice

The two threads are never in step, so both of these happen constantly, and both
are normal:

- **The renderer reads the same snapshot more than once.** At 144 Hz against a
  60 Hz simulation it asks more often than new snapshots appear. That is what
  the next section is about.
- **The renderer never sees some snapshots.** If a frame takes a long time, the
  simulation keeps ticking and overwrites snapshots that were never read. They
  are gone, and that is correct, because the renderer only ever wants the
  newest one.

---

## 4. Why rendering the latest state isn't enough

Suppose the simulation produces:

```text
Tick 100 → State A
Tick 101 → State B
Tick 102 → State C
```

At 60 Hz, there are only 60 new states every second.

But a 144 Hz renderer needs to produce 144 visual frames every second.

If Godot simply renders the **latest simulation state**, it might display:

```text
State A
State A
State B
State B
State B
State C
State C
...
```

The player's character therefore appears to stay in one position for a few
frames and then suddenly jump to the next position.

The simulation itself is perfectly correct, but the **visual motion is uneven**.

---

## 5. Interpolation

This is where interpolation comes in.

The renderer intentionally stays **one simulation tick behind**.

That gives it two states to work with.

For example:

```text
Tick 100 ─────────────── Tick 101
State A                   State B
```

Instead of immediately displaying State B, the renderer smoothly moves from A
toward B.

If the renderer has several frames to display between those ticks, it can
calculate intermediate positions.

For example:

```text
A → A₁ → A₂ → A₃ → B
```

It can use linear interpolation (`lerp`) to calculate those intermediate values.

The renderer does **not predict the future**.

It already has both states:

```text
previous state
next state
```

It simply calculates where something should visually appear **between those two
known states**.

---

## 6. Why the renderer stays one tick behind

The renderer needs two states to interpolate.

If it only had:

```text
Tick 100
```

it couldn't interpolate toward Tick 101 because Tick 101 doesn't exist yet.

So it deliberately waits until it has:

```text
Tick 100
Tick 101
```

Then it can smoothly render the visual transition between them.

This introduces **one tick of latency**, approximately:

**16.67 ms at 60 Hz.**

The important thing is that this latency is **constant**, rather than constantly
changing.

---

## 7. The tradeoff: latency vs. prediction

Rendering one tick behind means the renderer is always showing the **recent
past**, not the exact present.

To interpolate smoothly toward a position, the renderer must already have that
position available.

That means the newest thing it can blend toward is the latest tick that has
already been produced by the simulation.

At 60 Hz, this adds about:

**16.67 ms of visual latency.**

For a single-player action RPG, that is usually a very small and acceptable
tradeoff, because the motion stays smooth and stable.

Some competitive games choose a different approach called **extrapolation**.

Instead of blending between two known states, the renderer tries to **guess
forward** from the latest state it has.

For example:

```text
guess = current_position + velocity × time_since_tick
```

The advantage is that this can reduce or even eliminate the one-tick visual
delay.

But the downside is that the renderer is now **predicting**, not displaying
confirmed simulation data.

If the character suddenly stops, hits a wall, or changes direction, the guess
can be wrong.

When the real next tick arrives, the renderer has to correct itself, which can
cause a visible snap or rubber-banding effect.

So the tradeoff is:

- **Interpolation** = small, constant latency, but smooth and reliable motion.
- **Extrapolation** = less latency, but more risk of visible corrections.

In practice:

- **Interpolation** is usually better for games that prioritize stable, readable
  motion, such as single-player action games, ARPGs, platformers, and many
  third-person games.
- **Extrapolation** is more attractive in games where shaving off every bit of
  input/display delay matters, such as competitive shooters or other highly
  latency-sensitive multiplayer games.

---

## 8. What happens if the simulation falls behind?

Suppose the renderer is currently interpolating:

```text
Tick 100 → Tick 101
```

and the simulation is supposed to produce Tick 102, but Tick 102 is taking too
long.

Eventually the renderer finishes its transition from 100 to 101.

Now it needs:

```text
Tick 101 → Tick 102
```

but Tick 102 isn't available yet.

There is nothing to interpolate toward.

The renderer cannot invent the actual future simulation state.

Therefore, it has to keep displaying the most recent information it has, which
can result in a **stutter or freeze**.

Interpolation can smooth out the normal difference between simulation frequency
and rendering frequency, but it **cannot hide a simulation that is genuinely too
slow to keep up**.

---

## The whole system in one flow

```text
PLAYER INPUT
     ↓
RUST SIMULATION
     ↓
60 fixed ticks/sec
     ↓
Tick 100 → State 100
Tick 101 → State 101
Tick 102 → State 102
     ↓
GODOT RENDERER
     ↓
Keeps two simulation states
     ↓
Interpolates between them
     ↓
144 visual frames/sec
     ↓
SMOOTH MOTION ON SCREEN
```

The fundamental separation is:

> **Rust simulation = determines what is actually happening.**
>
> **Godot renderer = determines how that state is visually displayed.**

The simulation advances the world in **fixed 60 Hz steps**. The renderer can run
at whatever frame rate the hardware supports and uses **interpolation between
known simulation states** to make the motion appear smooth.
