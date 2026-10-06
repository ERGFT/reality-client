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
| 3 | CI: green builds and checks on three OSes (the repository is public, Actions are free) | ✅ |
| 4 | The core without a zip archive and a Python patch: the TUN-descriptor fix goes into vpn-core itself ([#28](https://github.com/ERGFT/vpn-core/pull/28)), the build fetches the core by commit hash; then a Cargo dependency (`reality-core`) | ⏳ first part in progress |
| 5 | Split `rust-client/src/lib.rs` (~3600 lines) into modules: state, handlers, UI bindings | ⏳ |
| 6 | End-to-end checks: a real REALITY server → Windows, Linux, Android | ⏳ needs a server and devices |
| 7 | Android: move the TUN policy from Kotlin to Rust, keep a minimal `VpnService` shim | ⏳ |
| 8 | Remove C# and PowerShell scripts after parity, replace with `cargo xtask` | ⏳ |
| 9 | First release: signed builds, SHA-256, install and update instructions | ⏳ |
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
