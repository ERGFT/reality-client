#!/usr/bin/env python3
"""Check Android VPN foreground-service declarations without an Android SDK."""

import sys
import xml.etree.ElementTree as ET
from pathlib import Path


ANDROID = "http://schemas.android.com/apk/res/android"
ANDROID_NAME = f"{{{ANDROID}}}name"


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(f"ANDROID_MANIFEST_CHECK=FAIL: {message}")


manifest_path = (
    Path(sys.argv[1])
    if len(sys.argv) == 2
    else Path(__file__).resolve().parents[1]
    / "android/app/src/main/AndroidManifest.xml"
)
root = ET.parse(manifest_path).getroot()
permissions = {
    node.get(ANDROID_NAME)
    for node in root.findall("uses-permission")
}
require(
    "android.permission.FOREGROUND_SERVICE" in permissions,
    "base foreground-service permission is missing",
)
require(
    "android.permission.ACCESS_NETWORK_STATE" in permissions,
    "network-state permission required by the VPN conflict check is missing",
)
require(
    "android.permission.FOREGROUND_SERVICE_SYSTEM_EXEMPTED" in permissions,
    "VPN system-exempted foreground-service permission is missing",
)
require(
    "android.permission.FOREGROUND_SERVICE_SPECIAL_USE" not in permissions,
    "generic special-use permission should not substitute for the VPN service type",
)

service = root.find("application/service[@android:name='.RealityVpnService']", {
    "android": ANDROID,
})
require(service is not None, "RealityVpnService declaration is missing")
require(
    service.get(f"{{{ANDROID}}}foregroundServiceType") == "systemExempted",
    "RealityVpnService must declare systemExempted",
)
require(
    service.get(f"{{{ANDROID}}}permission") == "android.permission.BIND_VPN_SERVICE",
    "RealityVpnService must be protected by BIND_VPN_SERVICE",
)
require(
    service.get(f"{{{ANDROID}}}exported") == "true",
    "RealityVpnService must be exported for the Android VPN system to bind",
)
print("ANDROID_MANIFEST_CHECK=PASS")
