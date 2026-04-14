#!/usr/bin/env bash
# Auto-formats staged files before git commit.
# Used as a Claude Code PreToolUse hook on Bash commands containing "git commit".
# Runs formatters, re-stages changed files, and lets the commit proceed.

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

# Format frontend files and re-stage
FRONTEND_FILES=$(echo "$STAGED" | grep -E '\.(ts|js|svelte|css|json|html)$' || true)
if [ -n "$FRONTEND_FILES" ]; then
  echo "$FRONTEND_FILES" | xargs bun prettier --write --ignore-unknown 2>/dev/null
  echo "$FRONTEND_FILES" | xargs git add 2>/dev/null
fi

# Format rust files and re-stage
RUST_FILES=$(echo "$STAGED" | grep -E '\.rs$' || true)
if [ -n "$RUST_FILES" ]; then
  (cd src-tauri && cargo fmt 2>/dev/null)
  echo "$RUST_FILES" | xargs git add 2>/dev/null
fi

exit 0
