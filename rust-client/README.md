# Reality Client Rust

The Rust/Slint GUI is an in-progress rewrite of the Windows C# reference client and remains a preview. The Windows x64 package is built by `build-windows.ps1` and includes the GPL license, third-party notice, pinned core source archive, source revision marker, and the signed Wintun x64 DLL with its license. The build verifies the bundled Wintun DLL SHA-256 before packaging. The Linux x86_64 package requires a native Linux host and is built by `build-linux.sh`. Release and pinned-core build commands use `--locked` so they fail if `Cargo.lock` does not match the manifests instead of silently selecting different dependencies.

The Rust app reads and writes the Windows-compatible `settings.txt` selection for the full JSON config, so a path selected by the C# client carries over and remains selected after restarting Rust. Desktop startup falls back to `advanced-config.json` if the saved path is missing. An isolated temp-directory regression test covers missing settings and round-tripping a path containing spaces. Current build and verification details, including hashes, platform limits, and test commands, are maintained in the status section below. No remote VPN traffic is claimed as verified.

## Windows host-side tests

The Windows test suites run on Windows without starting a VPN connection or changing the system proxy. They use local temporary files, an in-memory proxy backend, and a loopback SOCKS listener. With the Windows GNU Rust toolchain and MinGW-w64 installed, run from PowerShell:

```powershell
.\tests\windows-test.ps1
```

The helper builds a temporary `shlwapi` import library from `tests/windows/shlwapi.def`, then runs both release-profile suites and strict Clippy for both configurations. The temporary library is removed after the run. Current source passes 46 default tests and 49 with the Android bridge host-check, including the main binary target. Android host-check Clippy allows expected `dead_code` warnings for Android-only code. This Windows host has no Visual Studio MSVC linker, so the GNU Rust target is explicit. The tests do not enable the system proxy, alter routes, or connect to a remote server.

## Linux build prerequisites

Use a native Linux machine with Rust stable, Python 3, `unzip`, `pkg-config`, CMake, NASM, and the development packages required by Slint and libdbus. On Debian/Ubuntu, the expected packages include `python3`, `libdbus-1-dev`, `libxkbcommon-dev`, `libxkbcommon-x11-dev`, `libwayland-dev`, `libx11-xcb-dev`, `libxcb-xkb-dev`, `libfontconfig1-dev`, and `libfreetype6-dev`.

Run from this directory:

```sh
chmod +x build-linux.sh
./build-linux.sh
```

The script verifies the pinned core source revision and SHA-256, applies the version-checked TUN-FD ownership overlay, runs the Linux descriptor-cleanup regression test, builds the core FFI library, CLI config checker, and GUI, then creates `dist/linux-x86_64/` with the files needed to launch the app. It builds into a temporary directory and removes that directory when finished. After building, run `./dist/linux-x86_64/install-linux.sh` to install the program under `~/.local/opt/reality-client` and register a launcher in the user's applications menu. This installer uses no administrator privileges and keeps profile data in the separate XDG data directory. It now rejects an unset or relative `HOME` before writing any files. Its smoke check passed under MSYS using a temporary HOME and XDG data directory containing spaces, plus empty/relative HOME rejection cases; MSYS `/tmp` is not used for this check because Windows utilities cannot reliably access its virtual paths. This does not verify Unix executable permissions or native Linux build/install/runtime behavior.

For Linux full-JSON TUN configs, the pinned core creates the TUN device and owns its marked route rules; normal core Stop removes those routes. The GUI process needs root or `CAP_NET_ADMIN`. To grant only the network capability to the installed GUI binary, an administrator can run `sudo setcap cap_net_admin+ep "$HOME/.local/opt/reality-client/RealityClient"`; reapply it after replacing that binary during updates. Do not enable `strict_route` until you understand the kill-switch behavior: after a crash, remove only the core's marked leftovers with `sudo "$HOME/.local/opt/reality-client/reality-client" --tun-cleanup`. The core refuses to remove routes while an active instance owns its lock and leaves unmarked routes untouched. This client integration is not yet natively built or runtime-verified on Linux.

