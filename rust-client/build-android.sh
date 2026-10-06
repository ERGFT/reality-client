#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
android_dir="$script_dir/android"
app_dir="$android_dir/app"
archive="$repo_root/third_party/vpn-core-source.zip"
revision_file="$repo_root/third_party/vpn-core-source.commit"
expected_commit='ee68039943ebb2aaf3287bf622ae34c18bfa0cae'
expected_sha256='DF789EABC39029A403D72BA78367637E4347D372626A5FC79F35EC901AF1317D'
abi="${ANDROID_ABI:-arm64-v8a}"
case "$abi" in
    arm64-v8a) target='aarch64-linux-android' ;;
    x86_64) target='x86_64-linux-android' ;;
    *)
        echo "Unsupported Android ABI: $abi (supported: arm64-v8a, x86_64)." >&2
        exit 2
        ;;
esac
platform=26

for tool in cargo rustup cargo-ndk gradle sha256sum python3; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "Missing required tool: $tool" >&2
        exit 1
    }
done

sdk_root="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-}}"
[[ -n "$sdk_root" && -d "$sdk_root" ]] || {
    echo 'Set ANDROID_SDK_ROOT or ANDROID_HOME to an installed Android SDK.' >&2
    exit 1
}
android_jar="$sdk_root/platforms/android-35/android.jar"
[[ -f "$android_jar" ]] || {
    echo "Install Android SDK Platform 35 (platforms;android-35); missing: $android_jar" >&2
    exit 1
}
# cargo-ndk sets ANDROID_PLATFORM to the native API level (26 below), while
# Slint's android-build helper also treats that variable as an SDK platform.
# Pin the installed compile SDK jar explicitly so both meanings stay distinct.
export ANDROID_JAR="$android_jar"

ndk_root="${ANDROID_NDK_HOME:-${ANDROID_NDK_ROOT:-}}"
if [[ -z "$ndk_root" && -n "${ANDROID_HOME:-}" ]]; then
    ndk_root="$(find "$ANDROID_HOME/ndk" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | sort -V | tail -n 1 || true)"
fi
[[ -n "$ndk_root" && -d "$ndk_root" ]] || {
    echo 'Set ANDROID_NDK_HOME to an installed Android NDK (r25 or newer).' >&2
    exit 1
}
export ANDROID_NDK_HOME="$ndk_root"

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
rustup target list --installed | grep -Fxq "$target" || {
    echo "Install the Rust target first: rustup target add $target" >&2
    exit 1
}

python3 "$script_dir/tests/android-manifest-smoke.py" "$app_dir/src/main/AndroidManifest.xml"
gradle --no-daemon -p "$android_dir" :app:testDebugUnitTest

tmp_parent="$(cd -- "${TMPDIR:-/tmp}" && pwd)"
build_root="$(mktemp -d "$tmp_parent/reality-client-android.XXXXXXXX")"
cleanup() {
    case "$build_root" in
        "$tmp_parent"/reality-client-android.*) rm -rf -- "$build_root" ;;
        *) echo "Refusing to remove unexpected temporary path: $build_root" >&2; return 1 ;;
    esac
}
trap cleanup EXIT

source_dir="$build_root/core-source"
mkdir -p "$source_dir"
python3 - "$archive" "$source_dir" <<'PY'
import pathlib
import sys
import zipfile

archive = pathlib.Path(sys.argv[1])
destination = pathlib.Path(sys.argv[2]).resolve()
with zipfile.ZipFile(archive) as bundle:
    for member in bundle.infolist():
        target = (destination / member.filename).resolve()
        if target != destination and destination not in target.parents:
            raise SystemExit(f"Refusing archive path outside extraction root: {member.filename}")
    bundle.extractall(destination)
PY
[[ -f "$source_dir/Cargo.toml" ]] || {
    echo 'Pinned archive does not contain the expected workspace Cargo.toml.' >&2
    exit 1
}
python3 "$script_dir/patches/apply_core_tun_fd_ownership.py" "$source_dir"

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$build_root/target}"
if [[ "$(uname -s)" == Linux* ]]; then
    (
        cd "$source_dir"
        cargo test --locked -p reality-ffi --lib \
            tun_fd_ownership_tests::rc_start_closes_system_tun_fd_when_config_parse_fails
    )
else
    echo 'LINUX_TUN_FD_REGRESSION=SKIP (requires a native Linux host; run the Linux CI workflow)'
fi
jni_dir="$app_dir/build/generated/jniLibs"
rm -rf -- "$jni_dir"
mkdir -p "$jni_dir"

(cd "$source_dir" && cargo ndk -t "$abi" --platform "$platform" -o "$jni_dir" build --locked --release -p reality-ffi)
(cd "$script_dir" && cargo ndk -t "$abi" --platform "$platform" -o "$jni_dir" build --locked --release --lib)

core_library="$jni_dir/$abi/libreality.so"
client_library="$jni_dir/$abi/libreality_client_rs.so"
[[ -f "$core_library" ]] || { echo "Missing Android core FFI library: $core_library" >&2; exit 1; }
[[ -f "$client_library" ]] || { echo "Missing Android Slint/JNI library: $client_library" >&2; exit 1; }

bash "$script_dir/check-android-exports.sh" "$ndk_root" "$client_library"

gradle --no-daemon -p "$android_dir" :app:assembleDebug
apk="$app_dir/build/outputs/apk/debug/app-debug.apk"
[[ -f "$apk" ]] || { echo "Gradle did not create the expected APK: $apk" >&2; exit 1; }
python3 - "$apk" "$abi" <<'PY'
import sys
import zipfile

apk, abi = sys.argv[1:]
required = {
    f"lib/{abi}/libreality.so",
    f"lib/{abi}/libreality_client_rs.so",
}
with zipfile.ZipFile(apk) as package:
    missing = required.difference(package.namelist())
if missing:
    raise SystemExit("APK is missing: " + ", ".join(sorted(missing)))
PY

echo 'ANDROID_APK_BUILD=PASS'
echo "ABI=$abi"
echo "APK=$apk"
echo "CORE_SOURCE_COMMIT=$expected_commit"
