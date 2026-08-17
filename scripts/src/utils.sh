#!/usr/bin/env bash

# Idempotently upsert a managed block in a config file. The block is delimited
# by "# Marrowfall START" / "# Marrowfall END" markers. Re-running
# replaces the previous block.
function upsert_config_block() {
  local file="$1"
  local content="$2"
  local start_marker="# Marrowfall START"
  local end_marker="# Marrowfall END"

  mkdir -p "$(dirname "$file")"
  touch "$file"

  sed -i.bak "/$start_marker/,/$end_marker/d" "$file"

  if [ -s "$file" ]; then
    tail -c1 "$file" | read -r _ || echo >> "$file"
  fi

  {
    echo "$start_marker"
    echo "$content"
    echo "$end_marker"
  } >> "$file"

  rm -f "${file}.bak"
}
