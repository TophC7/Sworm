#!/usr/bin/env fish

# Dev desktop + local daemon, both sandboxed under .sworm/dev-remote and
# trusting each other up front, so no pairing step and no ~/.config remotes.

set -l repo_root (realpath (status dirname)/../..)
cd $repo_root

set -l root $repo_root/.sworm/dev-remote
set -l server_config_dir $root/config
set -l server_data_dir $root/data
# The desktop reads settings and client.pem from $XDG_CONFIG_HOME/sworm and
# keeps its DB under $XDG_DATA_HOME; child processes never inherit either.
set -l desktop_config_home $root/desktop/config
set -l desktop_data_home $root/desktop/data
set -l client_key $desktop_config_home/sworm/client.pem
set -l desktop_settings $desktop_config_home/sworm/settings.jsonc

mkdir -p $server_config_dir $server_data_dir $desktop_config_home/sworm $desktop_data_home

# Always rebuild: a stale bundle speaks an old wire protocol and hangs on connect.
bun run build:web; or exit 1

cargo build -p sworm-server; or exit 1
set -l server ./target/debug/sworm-server

if not test -f $client_key
    $server keygen $client_key >/dev/null; or exit 1
end
$server fingerprint $client_key >$root/authorized_keys; or exit 1

printf '{
  "authorized_keys_file": "%s",
  "web": { "listen": "127.0.0.1:7421", "assets_dir": "%s" }
}\n' $root/authorized_keys $repo_root/build-web >$server_config_dir/server.jsonc

# Written once: later edits made from the dev desktop's settings survive.
if not test -f $desktop_settings
    set -l server_fingerprint ($server --config-dir $server_config_dir fingerprint); or exit 1
    printf '{
  "remotes": { "dev": { "address": "127.0.0.1:7420", "fingerprint": "%s" } }
}\n' $server_fingerprint >$desktop_settings
end

setpriv --pdeathsig TERM $server \
    --config-dir $server_config_dir \
    --data-dir $server_data_dir \
    serve --listen 127.0.0.1:7420 &

XDG_CONFIG_HOME=$desktop_config_home XDG_DATA_HOME=$desktop_data_home exec bun app:dev
