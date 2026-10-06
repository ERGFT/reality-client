[Русский](CONTRIBUTING.md) | [English](CONTRIBUTING.en.md)

# Contributing

Contents:

1. [Build and checks](#1-build-and-checks)
2. [UI without the core](#2-ui-without-the-core)
3. [Rules](#3-rules)
4. [Pull requests](#4-pull-requests)

## 1. Build and checks

```sh
scripts/fetch-core.sh third_party/vpn-core   # the core is a Cargo dependency; once, and after vpn-core.rev changes
cd rust-client
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings -A dead_code
cargo test --locked
cargo check --locked --features android-bridge-check   # JNI bridge without an Android SDK
```

Linux dependencies are in the [README](README.en.md#quick-start); per-platform
packages are in [docs/PLATFORMS.en.md](docs/PLATFORMS.en.md).

## 2. UI without the core

```sh
cargo install slint-viewer --version 1.18.1 --locked
slint-viewer rust-client/ui/main.slint --component MainWindow --load-data demo.json
```

`demo.json` holds `MainWindow` property values (for example `"active-tab": 1`,
`"mobile-layout": true`, `"dark-theme": false`). More: [docs/DESIGN.en.md](docs/DESIGN.en.md).

The app can run headless: `xvfb-run -a rust-client/target/debug/reality-client-rs`,
and a screenshot comes from `import -window root shot.png` (ImageMagick).

## 3. Rules

- **Secrets** (VLESS links, UUIDs, tokens) never go into logs, process arguments or
  error messages.
- **Checks.** Everything you ran is recorded in [docs/STATUS.en.md](docs/STATUS.en.md)
  (and the Russian page). Anything not run is called "not verified".
- **Docs right away.** If you change code, the build or behaviour, update the docs in the
  same PR: README, `docs/`, `docs/STATUS.en.md`, `PLAN.en.md`, `CHANGELOG.md`.
  Docs left for later count as unfinished work.
- **Two languages.** When a document changes, so does its `.en.md`.
- **Colours and sizes** in the UI come from `Theme`, not written inline.
- **Binary files** are not added to git (except small assets such as the icon and screenshots).
- Commit messages describe what the change does.

## 4. Pull requests

- Description: what changed, how it was checked, what was not.
- CI (GitHub Actions) has to be green on all three platforms; the local checks from section 1 come before you push.
