#!/usr/bin/env bash

function create_doc() {
  local type="$1"
  local name="$2"

  if [[ -z "$type" || -z "$name" ]]; then
    echo "Usage: create_doc <design> <name>" >&2
    return 1
  fi

  if [[ "$name" =~ - ]]; then
    echo "Error: name must be snake_case (no hyphens). Got: $name" >&2
    return 1
  fi

  case "$type" in
    design) ;;
    *)
      echo "Unknown type: $type (expected: design)" >&2
      return 1
      ;;
  esac

  local template="docs/templates/${type}.md"
  local target_dir="docs/${type}"
  local date_prefix
  date_prefix=$(date +%Y_%m_%d)
  local filename="${date_prefix}_${name}.md"

  # Convert snake_case to Title Case for the document heading
  local title
  title=$(echo "$name" | tr '_' ' ' | awk '{for(i=1;i<=NF;i++) $i=toupper(substr($i,1,1)) substr($i,2)}1')

  mkdir -p "$target_dir"
  sed "s/{{name}}/${title}/g" "$template" >"${target_dir}/${filename}"
  echo "Created ${target_dir}/${filename}"
}
