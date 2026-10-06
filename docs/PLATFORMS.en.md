[Русский](PLATFORMS.md) | [English](PLATFORMS.en.md)

# Platforms: build, run, privileges

Contents:

1. [Linux](#1-linux)
2. [Windows](#2-windows)
3. [Android](#3-android)
4. [Connection modes and privileges](#4-connection-modes-and-privileges)
5. [Common checks](#5-common-checks)

> [!WARNING]
> No platform has been verified end-to-end against a real server. What was
> actually run is listed in [STATUS.en.md](STATUS.en.md).

## 1. Linux

**Dependencies** (Debian/Ubuntu):

```sh
sudo apt install build-essential cmake nasm pkg-config git python3 \
    libdbus-1-dev libfontconfig1-dev libfreetype6-dev \
    libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libx11-dev libgl1-mesa-dev
```

You need Rust stable (2024 edition). Profile storage needs a running Secret
Service (GNOME Keyring, KWallet).

**Build and install:**

```sh
cd rust-client
./build-linux.sh          # dist/linux-x86_64/ — the build verifies the pinned core
./install-linux.sh        # ~/.local/opt/reality-client, per-user launcher and icon
```

The build script fetches vpn-core by exactly the commit hash in `third_party/vpn-core.rev`
into `third_party/vpn-core/` (`cargo xtask fetch-core`), builds the core's command-line
program and the UI; the core itself is linked into `RealityClient` as a Cargo dependency
(there is no separate `libreality.so`). To build against a local copy of the core:
`REALITY_CORE_URL=/path/to/vpn-core ./build-linux.sh` (`third_party/vpn-core` must be
empty or absent).

**Client tests only:** first `cargo xtask fetch-core`, then
`cargo test --locked` in `rust-client/` (the core compiles together with the client;
`cmake` and `nasm` are needed, as for `aws-lc`).

**TUN** needs `root` or `CAP_NET_ADMIN`:

```sh
sudo setcap cap_net_admin+ep "$HOME/.local/opt/reality-client/RealityClient"
```

Repeat after replacing the binary. Do not enable `strict_route` without
understanding the kill switch: after a crash, leftovers are removed with the
core's `reality-client --tun-cleanup` as root.

## 2. Windows

**Cross-build from Linux** (a compile check without Windows): `rustup target add x86_64-pc-windows-gnu`, `apt install gcc-mingw-w64-x86-64`, then
`CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc cargo build --locked --release --target x86_64-pc-windows-gnu`.

**Dependencies:** Rust (`stable-x86_64-pc-windows-gnu`), MSYS2 MinGW-w64 GCC (default
path `%LOCALAPPDATA%\Programs\msys64\mingw64\bin`, overridable with `REALITY_MINGW_BIN`).

```powershell
cargo xtask core-cli                     # fetches the core by commit hash into third_party\vpn-core and builds rust-client\third_party\reality-client.exe
cargo xtask package-windows              # core + client + package: rust-client\dist\windows-x64\RealityClient-Rust.exe
cargo xtask test-windows                 # tests and Clippy on the GNU toolchain, no MSVC link.exe
```

The package holds `RealityClient-Rust.exe` (the core is inside it) and `third_party\reality-client.exe`
(both built from one core commit), `wintun.dll` (the official one, checked by SHA-256),
licences and `CORE-SOURCE.txt` with the core's source repository and commit (GPL).

**System proxy** is enabled only in profile mode and restores the previous values on
disconnect. A backup is kept on disk; after a crash the client offers recovery.

**TUN** needs Run as administrator and `wintun.dll` next to the app. Do not keep the
whole UI elevated without need.

## 3. Android

**Dependencies:** Rust target `aarch64-linux-android` (`x86_64-linux-android` for an
emulator), `cargo-ndk`, JDK 17, Gradle 8.9, Android SDK platform 35, Build-Tools
35.0.0, NDK r25+.

```sh
cargo install cargo-ndk
rustup target add aarch64-linux-android
cd rust-client
./build-android.sh                       # debug APK, arm64-v8a
ANDROID_ABI=x86_64 ./build-android.sh    # for an emulator
```

Accept the Android SDK licences by hand. Kotlin-layer tests: `gradle test` in
`rust-client/android/`.

- Installing the APK does not enable the VPN: Android asks for permission separately.
- Minimum Android is 8.0 (API 26). Material You dynamic colours need Android 12
  (API 31); older versions use the built-in palette.
- The service is declared `systemExempted`, as for VPN apps.

## 4. Connection modes and privileges

| Mode | What it does | Privileges |
|---|---|---|
| Profile (proxy) | the core listens on `127.0.0.1:1080` (SOCKS5/HTTP); on Windows the system proxy can be enabled | regular |
| Full config, no TUN | any sing-box/Xray config without a `tun` inbound | regular |
| Full config with TUN, Windows | Wintun, `/1` routes; kill switch with `strict_route` | administrator |
| Full config with TUN, Linux | the core's TUN device and routes | `root` or `CAP_NET_ADMIN` |
| Android | `VpnService` hands the core a TUN descriptor | user's VPN permission |

On Android, unsupported routing options (route exclusions, several `tun` inbounds,
and so on) are rejected before the interface is created.

## 5. Common checks

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings -A dead_code
cargo test --locked
cargo check --locked --features android-bridge-check    # JNI bridge without an Android SDK
```

Details and contribution rules: [CONTRIBUTING.en.md](../CONTRIBUTING.en.md).
