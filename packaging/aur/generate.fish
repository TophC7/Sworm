#!/usr/bin/env fish

if test (count $argv) -ne 3
    echo 'Usage: fish packaging/aur/generate.fish <package-version> <deb-directory> <output-directory>' >&2
    exit 1
end

set -l app_version $argv[1]
if not string match --quiet --regex '^0\.0\.(0|[1-9][0-9]*)\+[0-9a-f]{12}$' -- "$app_version"
    echo "Expected an automatic release version (0.0.<build>+<12-character commit>), got: $app_version" >&2
    exit 1
end

set -l commit_hash (string split -f 2 + -- "$app_version")
set -l script_dir (path resolve (status dirname))
set -l deb_dir $argv[2]
set -l output_dir $argv[3]
set -l amd64_deb "$deb_dir"/sworm_"$app_version"_amd64.deb
set -l arm64_deb "$deb_dir"/sworm_"$app_version"_arm64.deb
for deb in "$amd64_deb" "$arm64_deb"
    if not test -f "$deb"
        echo "Missing release artifact: $deb" >&2
        exit 1
    end
end

set -l amd64_sum_line (sha256sum -- "$amd64_deb")
or exit 1
set -l arm64_sum_line (sha256sum -- "$arm64_deb")
or exit 1
set -l amd64_sha (string split -f 1 ' ' -- "$amd64_sum_line")
set -l arm64_sha (string split -f 1 ' ' -- "$arm64_sum_line")

mkdir -p -- "$output_dir"
or exit 1
string replace --all '@VERSION@' "$app_version" <"$script_dir/PKGBUILD.in" |
    string replace --all '@COMMIT@' "$commit_hash" |
    string replace --all '@AMD64_SHA256@' "$amd64_sha" |
    string replace --all '@ARM64_SHA256@' "$arm64_sha" >"$output_dir/PKGBUILD"
