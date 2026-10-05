#!/usr/bin/env bash
set -euo pipefail

package_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
home_dir="${HOME:-}"
if [[ -z "$home_dir" || "$home_dir" != /* ]]; then
    echo 'HOME must be set to an absolute directory before installing.' >&2
    exit 1
fi

install_dir="$home_dir/.local/opt/reality-client"
data_home="${XDG_DATA_HOME:-$home_dir/.local/share}"
if [[ "$data_home" != /* ]]; then
    data_home="$home_dir/.local/share"
fi
applications_dir="$data_home/applications"

for file in RealityClient reality-client libreality.so LICENSE.txt README.md; do
    [[ -f "$package_dir/$file" ]] || {
        echo "Missing package file: $package_dir/$file" >&2
        exit 1
    }
done

install -d -m 0755 "$install_dir" "$applications_dir"
install -m 0755 "$package_dir/RealityClient" "$install_dir/RealityClient"
install -m 0755 "$package_dir/reality-client" "$install_dir/reality-client"
install -m 0644 "$package_dir/libreality.so" "$install_dir/libreality.so"
install -m 0644 "$package_dir/LICENSE.txt" "$install_dir/LICENSE.txt"
install -m 0644 "$package_dir/README.md" "$install_dir/README.md"

desktop_exec="$install_dir/RealityClient"
desktop_exec="${desktop_exec//\\/\\\\}"
desktop_exec="${desktop_exec//\"/\\\"}"
desktop_exec="${desktop_exec//%/%%}"
desktop_file="$applications_dir/reality-client.desktop"
cat > "$desktop_file" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Reality Client
Comment=VLESS/REALITY client
Exec="$desktop_exec"
Terminal=false
Categories=Network;Security;
DESKTOP
chmod 0644 "$desktop_file"

echo 'RUST_CLIENT_LINUX_INSTALL=PASS'
echo "APPLICATION=$install_dir/RealityClient"
echo "DESKTOP_ENTRY=$desktop_file"
