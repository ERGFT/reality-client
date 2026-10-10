[Русский](PLAN.md) | [English](PLAN.en.md)

# Plan: a Rust client for vpn-core

## Current task: HTTPS subscriptions

Detailed plan and evidence: [SUBSCRIPTIONS_WORK_PLAN.md](docs/SUBSCRIPTIONS_WORK_PLAN.md).
Implement protected subscription storage, plain/Base64 VLESS imports, server
groups, manual refresh and UI controls. Updates preserve manual profiles,
server selection and a running session. Windows tests and Android Keystore/Slint
runtime checks passed. [PR #19](https://github.com/ERGFT/reality-client/pull/19) is
merged, as is PR20; `.11` is published and main CI is green.
A local HTTPS subscription and two VLESS/TLS servers passed transport/storage
checks. A populated-JSON profile-binding conflict was found and fixed for a new PR.
Guest Windows installer/UI/browser proxy/TUN, separate REALITY and fresh Android
runtime checks remain. The Windows ISO is downloaded and verified; VM installation
awaits the user's license acceptance.

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
| 3 | CI: green builds and checks on three OSes (the repository is public, Actions are free) | ✅ |
| 4 | The core without a zip archive and a Python patch: the TUN-descriptor fix goes into vpn-core itself ([#28](https://github.com/ERGFT/vpn-core/pull/28)), the build fetches the core by commit hash; the core is a Cargo dependency of the client (`reality-ffi`, linked into the app, no `libloading` or separate `.dll`/`.so`) | ✅ Linux verified, Windows/Android in CI |
| 5 | `rust-client/src/lib.rs` (3600 lines) split into modules: `ui/` (handlers by topic, `UiState`) and pure modules with tests; behaviour and tests unchanged | ✅ |
| 6 | End-to-end checks: a real REALITY server → Windows, Linux, Android | ⏳ needs a server and devices |
| 7 | Android: TUN policy moved from Kotlin to Rust (`android_tun.rs`, host tests), `VpnService` is a thin shell | ✅ not checked on a device |
| 8 | PowerShell scripts replaced by `cargo xtask` (`fetch-core`, `core-cli`, `package-windows`, `test-windows`); C# and its scripts go after parity and stage 6 | 🔶 CI builds the Windows package; local `package-windows` built with relative `CARGO_TARGET_DIR`; GNU-toolchain `test-windows` passed (92/95 host tests, both Clippy sets). Target lookup and Clippy argument order fixed. C# stays |
| 9 | First release: signed builds, SHA-256, install and update instructions | 🔶 pre-release `v0.1.0-preview.9` is published: Windows x64, Linux x86_64 and Android ARM64 debug APK, each with SHA-256; signing and update instructions come later. Linux had to be attached manually because checkout removed the downloaded artifact and the workflow ran twice for a tag (`push` + `release`); the step order is fixed and the redundant trigger removed in the local branch, but CI has not verified the changes |
| 10 | Apple: macOS, then iOS/tvOS (Network Extension) | 💤 postponed |

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
- **Licence:** the core is GPL-3.0-or-later, so the client is too; the repository is
  published with open sources under the same terms.
- **The fate of PR #2:** it carries Windows TUN, `wintun.dll` and release workflows;
  decide whether to merge it whole or split it.
