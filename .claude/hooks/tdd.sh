#!/usr/bin/env bash
jq -n '{
  "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "allow",
    "additionalContext": "REMINDER: TDD requires tests before implementation. Ensure you have failing tests first."
  }
}'
exit 0
