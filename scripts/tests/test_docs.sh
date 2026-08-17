#!/usr/bin/env bash
# Tests for docs.sh

set_up_before_script() {
  # shellcheck source=scripts/src/docs.sh
  source "scripts/src/docs.sh"
}

set_up() {
  TEST_DIR=$(mktemp -d)
  cd "$TEST_DIR" || exit 1

  mkdir -p docs/templates
  echo "# Design: {{name}}" >docs/templates/design.md
}

tear_down() {
  cd /
  rm -rf "$TEST_DIR"
}

function test_create_doc_missing_args_returns_error() {
  local output
  output=$(create_doc 2>&1)
  assert_exit_code "1"
  assert_contains "Usage:" "$output"
}

function test_create_doc_missing_name_returns_error() {
  local output
  output=$(create_doc brief 2>&1)
  assert_exit_code "1"
  assert_contains "Usage:" "$output"
}

function test_create_doc_invalid_type_returns_error() {
  local output
  output=$(create_doc invalid test 2>&1)
  assert_exit_code "1"
  assert_contains "Unknown type" "$output"
}

function test_create_doc_replaces_name_placeholder() {
  create_doc design test_project

  local expected_date
  expected_date=$(date +%Y_%m_%d)
  local expected_file="docs/design/${expected_date}_test_project.md"

  assert_file_contains "$expected_file" "# Design: Test Project"
  assert_not_contains "{{name}}" "$(cat "$expected_file")"
}

function test_create_doc_design_creates_file() {
  create_doc design api_v2
  assert_exit_code "0"

  local expected_date
  expected_date=$(date +%Y_%m_%d)
  local expected_file="docs/design/${expected_date}_api_v2.md"

  assert_file_exists "$expected_file"
}

function test_create_doc_rejects_kebab_case() {
  local output
  output=$(create_doc design my-feature 2>&1)
  assert_exit_code "1"
  assert_contains "snake_case" "$output"
}
