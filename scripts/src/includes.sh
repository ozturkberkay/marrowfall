#!/usr/bin/env bash
# All the functions assume the working directory is the project root!

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"

# shellcheck source=scripts/src/setup.sh
source "${SCRIPT_DIR}/setup.sh"

# shellcheck source=scripts/src/utils.sh
source "${SCRIPT_DIR}/utils.sh"

# shellcheck source=scripts/src/docs.sh
source "${SCRIPT_DIR}/docs.sh"
