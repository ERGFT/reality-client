#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
tmp_parent="$(cd -- "${TMPDIR:-/tmp}" && pwd)"
test_root="$(mktemp -d "$tmp_parent/reality-client-installer-smoke.XXXXXXXX")"
cleanup() {
    case "$test_root" in
        "$tmp_parent"/reality-client-installer-smoke.*) rm -rf -- "$test_root" ;;
        *) echo "Refusing to remove unexpected temporary path: $test_root" >&2; return 1 ;;
    esac
}
trap cleanup EXIT

package_dir="$test_root/package with spaces"
home_dir="$test_root/home with spaces"
data_home="$test_root/app data with spaces"
mkdir -p "$package_dir" "$home_dir"
for file in RealityClient reality-client libreality.so LICENSE.txt README.md; do
    printf 'smoke fixture: %s\n' "$file" > "$package_dir/$file"
done
cp "$script_dir/../install-linux.sh" "$package_dir/install-linux.sh"

HOME="$home_dir" XDG_DATA_HOME="$data_home" \
    bash "$package_dir/install-linux.sh"

app_dir="$home_dir/.local/opt/reality-client"
desktop_file="$data_home/applications/reality-client.desktop"
for file in RealityClient reality-client libreality.so LICENSE.txt README.md; do
    cmp "$package_dir/$file" "$app_dir/$file"
done
if [[ "${OSTYPE:-}" != msys* && "${OSTYPE:-}" != cygwin* ]]; then
    [[ -x "$app_dir/RealityClient" ]]
    [[ -x "$app_dir/reality-client" ]]
fi
[[ -f "$desktop_file" ]]
expected_exec="$app_dir/RealityClient"
expected_exec="${expected_exec//\\/\\\\}"
expected_exec="${expected_exec//\"/\\\"}"
expected_exec="${expected_exec//%/%%}"
grep -Fxq "Exec=\"$expected_exec\"" "$desktop_file"

printf 'user-owned file\n' > "$app_dir/user-note.txt"
for file in RealityClient reality-client libreality.so LICENSE.txt README.md; do
    printf 'updated smoke fixture: %s\n' "$file" > "$package_dir/$file"
done
HOME="$home_dir" XDG_DATA_HOME="$data_home" \
    bash "$package_dir/install-linux.sh" >/dev/null
for file in RealityClient reality-client libreality.so LICENSE.txt README.md; do
    cmp "$package_dir/$file" "$app_dir/$file"
done
[[ "$(cat "$app_dir/user-note.txt")" == 'user-owned file' ]]

bad_package="$test_root/incomplete package"
bad_home="$test_root/unused home"
mkdir -p "$bad_package" "$bad_home"
cp "$script_dir/../install-linux.sh" "$bad_package/install-linux.sh"
if HOME="$bad_home" XDG_DATA_HOME="$bad_home/data" \
    bash "$bad_package/install-linux.sh" >/dev/null 2>&1; then
    echo 'Installer unexpectedly accepted an incomplete package.' >&2
    exit 1
fi
[[ ! -e "$bad_home/.local/opt/reality-client" ]]

if HOME='' XDG_DATA_HOME="$test_root/empty-home-data" \
    bash "$package_dir/install-linux.sh" >/dev/null 2>&1; then
    echo 'Installer unexpectedly accepted an empty HOME.' >&2
    exit 1
fi
if HOME='relative-home' XDG_DATA_HOME="$test_root/relative-home-data" \
    bash "$package_dir/install-linux.sh" >/dev/null 2>&1; then
    echo 'Installer unexpectedly accepted a relative HOME.' >&2
    exit 1
fi
[[ ! -e "$test_root/empty-home-data/applications/reality-client.desktop" ]]
[[ ! -e "$test_root/relative-home-data/applications/reality-client.desktop" ]]

echo 'LINUX_INSTALLER_SMOKE=PASS'
