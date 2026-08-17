#!/usr/bin/env bash
# PreToolUse hook for Bash: enforce `prek run` for pre-commit checks

COMMAND=$(jq -r '.tool_input.command')

if echo "$COMMAND" | grep -qE '(prek( run)? --all-files|pre-commit run)'; then
  jq -n '{
    "hookSpecificOutput": {
      "hookEventName": "PreToolUse",
      "permissionDecision": "deny",
      "permissionDecisionReason": "Do not use `pre-commit run` or `prek --all-files`. Instead, `git add` your changes and execute `prek run --verbose > /tmp/prek.log 2>&1`."
    }
  }'
else
  exit 0
fi
