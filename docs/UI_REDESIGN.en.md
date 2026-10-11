[Русский](UI_REDESIGN.md) | [English](UI_REDESIGN.en.md)

# Reality Client UI refresh: 2026-10-11

The owner requested a Clash Verge Rev inspired interface without copying the
whole application. Existing Rust/Slint, core integration and platforms remain.

## Changes

- Compact desktop sidebar, persistent page header and quick theme switch.
- Tonal surfaces and a connection dashboard card.
- Separate local server/subscription views with contextual add actions,
  VLESS/HTTPS placeholders and explicit empty states.
- Import still detects the link format. Navigation does not change the selected
  profile, a running connection or an existing import draft.
- Android retains Material You, bottom navigation and platform safe insets.

## Design source

[Clash Verge Rev](https://github.com/clash-verge-rev/clash-verge-rev) is the
navigation/profile organization reference (GPL-3.0). This Slint implementation
copies no upstream React/TypeScript code, logos or assets. It is not a Tauri
migration or a Clash Verge fork; Reality Client remains GPL-3.0-or-later.

## Verification

Windows GNU build passed; 120 regular and 123 Android bridge host tests passed,
one diagnostic ignored. CI-style Android bridge Clippy passed. The existing
model test was extended to render both themes and desktop/mobile widths, and
assert stable selection across view switches. Native Windows home, theme and
subscription input controls were inspected with an isolated empty profile on D.

No host VPN was started and Happ/network settings were unchanged. New Android
APK/runtime, physical phones and TUN were not tested in this UI iteration.
GitHub CI must finish before merge. All files/caches are on D.

## Next iterations

Owner feedback, search/filter with stable server identities, clearer routing
editing and server details. Never invent latency, countries or IP addresses.