## Android ARM64 APK build

The Android package currently targets `arm64-v8a`. Install Rust stable, the `aarch64-linux-android` Rust target, `cargo-ndk`, JDK 17, Gradle 8.9, Android SDK platform 35, Android Build-Tools 35.0.0, and Android NDK r25 or newer. Set `ANDROID_SDK_ROOT` and `ANDROID_NDK_HOME`, then run from this directory:

```sh
cargo install cargo-ndk
rustup target add aarch64-linux-android
chmod +x build-android.sh
./build-android.sh
```

The script checks the pinned core source revision and SHA-256, applies the local TUN ownership overlay, runs the Linux-host regression test for descriptor cleanup after a config-parse failure, builds both `libreality.so` and `libreality_client_rs.so` for ARM64, checks that `android_main` and all Kotlin-called JNI methods are exported from the Rust library, runs JVM unit tests for IPv4/IPv6 TUN CIDR parsing, derived DNS peer addresses (including subnet boundaries), and preservation of the pending permission request across Activity recreation, packages the libraries into the Gradle APK, and confirms both shared libraries are present. The APK is written to `android/app/build/outputs/apk/debug/app-debug.apk`. A GitHub Actions workflow installs Android SDK/NDK, builds this APK, and uploads it as a short-lived artifact for device testing. On 2026-10-05, GitHub blocked the latest hosted runs before a runner started because recent account payments failed or the spending limit needs to be increased; hosted verification is therefore pending an account billing fix. All seventeen Android JVM unit tests pass on this Windows host. The ARM64 core and Rust GUI native libraries were cross-compiled from the pinned core and current client source using the isolated SDK/NDK; JNI exports were verified, and Gradle assembled a debug APK containing all four `arm64-v8a` native libraries. Most recent APK SHA-256: `1ACA619E479A55877E54108EDEE1353F933986111BF8142A94B634DA9EF84DC7`. These manual component builds did not execute the Linux-only TUN-FD regression test from the full script. The app has not been installed or run on an Android device, so Android VPN operation remains unverified and unsupported.

Before building, the script applies `patches/apply_core_tun_fd_ownership.py` to the extracted, hash-verified core source. The overlay keeps the supplied TUN descriptor under RAII ownership through config parsing and startup failures, and gives the live TUN device its own descriptor. The Windows FFI DLL and package were rebuilt with this overlay on 2026-10-05; the Android ARM64 core and debug APK were subsequently built from the same patched pinned source. The Linux-only descriptor-cleanup regression test has not yet run on Linux.

The Android manifest declares the VPN foreground service as `systemExempted` and includes its corresponding Android permission, which is the platform category listed for VPN apps configured through system VPN settings. `tests/android-manifest-smoke.py` checks that declaration and the `BIND_VPN_SERVICE` protection; this static check does not replace an APK build or device runtime test.

### Clipboard import update (2026-10-05)

The Rust/Slint profile form now exposes the paste action on Android as well as Windows. On Android, the action reads the foreground app's clipboard through `MainActivity.readClipboardText()` only after the user taps Paste; the shared sanitizer trims surrounding whitespace and rejects multiline clipboard contents before the link reaches the profile field. Verification on this Windows host: Rust ARM64 Android library cross-compilation passed, required JNI exports passed, the 13 Android JVM tests passed, and Gradle assembled a debug APK containing the updated Rust library. After the JSON editor diagnostic UI update, the latest APK SHA-256 is `790109A673941A85333C8B39B47A5BB7E1AEFC39084BCAE317C2DD89348952C4`. No Android device was available, so the GUI paste action and VPN runtime have not been exercised on-device.

The full-JSON checker now captures bounded stdout/stderr while preserving its 20-second timeout, drains excess output to avoid child-process pipe deadlocks, redacts links and credential-like fields, and presents a bounded diagnostic to the user. The JSON editor displays the diagnostic below its buttons; a disconnected GUI smoke confirmed a core error is visible with its specific reason. Windows tests cover output capping and redaction. Android ARM64 cross-compilation and APK packaging also passed after this shared Rust/UI change; latest APK SHA-256: `790109A673941A85333C8B39B47A5BB7E1AEFC39084BCAE317C2DD89348952C4`.

