[Русский](ARCHITECTURE.md) | [English](ARCHITECTURE.en.md)

# Client internals

For anyone who reads, changes or extends the client. Usage is in the
[README](../README.en.md); work stages are in [PLAN.en.md](../PLAN.en.md).

Contents:

1. [Layers](#1-layers)
2. [Connection lifecycle](#2-connection-lifecycle)
3. [UI](#3-ui)
4. [The core](#4-the-core)
5. [Threads](#5-threads)
6. [Android](#6-android)
7. [Secret storage](#7-secret-storage)
8. [Repository map](#8-repository-map)
9. [Known technical debt](#9-known-technical-debt)

## 1. Layers

```mermaid
flowchart TB
  subgraph UI[UI]
    S[ui/main.slint<br/>Theme + components + pages]
  end
  subgraph APP[Client logic — rust-client/src]
    LIB[lib.rs<br/>state and UI bindings]
    PR[profiles.rs<br/>profiles and storage]
    SEC[security.rs<br/>secret redaction]
    CORE[core.rs, ffi_core.rs, ffi_session.rs<br/>start and control the core]
    MAT[material.rs<br/>dynamic colours]
  end
  subgraph PLAT[Platform adapters]
    WP[windows_proxy.rs<br/>system proxy]
    PF[platform.rs<br/>paths, window, clipboard, OS]
    AB[android_bridge.rs<br/>JNI]
  end
  K[Kotlin: MainActivity, RealityVpnService]
  LR[libreality — vpn-core, C ABI]

  S <--> LIB
  LIB --> PR & SEC & CORE & MAT
  LIB --> WP & PF & AB
  CORE --> LR
  AB <--> K
  K -- TUN fd, protect --> LR
```

Everything above the "adapters" line is shared between platforms. Platform code
is isolated behind `cfg(...)` and verified separately.

## 2. Connection lifecycle

```mermaid
sequenceDiagram
  actor U as User
  participant UI as Slint UI
  participant L as Client logic
  participant P as Profiles
  participant C as Core (libreality)
  U->>UI: power button
  UI->>L: connect-requested
  L->>P: read the secret link
  L->>L: build and validate the config (--check / rc_start)
  L->>C: rc_start(config, base_dir, tun_fd)
  C-->>L: handle or error (secrets redacted)
  L->>UI: status-text, connect-button-text
  loop every second
    L->>C: rc_request GET /stats, /connections
    L->>UI: speed, traffic, connections
  end
  U->>UI: power button
  L->>C: rc_stop
  L->>L: restore the system proxy
```

The UI derives the power button state from `connect-button-text`: "Подключить"
(Connect) means off, "Отключить" (Disconnect) means on, anything else means an
operation is in progress.

## 3. UI

- A single file, `ui/main.slint`. Colours, sizes and shapes live in the global
  `Theme`; components use only its tokens ([DESIGN.en.md](DESIGN.en.md)).
- One adaptive layout: a sidebar on desktop, a bottom navigation bar on phones
  (`mobile-layout`, set by the Android entry point).
- The link to Rust is the set of `property`/`callback` items on the root
  `MainWindow`. Rust sets the status strings; the markup derives the look from them.
- To work on the UI without the core: `slint-viewer ui/main.slint --component
  MainWindow --load-data demo.json` ([CONTRIBUTING.en.md](../CONTRIBUTING.en.md)).

## 4. The core

vpn-core is used as a library through its C ABI: `rc_start`, `rc_request`,
`rc_reload`, `rc_stop`, event and log callbacks, `rc_set_protect` (Android),
`rc_set_lock_dir` (Windows). The client loads it dynamically (`libloading`). The
config is sing-box/Xray JSON; the VLESS link is turned into it through a private
temporary file (`link_file`) so the secret never lands in process arguments.

The core version is pinned by a commit hash in `third_party/vpn-core.rev`; the
build scripts (`scripts/fetch-core.sh`, `scripts/fetch-core.ps1`) fetch exactly that
commit from the vpn-core repository. A commit hash fixes the content by itself, so
no separate checksum or local patches are needed.

> [!NOTE]
> Target design (stage 4 in [PLAN.en.md](../PLAN.en.md)): `reality-core` as a
> plain Cargo dependency, with no C ABI or `unsafe` wrappers on desktop. The C ABI
> stays only where the core is loaded into a process with a Kotlin layer (Android).

## 5. Threads

- Blocking calls (secret store reads, file dialogs, `rc_request`, IP checks) run
  off the UI thread and return results through `slint::invoke_from_event_loop`.
- Core callbacks arrive on its background threads; data goes into a log queue the
  UI drains on a timer.
- One core handle per session. A second start during a start, or a stop during a
  stop, is rejected by state flags.
- On window close (Windows, Linux) the client waits for active operations, stops
  the core and does not exit until the system proxy has been restored.

## 6. Android

The `VpnService` class has to be Kotlin/Java: it requests permission, creates the
TUN device, holds the foreground notification and hands the descriptor to the
core. UI, profiles and logic are Rust (`slint` + `android-activity`).

- The config reaches the service through a temporary file in the app-private
  directory, not an `Intent` (Binder size limit). The file is erased after it is
  read, cancelled or fails.
- Before `establish()` the service validates the config: exactly one `tun`
  inbound, supported routes and app filter. Anything unsupported is rejected
  before the interface exists.
- Colours: `MainActivity.readSystemPalette()` reads the Material You palette;
  `material.rs` maps it to tokens ([DESIGN.en.md](DESIGN.en.md#dynamic-colours-android)).

## 7. Secret storage

| OS | Store |
|---|---|
| Windows | DPAPI (CurrentUser); format compatible with the previous C# version |
| Linux | Secret Service (libsecret / D-Bus) |
| Android | AES-GCM + Android Keystore |

Secrets are not written to logs, process arguments or error messages
(`security.rs` redacts links, UUIDs and query parameters). The profile store is
capped at 2 MiB and links at 16 KiB.

## 8. Repository map

| Path | What |
|---|---|
| `rust-client/src/` | client logic and platform adapters |
| `rust-client/ui/main.slint` | the UI |
| `rust-client/android/` | Kotlin: `MainActivity`, `RealityVpnService`, TUN policy, JVM tests |
| `rust-client/assets/` | app icon |
| `rust-client/build-*.{sh,ps1}` | per-platform package builds |
| `rust-client/patches/` | local patches to the pinned core |
| `third_party/` | pinned core, `wintun.dll` |
| `src/`, `build.ps1`, `dist/` | previous C# version (archive) |
| `.github/workflows/` | CI: Linux, Windows, Android |

## 9. Known technical debt

- `rust-client/src/lib.rs` is a ~3600-line monolith: state, handlers and UI
  bindings should be split into modules (stage 5).
- The core is built from a zip archive with a Python patch (stage 4).
- Binary files are tracked in git (`dist/`, `third_party/*.exe`, `*.zip`, `wintun.dll`).
- Android TUN policy lives in Kotlin; it should move to Rust (stage 7).
