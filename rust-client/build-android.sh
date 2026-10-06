#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
android_dir="$script_dir/android"
app_dir="$android_dir/app"
core_revision="$(tr -d '[:space:]' < "$repo_root/third_party/vpn-core.rev")"
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

# Ядро — Cargo-зависимость клиента (path-зависимость на third_party/vpn-core),
# линкуется в libreality_client_rs.so; отдельной libreality.so больше нет.
(cd "$repo_root" && cargo xtask fetch-core)

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$build_root/target}"
jni_dir="$app_dir/build/generated/jniLibs"
rm -rf -- "$jni_dir"
mkdir -p "$jni_dir"

(cd "$script_dir" && cargo ndk -t "$abi" --platform "$platform" -o "$jni_dir" build --locked --release --lib)

client_library="$jni_dir/$abi/libreality_client_rs.so"
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
echo "CORE_SOURCE_COMMIT=$core_revision"
