#!/usr/bin/env bash
# Line coverage for the Rust workspace, over the library and the unit tests.
# Requires: cargo-llvm-cov and cargo-nextest. See the setup command below.

set -euo pipefail

# `cargo install` puts binaries in ~/.cargo/bin. The rustup.rs installer adds
# that to PATH via ~/.cargo/env, but Homebrew's rustup writes no such file, and
# git hooks run with a trimmed environment either way, so add it directly.
export PATH="${CARGO_HOME:-${HOME}/.cargo}/bin:${PATH}"

REQUIRED_COVERAGE=96

# What is left uncovered is almost all OS calls failing (a write returning
# Err). Reaching those needs a fake filesystem, or production API added purely
# to be tested. `crates/game` is at 100% and should stay there. Raise this
# number when the game grows, not by faking either.
#
# Two files cannot be reached from a plain test binary:
#
#   render/src/bridge.rs  a Godot node. Instantiating it needs a running
#                         engine, so it is exercised by launching the game,
#                         not by the test harness.
#   xtask-art/src/main.rs the binary entry point, which exists only to call
#                         `cli::run_from_args`. That function IS covered.
#
# Everything else is measured. Excluding anything further needs a reason as
# good as these two.
IGNORE='(render/src/bridge\.rs|xtask-art/src/main\.rs)'

for tool in cargo-llvm-cov cargo-nextest; do
  if ! command -v "${tool}" &> /dev/null; then
    echo "Error: ${tool} is not installed. Set up this repo with:"
    echo "  source scripts/src/includes.sh && setup"
    exit 1
  fi
done

# `--no-clean` is the difference between seconds and minutes. Without it
# cargo-llvm-cov wipes its build directory on every run, so all ~200
# dependencies recompile from scratch, at the opt-level the dev profile sets.
# Instrumented artifacts live in their own target directory anyway, so there is
# nothing to mix them up with.
#
# The catch: moving or deleting a source file leaves its stale object behind,
# and it reappears in the report at 0%, dragging the total under the gate. When
# that happens, `cargo llvm-cov clean --workspace` once. It costs seconds.
#
# `llvm-tools-preview` comes from rust-toolchain.toml, so the pinned stable
# toolchain is enough, no nightly needed.
cargo llvm-cov --no-clean nextest \
  --workspace \
  --lib --test unit \
  --ignore-filename-regex "${IGNORE}" \
  --fail-under-lines "${REQUIRED_COVERAGE}"
