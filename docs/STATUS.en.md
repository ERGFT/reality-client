[Русский](STATUS.md) | [English](STATUS.en.md)

# Verification status

What was run and how it was confirmed. "Not verified" does not mean "broken", it
means "nobody ran it". Updated: 2026-10-06.

Contents:

1. [Verified](#1-verified)
2. [Not verified](#2-not-verified)
3. [Known limitations](#3-known-limitations)

## 1. Verified

| What | Where and how | By |
|---|---|---|
| Client build on Linux x86_64 | Ubuntu 24.04, `cargo build --locked` | Claude, 2026-10-06 |
| 51 unit tests | `cargo test --locked` in `rust-client/`, all pass (including the dynamic-colour mapping) | Claude, 2026-10-06 |
| `cargo fmt --check`, Clippy | clean (`-D warnings -A dead_code`) | Claude, 2026-10-06 |
| JNI bridge type-checks on the host | `cargo check --features android-bridge-check` | Claude, 2026-10-06 |
| Window startup on Linux | under Xvfb: the window renders and every page opens | Claude, 2026-10-06 |
| Windows x64 cross-build from Linux | `cargo build --locked --release --target x86_64-pc-windows-gnu` + `mingw-w64`: `reality-client-rs.exe` built; not run | Claude, 2026-10-06 |
| Linux installer | `tests/linux-installer-smoke.sh` — PASS | Claude, 2026-10-06 |
| UI layout | `slint-viewer` screenshots: 5 pages, phone (380 px, M3) and desktop (1080 px), both themes | Claude, 2026-10-06 |
| Windows build and tests (66 + 69), DPAPI compatibility with C# | developer machine | previous author ([log](BUILD-LOG.md), Russian); not re-run |
| Debug APK build (arm64), run in an Android 15 emulator | developer machine | previous author; not re-run |

## 2. Not verified

- End-to-end traffic "client → real VLESS/REALITY server → website" on **any** platform.
- Windows: running the built `.exe`, system proxy and TUN (Wintun) on a clean machine; the full `build-windows.ps1` package (core, `wintun.dll`).
- Linux: TUN and routes, Secret Service on a real desktop, `setcap`.
- Android: an APK build after the changes (Material 3, Kotlin `readSystemPalette` and its
  JNI call), `VpnService` and TUN on a device, `protect(fd)`, network changes,
  permission revocation, dynamic colours on Android 12+.
- The Linux regression test for closing the TUN descriptor (part of the build script, not run separately).
- CI: GitHub Actions jobs start and end at once with no steps — an account billing
  block; there are no hosted build results. Linux and Windows (cross-build) can be checked locally without CI; Android cannot: it needs the Android SDK (`dl.google.com` is unreachable in Claude's environment).

## 3. Known limitations

- The core is pinned to commit `ee68039` and built with a local patch (TUN descriptor
  ownership); vpn-core's main branch has moved on.
- `rust-client/src/lib.rs` is a ~3600-line monolith.
- Binary files are tracked in git (`dist/RealityClient.exe`, `third_party/*.exe`, `*.zip`, `wintun.dll`).
- The desktop layout assumes a window at least 760 px wide; width-based adaptation is
  off to avoid a size loop ([DESIGN.en.md](DESIGN.en.md#4-layout)).
