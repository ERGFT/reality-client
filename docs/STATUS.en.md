[Русский](STATUS.md) | [English](STATUS.en.md)

# Verification status

What was run and how it was confirmed. "Not verified" does not mean "broken", it
means "nobody ran it". Updated: 2026-10-10.

Contents:

1. [Verified](#1-verified)
2. [Not verified](#2-not-verified)
3. [Known limitations](#3-known-limitations)

## 1. Verified

| What | Where and how | By |
|---|---|---|
| Own local HTTPS + VLESS/TLS lab | Two TLS relays; test-scoped CA, plain/Base64, real-core HTTP/HTTPS to `.invalid`, intact 256 KiB, server switching, stable IDs, reopened DPAPI store, retained data on HTTP 503/empty body, rejected wrong UUID. Reproduced populated-JSON/link_file conflict in .11; lab and binding pass after the fix. 119 Windows library tests with Android bridge passed, 1 opt-in ignored; fmt and all-target Clippy passed. Native installer, browser proxy/TUN and REALITY were not verified in this run | Codex, 2026-10-10 |
| HTTPS subscriptions: Windows | 17 tests: plain/Base64, core parameters, TLS/redirects/limits/timeout/HTTP 403, DPAPI, atomic updates, manual profiles, stable IDs and real Slint callbacks. One TCP connection through an isolated core survives refresh/rename/delete. All-target Clippy with `android-bridge-check`, fmt. Fresh mobile and desktop renders inspected | Codex, 2026-10-09 |
| APK with subscriptions | Android x86_64 Rust library, Gradle Kotlin tests and debug APK built. Isolated APK installed and launched on API35 emulator; startup screenshot inspected, no VPN enabled. Does not verify an actual provider or physical phone | Codex, 2026-10-09 |
| Android subscription runtime | Opt-in `subscription-device-check` in a separate APK: real Slint add/select/refresh/reorder/rename/delete callbacks, stable IDs, Android Keystore URL/server reads after reopening and unchanged manual vault. PASS report on API35 x86_64. No VPN started; keyboard/clipboard input and actual provider remain unverified | Codex, 2026-10-09 |
| Client build on Linux x86_64 | Ubuntu 24.04, `cargo build --locked` | Claude, 2026-10-06 |
| 72 unit tests | `cargo test --locked` in `rust-client/`, all pass (including the dynamic-colour mapping) | Claude, 2026-10-06 |
| `cargo fmt --check`, Clippy | clean (`-D warnings -A dead_code`) | Claude, 2026-10-06 |
| JNI bridge type-checks on the host | `cargo check --features android-bridge-check` | Claude, 2026-10-06 |
| Window startup on Linux | under Xvfb: the window renders and every page opens | Claude, 2026-10-06 |
| Windows x64 cross-build from Linux | `cargo build --locked --release --target x86_64-pc-windows-gnu` + `mingw-w64`: `reality-client-rs.exe` built; not run | Claude, 2026-10-06 |
| CI on GitHub Actions: Linux, Windows, Android | green on `main` and PR #8: formatting, Clippy, tests (Windows: 75), Linux/Windows package build and content check, APK build | GitHub Actions, 2026-10-06 |
| Splitting `lib.rs` into modules | Linux: `fmt`, Clippy, 51 tests, `android-bridge-check`, the window starts; Windows: Clippy via cross-build (`x86_64-pc-windows-gnu`, both feature sets); Android code is compiled only by CI | Claude, 2026-10-06 |
| The core as a Cargo dependency (Linux) | `reality-ffi` is linked into the app; 52 tests, including starting the core in-process: loopback SOCKS listener, `/groups`, `/stats`, reload, stop; Windows: Clippy via cross-build; Android code is compiled by CI only | Claude, 2026-10-06 |
| Android TUN policy in Rust | `android_tun.rs`: 20 host tests (addresses, DNS neighbour, app filter, unsupported options, MTU); `nativePlanTun` and the Kotlin shell are checked by CI only (APK build and Kotlin tests), not run on a device | Claude, 2026-10-06 |
| `cargo xtask` | Linux: `fetch-core` (download, a repeat run changes nothing, a wrong command exits with 2), `fmt`, Clippy. Windows (GitHub Actions, PR #14) previously built `core-cli` and `package-windows`. Locally, `core-cli` built for pinned core `e069518…` (SHA-256 `2DE7A466…C330849`); `cargo xtask test-windows` passed 92 standard and 95 `android-bridge-check` host tests, plus both Clippy runs with `-D warnings -A dead_code`. `cargo xtask package-windows` built a 9-file package with a relative `CARGO_TARGET_DIR`; the Wintun digest matched its pinned value and the shortcut points to the packaged EXE. Fixed target-path lookup and Clippy argument order. `fetch-core` now downloads and verifies in a temporary directory; a failed source leaves no partial destination, and retry plus idempotent repeat passed locally. These checks do not launch the GUI, system proxy, TUN, or remote VPN | Codex, 2026-10-08 |
| Windows GUI package startup | Isolated data directory on D:; the `Reality Client` window appeared and closed cleanly; no connection was initiated | Codex, 2026-10-08 |
| Release `v0.1.0-preview.9` | GitHub API confirms six assets: Windows x64 and Linux x86_64 archives, Android ARM64 debug APK, and three SHA-256 files. The original Windows asset is a ZIP. The Linux publishing error (checkout cleaned the artifact; two events launched duplicate workflows) is fixed on the PR branch; Linux, Windows, and Android Actions passed. The PR changes the future Windows asset to one GUI installer `.exe`; it installs the app files under `%LOCALAPPDATA%\Programs\Reality Client` and adds a shortcut. `preview.9` itself was not rebuilt | Codex, 2026-10-08 |
| Linux installer | `tests/linux-installer-smoke.sh` — PASS | Claude, 2026-10-06 |
| UI layout | `slint-viewer` screenshots: 5 pages, phone (380 px, M3) and desktop (1080 px), both themes | Claude, 2026-10-06 |
| Windows build and tests (66 + 69), DPAPI compatibility with C# | developer machine | previous author ([log](BUILD-LOG.md), Russian); not re-run |
| Debug APK build (arm64), run in an Android 15 emulator | developer machine | previous author; not re-run |

## 2. Not verified

- End-to-end traffic "client → real VLESS/REALITY server → website" on **any** platform.
- Windows: system proxy, TUN (Wintun), and real remote traffic on a clean machine. The GUI package passed a brief isolated startup smoke check, which does not verify connection behavior.
- Linux: TUN and routes, Secret Service on a real desktop, `setcap`.
- Android: an APK build after the changes (Material 3, Kotlin `readSystemPalette` and its
  JNI call), `VpnService` and TUN on a device, `protect(fd)`, network changes,
  permission revocation, dynamic colours on Android 12+.
- The regression test for closing the TUN descriptor lives in vpn-core and runs in its CI; the client no longer runs it.
- Running the built `.exe` and APK: CI only builds them and checks the package contents; they are not launched and no network scenarios run.

## 3. Known limitations

- The core is pinned by a commit hash in `third_party/vpn-core.rev` and fetched at
  build time (no patches). The pin is the merge commit of the TUN-descriptor ownership
  fix ([vpn-core#28](https://github.com/ERGFT/vpn-core/pull/28)).
- Binary files are tracked in git (`dist/RealityClient.exe`, `third_party/reality-client.exe` for C#, `wintun.dll`).
- The desktop layout assumes a window at least 760 px wide; width-based adaptation is
  off to avoid a size loop ([DESIGN.en.md](DESIGN.en.md#4-layout)).
