#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
tmp_parent="$(cd -- "${TMPDIR:-/tmp}" && pwd)"
test_root="$(mktemp -d "$tmp_parent/reality-client-android-exports-smoke.XXXXXXXX")"
cleanup() {
    case "$test_root" in
        "$tmp_parent"/reality-client-android-exports-smoke.*) rm -rf -- "$test_root" ;;
        *) echo "Refusing to remove unexpected temporary path: $test_root" >&2; return 1 ;;
    esac
}
trap cleanup EXIT

fake_nm="$test_root/llvm-nm"
client_library="$test_root/libreality_client_rs.so"
cat > "$fake_nm" <<'FAKE_NM'
#!/usr/bin/env bash
set -euo pipefail
for symbol in \
    android_main \
    Java_com_ergft_realityclient_MainActivity_nativeVpnPermissionDenied \
    Java_com_ergft_realityclient_MainActivity_nativeActivityDestroyed \
    Java_com_ergft_realityclient_RealityVpnService_nativeStart \
    Java_com_ergft_realityclient_RealityVpnService_nativePlanTun \
    Java_com_ergft_realityclient_RealityVpnService_nativeStop \
    Java_com_ergft_realityclient_RealityVpnService_nativeVpnStartFailed; do
    [[ "${ANDROID_EXPORTS_MISSING:-}" == "$symbol" ]] && continue
    printf '0000000000001000 T %s\n' "$symbol"
done
FAKE_NM
chmod +x "$fake_nm"
: > "$client_library"

pass_output="$(ANDROID_LLVM_NM="$fake_nm" bash "$script_dir/../check-android-exports.sh" "$test_root/fake-ndk" "$client_library")"
[[ "$pass_output" == 'ANDROID_JNI_EXPORTS=PASS' ]]

if fail_output="$(ANDROID_LLVM_NM="$fake_nm" ANDROID_EXPORTS_MISSING=Java_com_ergft_realityclient_MainActivity_nativeActivityDestroyed bash "$script_dir/../check-android-exports.sh" "$test_root/fake-ndk" "$client_library" 2>&1)"; then
    echo 'Android export check accepted a missing Activity lifecycle callback.' >&2
    exit 1
fi
[[ "$fail_output" == *'missing the required exported symbol: Java_com_ergft_realityclient_MainActivity_nativeActivityDestroyed'* ]]

echo 'ANDROID_EXPORTS_SMOKE=PASS'