## Current status

- **Interface:** desktop uses a five-section sidebar; Android uses compact top tabs. The dashboard shows server endpoint/IP resolution, direct and proxied public-IP checks, speed and traffic counters. The connections panel currently shows the core's active-connection count and logs; the pinned core API does not expose a per-stream list to this GUI.
- **Website routing:** domain entries accept hostnames or HTTP/HTTPS URLs. A URL path is discarded because the network routing rule applies to the entire hostname. IPv4 and IPv6 ranges use CIDR notation. Android package filtering is exposed when editing a TUN config.
- **Windows:** the current x64 package is built by `build-windows.ps1` and includes `wintun.dll` plus `WINTUN-LICENSE.txt`. The EXE SHA-256 is `D69EED6F3CC72CE4CB09DAD8EC73A57274E46D7A9E60B1C6132B985FC325941A`. The client now binds the pinned core's `rc_set_lock_dir`, prepares a protected `%ProgramData%\RealityClient` directory for TUN, requires the packaged DLL, and offers a confirmed manual `--tun-cleanup` action. TUN requires an elevated client. Host tests and cross-compilation pass, but real interface/routes/WFP behavior has not been exercised; treat Windows TUN as unverified. System-proxy changes and real remote VPN traffic also remain unverified.
- **Linux:** full-JSON TUN and marked-route cleanup are wired to the pinned core. The Windows host cannot establish native Linux compilation, Secret Service integration, route lifecycle, or runtime operation; those remain unverified.
- **Android:** the repository contains the Kotlin `VpnService`, TUN descriptor handoff, and per-app package filtering. The latest shared UI change has not been rebuilt into an APK in this environment because `cargo-ndk` and Android SDK/NDK are unavailable. No device runtime test has been performed.
- **Current Windows checks:** `cargo fmt --check`, 46 default tests, 49 Android bridge host-check tests, and strict Clippy for both configurations pass. These checks do not start TUN, alter routes, change the system proxy, or contact a remote VPN server.
- **Geolocation:** the dashboard does not display a country flag. The core does not supply GeoIP metadata; an online lookup would disclose the server IP to an outside service, while a local GeoIP database would need to be selected and packaged.
- **Release status:** this work is on the PR branch. The locally built Windows package is not evidence that the published preview release or an Android APK contains these latest changes.
The Rust JNI bridge can be type-checked on a desktop host without Android SDK files using `cargo check --features android-bridge-check`. This does not compile the Android-only Slint entry point, Kotlin app, APK, or native Android core.


The Windows GitHub Actions workflow rebuilds the pinned core FFI DLL, runs the Windows feature test suite, packages the x64 app, and checks the expected package files.

## Implementation history

### Android TUN config guard (2026-10-05)

The Android VPN service now rejects full JSON configs unless they contain exactly one `tun` inbound. The service establishes one Android TUN descriptor and hands one descriptor to the core; previously, multiple TUN inbounds silently selected the first for Android setup. The selector has JVM regression coverage for zero, one, and multiple TUN inbounds. Verification: all 14 Android JVM tests passed; Gradle assembled the debug APK and `apksigner` verified its v2 signature. APK SHA-256: `F274DD3004A132381EAEFEF0431471CE9896CAA214D3CC2686B1799FB3B8476D`. This is not an Android device or VPN runtime test.

### Android TUN address parity (2026-10-05)

Android now derives its VPN interface addresses from the same `address`, `inet4_address`, and `inet6_address` fields and defaults used by the pinned core. It passes one effective IPv4 and IPv6 address to `VpnService.Builder`, matching the core's first-per-family selection, and validates IPv4 prefixes against the core's `/30` limit. Verification: all 17 Android JVM tests passed, Gradle assembled the debug APK, and `apksigner` verified its v2 signature. APK SHA-256: `3A5016A9BB1811BBBE8B39A67069872E98C9C6FDFA846956CFD589C08C6FEB40`. No Android device or VPN runtime test was performed.
### Profile operation parity (2026-10-05)

