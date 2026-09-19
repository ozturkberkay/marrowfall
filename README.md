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
- Four transports cross that boundary. Two latest-wins triple buffers carry
  held input in and snapshots out, because only the newest of either is ever
  wanted, and the input buffer is what keeps walking speed independent of frame
  rate. Two crossbeam channels carry commands in and generated chunks out, where
  every message must arrive: a dropped chunk is a permanent hole in the map.

### Monorepo Layout

```text
.
├── crates/
│   ├── game/                 # Pure Rust simulation
│   ├── host/                 # Sim runner (thread + channels)
│   ├── render/               # Godot frontend (gdext)
│   ├── sprites/              # Sprite manifest format (pipeline writes, game reads)
│   ├── worldgen/             # World generation
│   ├── xtask-art/            # The character art pipeline
│   └── xtask-world/          # World preview tool
├── project/                  # Godot project, including `data/` tuning tables
├── art/                      # Concepts, sprites, animations, skeletons, goldens, branding
├── tools/                    # Blender scripts, and the glTF-Validator driver
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

### Testing

Three-tier test architecture. Every crate keeps its tests in a sibling
`tests/` directory, never next to source files, which means a test only ever
sees that crate's public API. Each tier is its own Cargo test target, and that
is what lets one command run a whole tier across the workspace:

| Tier | Dependencies | Command | Wired in |
| ----------- | ------------------------------------- | -------------------------------------------------- | --------------------------- |
| Unit | Mocks and local stubs, no remote service | `cargo nextest run --workspace --test unit` | `game`, `host`, `render`, `sprites`, `worldgen`, `xtask-art`, `xtask-world` |
| Integration | Real threads and channels, no engine | `cargo nextest run --workspace --test integration` | `host` |
| E2E | Black box, launches `godot --headless` | `cargo nextest run --workspace --test e2e` | `render` |

#### Looking at the character

Tests prove the game runs. They cannot tell you the character looks right.
This opens the game, poses him through every animation, and saves the frames
to `art/preview/e2e/` so a person (or an AI) can look at them:

```bash
MARROWFALL_VISUAL_HARNESS=1 cargo nextest run --workspace --test e2e
```

Local only, because a headless Godot draws no pixels.
