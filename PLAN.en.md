[Русский](PLAN.md) | [English](PLAN.en.md)

# Plan: a Rust client for vpn-core

The goal is a graphical client for [vpn-core](https://github.com/ERGFT/vpn-core),
**written entirely in Rust** wherever possible: shared logic, UI (Slint), platform
adapters. Only thin helper layers are allowed where the platform leaves no choice,
for example Android's `VpnService` class has to be Kotlin/Java.

Platform priority: **Windows → Android → Linux**. Apple (macOS, iOS, tvOS) is
postponed: it has its own network-extension model.

Stage status (updated as work goes on):

| Stage | What | Status |
|---|---|---|
| 0 | Skeleton: profiles, starting the core over the C ABI, logs, builds for three platforms | ✅ in place, preliminary |
| 1 | UI: one theme, adaptive layout, a power-button home screen; Android in Material 3 Expressive with dynamic colours | ✅ layout verified by screenshots; dynamic colours not verified on a device |
| 2 | Repository polish after vpn-core: README, docs, bilingual pages, status | ✅ |
| 3 | CI: green builds and checks on three OSes (the repository is public, Actions are free) | 🔶 Android and Linux passed on PR #18; Windows is running |
| 4 | The core without a zip archive and a Python patch: the TUN-descriptor fix goes into vpn-core itself ([#28](https://github.com/ERGFT/vpn-core/pull/28)), the build fetches the core by commit hash; the core is a Cargo dependency of the client (`reality-ffi`, linked into the app, no `libloading` or separate `.dll`/`.so`) | ✅ Linux verified, Windows/Android in CI |
| 5 | `rust-client/src/lib.rs` (3600 lines) split into modules: `ui/` (handlers by topic, `UiState`) and pure modules with tests; behaviour and tests unchanged | ✅ |
| 6 | End-to-end checks: a real REALITY server → Windows, Linux, Android | ⏳ needs a server and devices |
| 7 | Android: TUN policy moved from Kotlin to Rust (`android_tun.rs`, host tests), `VpnService` is a thin shell | ✅ not checked on a device |
| 8 | PowerShell scripts replaced by `cargo xtask` (`fetch-core`, `core-cli`, `package-windows`, `test-windows`); C# and its scripts go after parity and stage 6 | 🔶 Local `package-windows` and `test-windows` passed on GNU toolchain (92/95 host tests, both Clippy sets); GUI EXE has the Windows GUI subsystem. PR #18 adds the installer; Windows CI is still running. C# stays |
| 9 | First release: Windows installer, Android/Linux builds, SHA-256, install and update instructions | 🔶 PR #18 switches future Windows releases to one per-user `.exe` installer with a shortcut; `v0.1.0-preview.9` is unchanged and still contains a ZIP. Android/Linux CI passed; Windows CI is running. Windows signing and updates require separate setup |
| 10 | Apple: macOS, then iOS/tvOS (Network Extension) | 💤 postponed |
| 11 | Finish the owner's feedback: Windows opens the GUI directly without a console; Android respects cutouts, system bars and gesture areas across popular models using system insets; verify UI/packages on real devices | 🔶 PR #18 adds the Windows GUI subsystem, hidden core subprocesses, an Inno Setup per-user installer and Android safe insets. Local checks and Android/Linux CI passed; Windows CI is running. Pixel/OEM device checks and end-to-end REALITY connection are still pending. SmartScreen depends on Authenticode signing and publisher reputation |

## Principles

- **Honest status.** "Verified" means: it was run and recorded in
  [docs/STATUS.en.md](docs/STATUS.en.md). Everything else is "not verified".
- **No secrets in logs or process arguments.** The VLESS link lives in the OS secure store.
- **An error instead of silence.** An unsupported setting is rejected with a clear
  message, never silently replaced.
- **One UI.** Platform differences live in tokens and adapters, not in copies of screens.
- **No binaries in git.** `wintun.dll`, ready-made builds and the core archive are
  downloaded or built by a script with SHA-256 checks (stage 4).

## Dependencies on the project owner

- **Stage 6:** a test VLESS/REALITY server and real devices; without them "it works"
  cannot be claimed.
- **Stage 11:** real Android devices from multiple manufacturers are needed to
  visually verify cutouts, system bars and gesture navigation. Generic Android
  insets are implemented; OEM checks are not claimed as complete.
- **SmartScreen:** the warning cannot be removed with a client code change alone.
  Trusted signing requires an Authenticode certificate controlled by the owner;
  app reputation may also take time to build after signing.
- **Licence:** the core is GPL-3.0-or-later, so the client is too; the repository is
  published with open sources under the same terms.
- **PR #18:** contains Windows installer packaging and Android safe insets;
  check that Windows CI completes and review the result before merging.