Rust now blocks profile deletion during another operation or an active session, matching the C# reference. Android also treats a pending VPN permission request as an active session; the state guard is covered by the Windows host Rust suites. Current Windows verification: 38 default tests and 41 Android bridge host-check tests, strict Clippy for both, and rebuilt Windows x64 package (SHA-256 `E82EFFB471762B265A3289315D71972CC5FB545F9F586A9FE49ED2D27BD50AFE`). Android ARM64 core and Rust/Slint JNI library cross-compiled, JNI exports passed, all 17 Android JVM tests passed, APK v2 signature verified (SHA-256 `1ACA619E479A55877E54108EDEE1353F933986111BF8142A94B634DA9EF84DC7`). Android device runtime and VPN traffic remain unverified.

### Android route-policy guard (2026-10-05)
`RealityVpnService` now rejects every TUN routing option that the pinned core marks unsupported on Android, plus `inet4_route_exclude_address` and `inet6_route_exclude_address`, which the Android VPN builder could not represent. This check runs before `Builder.establish()`, so unsupported full-JSON settings do not briefly install the Android VPN interface before failing in core startup. Gradle JVM tests: 19 passed, 0 failures/errors; `:app:assembleDebug` succeeded; APK v2 signature verified. APK SHA-256: `4DBCBD056E54B7CCDAD4325D8284E835FC2A1E4C4A0A1F8310A1636A81C51654`. No device installation or VPN runtime was performed.

### Tabbed GUI and Android app selection (2026-10-05)
The Rust/Slint UI has Home, Servers, Connections, and Settings tabs, dark/light themes, a mobile layout, traffic/rate cards, managed domain/IP routing rules, TUN/Fake-IP controls, and an About panel with version and repository. Android lists launchable apps through `PackageManager`; selected packages are applied with `VpnService.Builder.addAllowedApplication`. Because the pinned core rejects `include_package`, the Android service removes that field from the JSON passed to the core after applying the app filter. The dashboard has opt-in public-IP checks through the regular OS route and through a local core SOCKS/mixed inbound. Both call `api.ipify.org`; the provider sees the request IP. The regular OS route may still use an active VPN on Android, while full-config rules may send the proxied check directly. The core API exposes an active-connection count rather than per-stream details. Linux TUN remains without a native build or runtime check.

### External IP status check (2026-10-05)
The Home tab offers explicit external-IP checks through the regular OS route and through a loopback SOCKS or mixed inbound in the running core. Requests go to `api.ipify.org`; the provider sees the request IP. The regular OS route may still use an active VPN on Android, and full-config rules may send the proxied check directly. The proxied check refuses to run while disconnected or when a full config has no local SOCKS-capable inbound. Each request uses a 10-second timeout and accepts a response body up to 128 bytes, then validates it as an IP. A separate action resolves the selected server endpoint through system DNS and displays up to four results; the DNS resolver sees the domain, and these records may not match a load-balanced connection address. The current dashboard also shows the selected profile's configured endpoint and core traffic counters. Verification: 44 Windows Rust tests and 47 Android-bridge host-check tests passed; strict Clippy passed for both. Android ARM64 Rust compilation, Gradle assembly, APK v2 signature verification, and equality of the packaged JNI library hash passed. The 19 Android JVM tests were up-to-date and passed earlier. No live VPN server request or Android device runtime was tested. Current Windows EXE SHA-256: `F298CA3A1E6E2EABD6184DDF8053E5A1BFFE857D79FDCC9FE5555E65FB0A9EEA`; ZIP SHA-256: `02CB1CFDED232A0316C1FC9FFB246CBF31C76022FCADED61CA5F0CE8F382A056`; Android APK SHA-256: `5418660C6C454105A8F57B3293831B735EE2DCF68A56CA9173800B3018DCDE8D`.
