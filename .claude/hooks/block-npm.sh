#!/usr/bin/env bash
# blocks npm/pnpm/node commands -- enforces bun-only

INPUT=$(cat)
COMMAND=$(echo "$INPUT" | jq -r '.tool_input.command // empty')

if echo "$COMMAND" | grep -qE '^\s*(npm|npx|pnpm|pnpx|node)\s'; then
  echo "Blocked: use bun instead of npm/pnpm/node" >&2
  exit 2
fi

exit 0
