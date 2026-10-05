#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
    echo "Usage: $0 <android-ndk-root> <client-library.so>" >&2
    exit 2
fi

ndk_root="$1"
client_library="$2"
[[ -f "$client_library" ]] || {
    echo "Missing Android client library: $client_library" >&2
    exit 1
}

llvm_nm="${ANDROID_LLVM_NM:-}"
if [[ -z "$llvm_nm" ]]; then
    llvm_nm="$(find "$ndk_root/toolchains/llvm/prebuilt" -type f \
        \( -name llvm-nm -o -name llvm-nm.exe \) -print -quit 2>/dev/null || true)"
fi
[[ -x "$llvm_nm" ]] || {
    echo 'Android NDK llvm-nm was not found; cannot validate exported JNI entry points.' >&2
    exit 1
}

exported_symbols="$("$llvm_nm" --dynamic --defined-only "$client_library")"
for symbol in \
    android_main \
    Java_com_ergft_realityclient_MainActivity_nativeVpnPermissionDenied \
    Java_com_ergft_realityclient_MainActivity_nativeActivityDestroyed \
    Java_com_ergft_realityclient_RealityVpnService_nativeStart \
    Java_com_ergft_realityclient_RealityVpnService_nativeStop \
    Java_com_ergft_realityclient_RealityVpnService_nativeVpnStartFailed; do
    grep -Eq "(^|[[:space:]])${symbol}$" <<<"$exported_symbols" || {
        echo "Android client library is missing the required exported symbol: $symbol" >&2
        exit 1
    }
done

echo 'ANDROID_JNI_EXPORTS=PASS'
