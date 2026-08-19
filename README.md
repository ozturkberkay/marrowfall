<!-- markdownlint-disable MD033 MD041 -->
<p align="center">
  <img src="art/brand/logo.png" alt="Marrowfall" width="720">
</p>
<!-- markdownlint-enable MD033 MD041 -->

A single-player isometric action-RPG sandbox set in a dying medieval world.

## Architecture

### Summary

- Built as a headless, engine-agnostic, deterministic Rust simulation with
  Godot as a thin rendering/input frontend.
- The sim runs on a **dedicated thread** owned by `crates/host`.
- Three transports cross that boundary: a crossbeam channel carries commands in
  (every message must arrive), and two latest-wins triple buffers carry held
  input in and snapshots out (only the newest of either is ever wanted). The
  input buffer is what keeps walking speed independent of frame rate.

### Monorepo Layout

```text
.
├── crates/
│   ├── game/                 # Pure Rust simulation
│   ├── host/                 # Sim runner (thread + channels)
│   ├── render/               # Godot frontend (gdext)
│   ├── sprites/              # Sprite manifest format (pipeline writes, game reads)
│   └── xtask-art/            # The character art pipeline
├── project/                  # Godot project
├── art/                      # Concepts, sprites, animations, branding
├── scripts/                  # Shell scripts and git hooks
├── docs/                     # Design docs, blog posts etc.
└── Cargo.toml                # Rust workspace root
```

## Local Development

### Setup

1. Install the pre-requisites and setup the game:

    ```bash
    source scripts/src/includes.sh
    setup
    ```

2. Run the game:

    ```bash
    godot --path project
    ```

    WASD moves the survivor. The keys are bound by physical location, so they
    stay in the same place on a non-QWERTY layout.

### Testing

Three-tier test architecture. Every crate keeps its tests in a sibling
`tests/` directory, never next to source files, which means a test only ever
sees that crate's public API. Each tier is its own Cargo test target, and that
is what lets one command run a whole tier across the workspace:

| Tier | Dependencies | Command | Wired in |
| ----------- | ------------------------------------- | -------------------------------------------------- | --------------------------- |
| Unit | Mocks only, zero I/O | `cargo nextest run --workspace --test unit` | `game`, `host`, `render`, `sprites`, `xtask-art` |
| Integration | Real threads and channels, no engine | `cargo nextest run --workspace --test integration` | nothing yet |
| E2E | Black box, launches `godot --headless` | `cargo nextest run --workspace --test e2e` | nothing yet |

> **gdext gotcha:** after recompiling Rust, restart the Godot editor, the
> running editor does not reliably pick up a rebuilt library.
