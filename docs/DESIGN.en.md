[Русский](DESIGN.md) | [English](DESIGN.en.md)

# Design system

The UI is a single file, `rust-client/ui/main.slint`. All colours, corner radii
and sizes come from the global `Theme` instead of being written inline.

Contents:

1. [Principles](#1-principles)
2. [Tokens](#2-tokens)
3. [Components](#3-components)
4. [Layout](#4-layout)
5. [Material 3 Expressive on Android](#5-material-3-expressive-on-android)
6. [Dynamic colours (Android)](#dynamic-colours-android)
7. [Previewing without the core](#7-previewing-without-the-core)

<p align="center">
  <img src="images/desktop-settings.png" alt="Settings on desktop" width="640">
</p>

## 1. Principles

- **One UI.** Platform differences live in tokens and layout, not in copies of screens.
- **State from strings.** Rust sets `status-text` and `connect-button-text`; the
  markup derives the power button and indicators from them.
- **No icon fonts.** Icons are vector `Path`s, so they never turn into tofu boxes
  on devices missing a font.
- **Accessibility.** Buttons, toggles and tabs carry `accessible-*` roles and labels.

## 2. Tokens

`Theme` carries colours for four modes: dark and light, regular and M3.

| Token | Purpose |
|---|---|
| `bg` | window background |
| `surface`, `surface-2`, `surface-3` | cards, tiles, hover — three steps |
| `outline` | borders and dividers |
| `text`, `dim`, `faint` | primary, secondary, muted text |
| `accent`, `accent-hover`, `accent-soft`, `on-accent`, `on-accent-container` | primary colour, its container and the text colours on them |
| `warn`, `warn-soft`, `danger`, `danger-soft` | warnings and destructive actions |
| `r-card`, `r-tile`, `r-row`, `r-field` | corner radii |
| `m3`, `dynamic`, `dark` | modes: Material 3, system colours, dark theme |

## 3. Components

| Component | What |
|---|---|
| `Card`, `Stat`, `Notice`, `Chip` | containers and badges |
| `Btn` | button: `kind` 0 primary, 1 tonal, 2 outlined, 3 destructive |
| `Toggle`, `ToggleRow`, `Segmented` | switches and segmented choice |
| `PowerButton` | the main button: 0 off, 1 busy, 2 on |
| `ProfileRow`, `IpRow` | a profile row, an IP-check row |
| `RailItem`, `TabItem` | navigation: sidebar and bottom bar |
| `Icon` | 24×24 vector icon from an SVG path |

## 4. Layout

- **Desktop:** a 236 px sidebar, centred content with a 680 px maximum column
  width, minimum window size 760×560.
- **Phone:** bottom navigation (M3 navigation bar), 16 px margins, a top inset
  for the status bar.
- Selected by the `mobile-layout` property, which the Android entry point sets.
  Layout does not depend on the current window width, which avoids a size binding loop.

## 5. Material 3 Expressive on Android

With `mobile-layout`, `Theme.m3` turns on:

- tonal surfaces instead of borders (`surface` → `surface-2` → `surface-3`);
- large corner radii: cards 28 px, tiles 24 px, rows 20 px;
- pill buttons 48 px tall;
- a 52×32 switch whose thumb grows and moves when turned on;
- a navigation bar with a 64×32 pill indicator and 12 px labels;
- a power button that changes shape: a rounded square at rest, a circle when
  connected (280 ms animation).

## Dynamic colours (Android)

On Android 12 and later, `MainActivity.readSystemPalette()` returns the system
tonal palette (`system_accent1_*`, `system_neutral1_*`, `system_neutral2_*`) as
`key=AARRGGBB,…`. The `material.rs` module (unit-tested) maps it to tokens:

| Token | Dark theme | Light theme |
|---|---|---|
| background | neutral1 900 | neutral1 10 |
| accent | accent1 200 | accent1 600 |
| accent container | accent1 700 | accent1 100 |
| text on accent | accent1 800 | accent1 0 |
| primary text | neutral1 100 | neutral1 900 |
| secondary text | neutral2 200 | neutral2 700 |
| outlines | neutral2 700 | neutral2 200 |

Surfaces are intermediate tones between neighbouring neutral1 steps. If the
palette is unavailable (Android below 12, a read error) or incomplete, the built-in
teal tonal theme stays in place.

> [!NOTE]
> Reading the palette on a device has not been verified: the APK build and a run on
> Android 12+ remain open in [STATUS.en.md](STATUS.en.md). The colour mapping is
> covered by unit tests.

## 7. Previewing without the core

```sh
cargo install slint-viewer --version 1.18.1 --locked
slint-viewer rust-client/ui/main.slint --component MainWindow --load-data demo.json
```

`demo.json` holds `MainWindow` property values, for example:

```json
{ "mobile-layout": true, "dark-theme": false, "active-tab": 1,
  "status-text": "VPN подключён", "connect-button-text": "Отключить" }
```

Global `Theme` tokens cannot be set from JSON; dynamic colours can only be
checked on a device.
