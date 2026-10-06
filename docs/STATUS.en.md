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
| 72 unit tests | `cargo test --locked` in `rust-client/`, all pass (including the dynamic-colour mapping) | Claude, 2026-10-06 |
| `cargo fmt --check`, Clippy | clean (`-D warnings -A dead_code`) | Claude, 2026-10-06 |
| JNI bridge type-checks on the host | `cargo check --features android-bridge-check` | Claude, 2026-10-06 |
| Window startup on Linux | under Xvfb: the window renders and every page opens | Claude, 2026-10-06 |
| Windows x64 cross-build from Linux | `cargo build --locked --release --target x86_64-pc-windows-gnu` + `mingw-w64`: `reality-client-rs.exe` built; not run | Claude, 2026-10-06 |
| CI on GitHub Actions: Linux, Windows, Android | green on `main` and PR #8: formatting, Clippy, tests (Windows: 75), Linux/Windows package build and content check, APK build | GitHub Actions, 2026-10-06 |
| Splitting `lib.rs` into modules | Linux: `fmt`, Clippy, 51 tests, `android-bridge-check`, the window starts; Windows: Clippy via cross-build (`x86_64-pc-windows-gnu`, both feature sets); Android code is compiled only by CI | Claude, 2026-10-06 |
| The core as a Cargo dependency (Linux) | `reality-ffi` is linked into the app; 52 tests, including starting the core in-process: loopback SOCKS listener, `/groups`, `/stats`, reload, stop; Windows: Clippy via cross-build; Android code is compiled by CI only | Claude, 2026-10-06 |
| Android TUN policy in Rust | `android_tun.rs`: 20 host tests (addresses, DNS neighbour, app filter, unsupported options, MTU); `nativePlanTun` and the Kotlin shell are checked by CI only (APK build and Kotlin tests), not run on a device | Claude, 2026-10-06 |
| `cargo xtask` | Linux: `fetch-core` (download, a repeat run changes nothing, a wrong command exits with 2), `fmt`, Clippy. Windows (GitHub Actions, PR #14): `core-cli` and `package-windows` ran — the core's command-line program and the client with the core inside were built, and the package holds every expected file (the packaged-files step passed), including the `.lnk` and `CORE-SOURCE.txt`. `test-windows` has not been run anywhere (CI calls `cargo test` directly); how the `.lnk` behaves when the folder is moved was not checked | Claude, GitHub Actions, 2026-10-06 |
| Linux installer | `tests/linux-installer-smoke.sh` — PASS | Claude, 2026-10-06 |
| UI layout | `slint-viewer` screenshots: 5 pages, phone (380 px, M3) and desktop (1080 px), both themes | Claude, 2026-10-06 |
| Windows build and tests (66 + 69), DPAPI compatibility with C# | developer machine | previous author ([log](BUILD-LOG.md), Russian); not re-run |
| Debug APK build (arm64), run in an Android 15 emulator | developer machine | previous author; not re-run |

## 2. Not verified

- End-to-end traffic "client → real VLESS/REALITY server → website" on **any** platform.
- Windows: running the built `.exe`, system proxy and TUN (Wintun) on a clean machine; the full `cargo xtask package-windows` package (core, `wintun.dll`).
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
