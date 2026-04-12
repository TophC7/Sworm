#!/usr/bin/env bash
# Blocks git commit if staged files aren't formatted.
# Used as a Claude Code PreToolUse hook on Bash commands containing "git commit".

INPUT=$(cat)
COMMAND=$(echo "$INPUT" | jq -r '.tool_input.command // empty')

# Only intercept git commit commands
if ! echo "$COMMAND" | grep -qE '\bgit\s+commit\b'; then
  exit 0
fi

STAGED=$(git diff --cached --name-only --diff-filter=ACMR 2>/dev/null)

if [ -z "$STAGED" ]; then
  exit 0
fi

ERRORS=""

# Check frontend formatting
FRONTEND_FILES=$(echo "$STAGED" | grep -E '\.(ts|js|svelte|css|json|html)$' || true)
if [ -n "$FRONTEND_FILES" ]; then
  UNFORMATTED=$(echo "$FRONTEND_FILES" | xargs bun prettier --check --ignore-unknown 2>&1 || true)
  if echo "$UNFORMATTED" | grep -q "Code style issues"; then
    ERRORS="${ERRORS}\n[BLOCKED] Prettier found unformatted files. Run: bun prettier --write on staged files."
  fi
fi

# Check rust formatting
RUST_FILES=$(echo "$STAGED" | grep -E '\.rs$' || true)
if [ -n "$RUST_FILES" ]; then
  FMT_CHECK=$(cd src-tauri && cargo fmt --check 2>&1 || true)
  if [ -n "$FMT_CHECK" ]; then
    ERRORS="${ERRORS}\n[BLOCKED] cargo fmt found unformatted Rust files. Run: cd src-tauri && cargo fmt"
  fi
fi

if [ -n "$ERRORS" ]; then
  echo -e "$ERRORS" >&2
  exit 2
fi

exit 0
