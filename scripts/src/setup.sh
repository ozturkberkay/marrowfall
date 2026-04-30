#!/usr/bin/env bash

# Brew

function setup_brew() {
  echo "🍺 Installing Homebrew."
  /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/dc89d02c0107d688e089c1683dcda3401719f1f8/install.sh)"
  echo "🍺 Installing formulas from the bundle."
  brew bundle install --file=Brewfile
}

# Git

function setup_precommit() {
  echo "🔧 Installing pre-commit hooks."
  prek install
}

function setup_git_lfs() {
  echo "🔧 Setting up Git LFS."
  git lfs install
  git lfs pull
}

# Rust

function setup_rust() {
  echo "🦀 Installing the Rust toolchain."
  # Version and components come from `rust-toolchain.toml`.
  rustup show active-toolchain || rustup toolchain install
  echo "🦀 Installing cargo tools."
  # These land in ~/.cargo/bin, which Homebrew's rustup does not add to PATH.
  cargo install --locked cargo-deny cargo-llvm-cov
}

# Python

function setup_python() {
  echo "🐍 Setting up the Python environment."
  uv sync
}

# Complete

function setup() {
  setup_brew
  setup_precommit
  setup_git_lfs
  setup_rust
  setup_python
}
