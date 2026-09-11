#!/bin/sh
set -e

# Run only in a disposable container/VM, under dbus-run-session and xvfb-run.
HOME="$(mktemp -d)"
export HOME
export XDG_CONFIG_HOME="$HOME/.config"
export XDG_DATA_HOME="$HOME/.local/share"
export XDG_CACHE_HOME="$HOME/.cache"
export GDK_BACKEND="x11"
export WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS="1"
LOG="$HOME/sworm.log"

git init --quiet "$HOME/project"
sworm "$HOME/project" >"$LOG" 2>&1 &
APP_PID=$!

if timeout 30s xdotool search --sync --onlyvisible --pid "$APP_PID" >/dev/null 2>&1; then
  sleep 3
  if kill -0 "$APP_PID" 2>/dev/null; then
    RESULT=0
  else
    RESULT=1
  fi
else
  RESULT=1
fi

kill "$APP_PID" 2>/dev/null || true
wait "$APP_PID" 2>/dev/null || true
cat "$LOG"
exit "$RESULT"
