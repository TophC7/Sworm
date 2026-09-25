#!/usr/bin/env fish

set -l repo_root (status dirname)/../..
cd $repo_root

set -l config_dir .sworm/dev-remote/config
set -l data_dir .sworm/dev-remote/data
set -l server_toml $config_dir/server.toml

mkdir -p $config_dir $data_dir

if not test -f $server_toml; or not grep -qF '[web]' $server_toml
    printf '\n[web]\nbind = "127.0.0.1:7421"\nassets_dir = "../../../build-web"\n' >> $server_toml
end

if not test -f build-web/index.html
    bun run build:web; or exit 1
end

cargo build -p sworm-server; or exit 1

setpriv --pdeathsig TERM ./target/debug/sworm-server \
    --config-dir $config_dir \
    --data-dir $data_dir \
    serve --listen 127.0.0.1:7420 &

exec bun app:dev
