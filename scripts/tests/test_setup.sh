#!/usr/bin/env bash
# Tests for setup.sh

set_up_before_script() {
  # shellcheck source=scripts/src/utils.sh
  source "scripts/src/utils.sh"
  # shellcheck source=scripts/src/setup.sh
  source "scripts/src/setup.sh"
}

set_up() {
  TEST_DIR=$(mktemp -d)
  MARROWFALL_ORIGINAL_HOME="$HOME"
  export HOME="$TEST_DIR"
  MARROWFALL_MOCK_LOG=""

  # Mock all external installers
  brew() { MARROWFALL_MOCK_LOG+="brew $*;"; }
  export -f brew
  curl() { echo "mock-curl"; }
  export -f curl
  cargo() { MARROWFALL_MOCK_LOG+="cargo $*;"; }
  export -f cargo
  prek() { MARROWFALL_MOCK_LOG+="prek $*;"; }
  export -f prek
}

tear_down() {
  export HOME="$MARROWFALL_ORIGINAL_HOME"
  rm -rf "$TEST_DIR"
  unset MARROWFALL_MOCK_LOG MARROWFALL_ORIGINAL_HOME
}

# --- setup_brew ---

function test_setup_brew_calls_brew_bundle() {
  setup_brew >/dev/null 2>&1
  assert_contains "brew bundle install --file=Brewfile" "$MARROWFALL_MOCK_LOG"
}

# --- setup_precommit ---

function test_setup_precommit_calls_prek_install() {
  setup_precommit >/dev/null 2>&1
  assert_contains "prek install" "$MARROWFALL_MOCK_LOG"
}
