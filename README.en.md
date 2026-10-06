# Reality Client

[Русский](README.md) | **English**

A cross-platform GUI VPN client written in **Rust + Slint** for the
[**vpn-core**](https://github.com/ERGFT/vpn-core) network core (VLESS, REALITY,
XTLS Vision). Paste a `vless://…` link, press the power button, and the client
starts the core, enables a proxy or a TUN device, and shows speed, traffic and logs.

## Status

| Platform | Build | Verified | Not yet verified |
|---|---|---|---|
| Linux x86_64 | `rust-client/build-linux.sh` | builds and passes 45 unit tests on Ubuntu 24.04; the window starts and renders | traffic through a real server, TUN, Secret Service on a real desktop |
| Windows x64 | `rust-client/build-windows.ps1` | built and tested on the developer machine | system proxy / TUN on a clean machine, real traffic |
| Android arm64 | `rust-client/build-android.sh` | debug APK builds and starts in an emulator | `VpnService`/TUN on a device, real traffic |
| macOS / iOS / tvOS | — | — | postponed, see [PLAN.md](PLAN.md) |

Pre-release stage: no platform has been verified end-to-end against a real
REALITY server yet. See [docs/STATUS.md](docs/STATUS.md) (Russian) for the
detailed matrix. Licensed under GPL-3.0-or-later.
