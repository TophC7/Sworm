#!/usr/bin/env fish
# Run only in a disposable container, under dbus-run-session and xvfb-run.

set -gx HOME (mktemp -d)
or exit 1
set -gx XDG_CONFIG_HOME "$HOME/.config"
set -gx XDG_DATA_HOME "$HOME/.local/share"
set -gx XDG_CACHE_HOME "$HOME/.cache"
set -gx GDK_BACKEND x11
set -l log "$HOME/sworm.log"

git init --quiet "$HOME/project"
or exit 1
sworm "$HOME/project" >"$log" 2>&1 &
set -l app_pid $last_pid

timeout 30s xdotool search --sync --onlyvisible --pid $app_pid
set -l result $status
if test $result -eq 0
    sleep 3
    kill -0 $app_pid
    set result $status
end

kill $app_pid 2>/dev/null
wait $app_pid 2>/dev/null
cat "$log"
exit $result
