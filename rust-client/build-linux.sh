#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
archive="$repo_root/third_party/vpn-core-source.zip"
revision_file="$repo_root/third_party/vpn-core-source.commit"
expected_commit='ee68039943ebb2aaf3287bf622ae34c18bfa0cae'
expected_sha256='DF789EABC39029A403D72BA78367637E4347D372626A5FC79F35EC901AF1317D'

[[ -f "$archive" ]] || { echo "Missing pinned core source archive: $archive" >&2; exit 1; }
[[ "$(tr -d '\r\n' < "$revision_file")" == "$expected_commit" ]] || {
    echo 'Pinned core revision does not match the expected commit.' >&2
    exit 1
}
actual_sha256="$(sha256sum "$archive" | cut -d ' ' -f 1 | tr '[:lower:]' '[:upper:]')"
[[ "$actual_sha256" == "$expected_sha256" ]] || {
    echo 'Pinned core source archive SHA-256 check failed.' >&2
    exit 1
}

tmp_parent="$(cd -- "${TMPDIR:-/tmp}" && pwd)"
build_root="$(mktemp -d "$tmp_parent/reality-client-linux.XXXXXXXX")"
cleanup() {
    case "$build_root" in
        "$tmp_parent"/reality-client-linux.*) rm -rf -- "$build_root" ;;
        *) echo "Refusing to remove unexpected temporary path: $build_root" >&2; return 1 ;;
    esac
}
trap cleanup EXIT

source_dir="$build_root/core-source"
mkdir -p "$source_dir"
unzip -q "$archive" -d "$source_dir"
[[ -f "$source_dir/Cargo.toml" ]] || {
    echo 'Pinned archive does not contain the expected workspace Cargo.toml.' >&2
    exit 1
}
python3 "$script_dir/patches/apply_core_tun_fd_ownership.py" "$source_dir"

export CARGO_TARGET_DIR="$build_root/target"
(
    cd "$source_dir"
    cargo test --locked -p reality-ffi --lib \
        tun_fd_ownership_tests::rc_start_closes_system_tun_fd_when_config_parse_fails
)
cargo build --locked --release --manifest-path "$source_dir/Cargo.toml" -p reality-ffi -p reality-client
cargo build --locked --release --manifest-path "$script_dir/Cargo.toml"

package_dir="$script_dir/dist/linux-x86_64"
mkdir -p "$package_dir"
install -m 0755 "$CARGO_TARGET_DIR/release/reality-client-rs" "$package_dir/RealityClient"
install -m 0755 "$CARGO_TARGET_DIR/release/reality-client" "$package_dir/reality-client"
install -m 0644 "$CARGO_TARGET_DIR/release/libreality.so" "$package_dir/libreality.so"
install -m 0644 "$repo_root/LICENSE.txt" "$package_dir/LICENSE.txt"
install -m 0644 "$script_dir/README.md" "$package_dir/README.md"
install -m 0755 "$script_dir/install-linux.sh" "$package_dir/install-linux.sh"

cat > "$package_dir/reality-client.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=Reality Client
Comment=VLESS/REALITY client
Terminal=false
Categories=Network;Security;
DESKTOP
desktop_exec="$package_dir/RealityClient"
desktop_exec="${desktop_exec//\\/\\\\}"
desktop_exec="${desktop_exec//\"/\\\"}"
desktop_exec="${desktop_exec//%/%%}"
printf 'Exec="%s"\n' "$desktop_exec" >> "$package_dir/reality-client.desktop"
chmod 0755 "$package_dir/reality-client.desktop"

echo 'RUST_CLIENT_LINUX_BUILD=PASS'
echo "PACKAGE=$package_dir"
echo "CORE_SOURCE_COMMIT=$expected_commit"
