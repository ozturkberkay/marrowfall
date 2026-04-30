#!/usr/bin/env bash
# Tests for utils.sh

set_up_before_script() {
  # shellcheck source=scripts/src/utils.sh
  source "scripts/src/utils.sh"
}

set_up() {
  TEST_DIR=$(mktemp -d)
}

tear_down() {
  rm -rf "$TEST_DIR"
}

# --- upsert_config_block ---

function test_upsert_creates_file_and_parent_dirs() {
  local file="${TEST_DIR}/nested/dir/config"
  upsert_config_block "$file" "key = value"

  assert_file_exists "$file"
}

function test_upsert_wraps_content_with_markers() {
  local file="${TEST_DIR}/config"
  upsert_config_block "$file" "hello"

  assert_file_contains "$file" "# Marrowfall START"
  assert_file_contains "$file" "hello"
  assert_file_contains "$file" "# Marrowfall END"
}

function test_upsert_replaces_existing_block() {
  local file="${TEST_DIR}/config"
  upsert_config_block "$file" "old content"
  upsert_config_block "$file" "new content"

  assert_file_contains "$file" "new content"
  assert_file_not_contains "$file" "old content"
}

function test_upsert_preserves_content_outside_markers() {
  local file="${TEST_DIR}/config"
  echo "before" >"$file"
  upsert_config_block "$file" "managed"
  echo "after" >>"$file"

  # Re-upsert — "before" and "after" must survive
  upsert_config_block "$file" "managed v2"

  assert_file_contains "$file" "before"
  assert_file_contains "$file" "after"
  assert_file_contains "$file" "managed v2"
}

function test_upsert_produces_single_marker_pair() {
  local file="${TEST_DIR}/config"
  upsert_config_block "$file" "first"
  upsert_config_block "$file" "second"
  upsert_config_block "$file" "third"

  local count
  count=$(grep -c "# Marrowfall START" "$file")
  assert_equals "1" "$count"
}

function test_upsert_into_empty_file() {
  local file="${TEST_DIR}/config"
  touch "$file"
  upsert_config_block "$file" "content"

  assert_file_contains "$file" "# Marrowfall START"
  assert_file_contains "$file" "content"
  assert_file_contains "$file" "# Marrowfall END"
}

function test_upsert_multiline_content() {
  local file="${TEST_DIR}/config"
  upsert_config_block "$file" "line1
line2
line3"

  assert_file_contains "$file" "line1"
  assert_file_contains "$file" "line2"
  assert_file_contains "$file" "line3"
}

function test_upsert_no_leftover_bak_file() {
  local file="${TEST_DIR}/config"
  upsert_config_block "$file" "content"

  assert_file_not_exists "${file}.bak"
}

function test_upsert_preserves_trailing_newline_of_existing_content() {
  local file="${TEST_DIR}/config"
  printf "existing" >"$file" # no trailing newline
  upsert_config_block "$file" "managed"

  # "existing" and the START marker must be on separate lines
  local first_two
  first_two=$(head -2 "$file")
  assert_contains "existing" "$first_two"
  assert_contains "# Marrowfall START" "$first_two"
}
