#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
core_revision="$(tr -d '[:space:]' < "$repo_root/third_party/vpn-core.rev")"

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
bash "$repo_root/scripts/fetch-core.sh" "$source_dir"

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
install -m 0644 "$script_dir/PACKAGE-README.md" "$package_dir/README.md"
install -m 0755 "$script_dir/install-linux.sh" "$package_dir/install-linux.sh"
install -m 0644 "$script_dir/assets/icon.png" "$package_dir/reality-client.png"

cat > "$package_dir/reality-client.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=Reality Client
Comment=VLESS/REALITY client
Terminal=false
Icon=reality-client
StartupWMClass=reality-client
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
echo "CORE_SOURCE_COMMIT=$core_revision"
