[Русский](#журнал-изменений) | [English](#changelog)

# Журнал изменений

Формат — [Keep a Changelog](https://keepachangelog.com/ru/1.1.0/). Версии —
[SemVer](https://semver.org/lang/ru/); первый стабильный релиз ещё не выпущен.

## [Не выпущено]

### Интерфейс
- Интерфейс переписан: глобальная тема с токенами, набор общих компонентов, один
  адаптивный макет (боковая панель на компьютере, нижняя навигация на телефоне),
  главный экран с кнопкой питания, серверы списком, упорядоченные настройки.
- Android: стиль Material 3 Expressive — тональные поверхности, крупные
  скругления, кнопки-«таблетки», переключатель и навигация M3, кнопка питания
  меняет форму при подключении.
- Android 12+: динамические цвета из системной палитры Material You
  (`MainActivity.readSystemPalette` + модуль `material.rs` с тестами).
- Android 15+: безопасные отступы для строки состояния, выреза камеры и жестовой навигации; внутренний верхний отступ уменьшен.
- Иконка приложения; на Linux — привязка окна к `.desktop` (app-id) и установка иконки.

### Windows
- Вместо ZIP релиз собирает единый GUI-установщик `.exe` с файлами клиента и ярлыком в меню «Пуск».
- Windows GUI-клиент собирается как Windows-приложение без консольного окна.

### Исправлено
- `cargo xtask test-windows` передаёт флаги Cargo для Clippy до `--`, а lint-флаги — после него.
- `cargo xtask package-windows` берёт клиентский EXE из настроенного `CARGO_TARGET_DIR`, включая абсолютный и относительный путь.

### Документация
- README, дорожная карта, статус проверок, архитектура, дизайн-система, инструкции
  по платформам, правила участия и политика безопасности — в двух языках.
- Старые журналы перенесены в `docs/` как архив.

### Структура кода
- `rust-client/src/lib.rs` (3600 строк) разнесён: обработчики окна — в `src/ui/` по темам
  (`appearance`, `diagnostics`, `config_editor`, `profiles`, `connection`, `groups`,
  `runtime`) с общим `UiState`; чистая логика с тестами — в `config_json`, `server_info`,
  `clipboard`, `runtime_stats`. Поведение и набор тестов не менялись.

### Сборка
- Ядро больше не лежит в git архивом с Python-патчем: хеш коммита vpn-core записан
  в `third_party/vpn-core.rev`, `scripts/fetch-core.{sh,ps1}` скачивают ровно этот
  коммит. Исправление владения TUN-дескриптором внесено в vpn-core
  ([#28](https://github.com/ERGFT/vpn-core/pull/28)). Windows-пакет собирает и
  `reality-client.exe` из той же версии и содержит `CORE-SOURCE.txt` (адрес и коммит
  исходников ядра).

- Ядро — зависимость Cargo клиента (`reality-ffi` по пути `third_party/vpn-core/ffi`,
  каталог скачивается `scripts/fetch-core.*` и лежит вне git): линкуется в само
  приложение, `libloading` и отдельные `reality.dll`/`libreality.so` убраны (Android:
  `libreality.so` из APK пропала, JNI-вызов `nativeStart` без пути к библиотекам).
  `[patch.crates-io]` ядра (патченный `rustls`, `smoltcp`) повторён в `Cargo.toml`
  клиента. Отдельно по-прежнему собирается консольная программа ядра `reality-client`.
  Тест запуска ядра в процессе теперь идёт и на Linux. `fetch-core.*` не трогает
  уже скачанную чистую копию нужной версии.

- Политика Android-TUN перенесена из Kotlin в Rust (`android_tun.rs`): проверка
  параметров, фильтр приложений, адреса, адрес DNS и маршруты считаются в Rust с
  20 тестами на хосте; `RealityVpnService` вызывает `nativePlanTun` и только применяет
  план к `VpnService.Builder`. `AndroidTunPolicy.kt`, `TunAddress.kt` и их тесты удалены.
  Строже прежнего: логические флаги и MTU не приводятся молчаливо из строк, числа и
  адреса разбираются стандартным разбором Rust (например, `010.0.0.1` отклоняется).

- `cargo xtask` (каталог `xtask/`, алиас в `.cargo/config.toml`) заменил PowerShell и bash-скачивание
  ядра: `fetch-core`, `core-cli`, `package-windows` (включая ярлык `.lnk` через COM и проверку SHA-256
  Wintun), `test-windows`. Удалены `scripts/fetch-core.{sh,ps1}`, `build_rust_core.ps1`,
  `rust-client/build-windows.ps1`, `rust-client/tests/windows-test.ps1`; Linux/Android-скрипты и CI
  вызывают `cargo xtask fetch-core`. Прежняя версия на C# и её скрипты пока остаются: их уберём после
  паритета и сквозной проверки (этап 6).

- Выпуск по тегу: тег `v*` запускает сборку на трёх платформах, и CI публикует в Releases
  `RealityClient-Rust-windows-x64.zip`, отладочный APK и архив Linux с файлами SHA-256
  (`.github/scripts/publish-release.sh`; тег с дефисом — пре-релиз). Заметки первой версии —
  `docs/releases/v0.1.0-preview.1.md`, порядок выпуска — `docs/RELEASING.md`. Сборки не подписаны.

### Подготовка к публикации
- Проверена история (97 коммитов) и дерево на секреты и личные данные: настоящих ссылок, ключей и токенов нет, только тестовые заглушки.
- Добавлены `CODE_OF_CONDUCT.md`, `SUPPORT.md`, шаблоны issue и pull request, `CODEOWNERS`, Dependabot; в `Cargo.toml` — лицензия и метаданные.

### Проверки
- Впервые запущены нативная сборка и тесты на Linux (51 тест).

## Предварительные выпуски

`v0.1.0-preview.1` … `preview.9` — предварительные сборки Windows x64, Android
arm64 (отладочный APK) и Linux x86_64. Подробности — [docs/BUILD-LOG.md](docs/BUILD-LOG.md)
и [v0.1.0-preview.9](https://github.com/ERGFT/reality-client/releases/tag/v0.1.0-preview.9).

---

# Changelog

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Versions follow
[SemVer](https://semver.org/); the first stable release has not been cut yet.

## [Unreleased]

### UI
- The UI was rewritten: a global theme with tokens, a set of shared components, one
  adaptive layout (sidebar on desktop, bottom navigation on phones), a power-button
  home screen, a server list, tidier settings.
- Android: Material 3 Expressive styling: tonal surfaces, large corner radii, pill
  buttons, an M3 switch and navigation bar, a power button that changes shape on connect.
- Android 12+: dynamic colours from the Material You system palette
  (`MainActivity.readSystemPalette` + the tested `material.rs` module).
- Android 15+: safe drawing insets for the status bar, display cutout, and gesture navigation; reduced the extra top gap.
- An app icon; on Linux the window is tied to the `.desktop` entry (app-id) and the
  icon is installed.

### Windows
- Replace the ZIP download with one GUI installer `.exe` that installs the client bundle and adds a Start menu shortcut.
- Build the Windows GUI as a Windows-subsystem application so starting it does not open a console window.

### Fixed
- `cargo xtask test-windows` passes Cargo flags for Clippy before `--` and lint flags after it.
- `cargo xtask package-windows` reads the client executable from the configured `CARGO_TARGET_DIR`, including absolute and relative paths.

### Documentation
- README, roadmap, verification status, architecture, design system, platform guides,
  contribution rules and the security policy, in two languages.
- Old logs moved to `docs/` as an archive.

### Code structure
- `rust-client/src/lib.rs` (3600 lines) was split: window handlers live in `src/ui/` by
  topic (`appearance`, `diagnostics`, `config_editor`, `profiles`, `connection`, `groups`,
  `runtime`) with a shared `UiState`; pure logic with tests lives in `config_json`,
  `server_info`, `clipboard`, `runtime_stats`. Behaviour and the test set are unchanged.

### Build
- The core is no longer a zip archive with a Python patch in git: the vpn-core commit
  hash is recorded in `third_party/vpn-core.rev` and `scripts/fetch-core.{sh,ps1}` fetch
  exactly that commit. The TUN-descriptor ownership fix went into vpn-core
  ([#28](https://github.com/ERGFT/vpn-core/pull/28)). The Windows package also builds
  `reality-client.exe` from the same version and carries `CORE-SOURCE.txt` (the core's
  source repository and commit).

- The core is a Cargo dependency of the client (`reality-ffi` at `third_party/vpn-core/ffi`,
  the directory is fetched by `scripts/fetch-core.*` and kept out of git): it is linked into
  the application itself, `libloading` and the separate `reality.dll`/`libreality.so` are
  gone (Android: `libreality.so` is no longer in the APK, the `nativeStart` JNI call has no
  library-directory argument). The core's `[patch.crates-io]` (patched `rustls`, `smoltcp`)
  is repeated in the client's `Cargo.toml`. The core's command-line program `reality-client`
  is still built separately. The in-process core start-up test now runs on Linux too.
  `fetch-core.*` leaves an already fetched clean copy of the right version alone.

- The Android TUN policy moved from Kotlin to Rust (`android_tun.rs`): option validation,
  the app filter, addresses, the DNS address and routes are computed in Rust with 20 host
  tests; `RealityVpnService` calls `nativePlanTun` and only applies the plan to
  `VpnService.Builder`. `AndroidTunPolicy.kt`, `TunAddress.kt` and their tests are removed.
  Stricter than before: boolean flags and MTU are not silently coerced from strings, and
  numbers and addresses use Rust's standard parsing (for example `010.0.0.1` is rejected).

- `cargo xtask` (the `xtask/` directory, alias in `.cargo/config.toml`) replaces PowerShell and the
  bash core download: `fetch-core`, `core-cli`, `package-windows` (including the `.lnk` shortcut via
  COM and the Wintun SHA-256 check), `test-windows`. Removed `scripts/fetch-core.{sh,ps1}`,
  `build_rust_core.ps1`, `rust-client/build-windows.ps1`, `rust-client/tests/windows-test.ps1`; the
  Linux/Android scripts and CI call `cargo xtask fetch-core`. The previous C# version and its scripts
  stay for now: they go after parity and the end-to-end check (stage 6).

- Releasing by tag: a `v*` tag starts the builds on three platforms, and CI publishes
  `RealityClient-Rust-windows-x64.zip`, a debug APK and the Linux archive to Releases with SHA-256
  files (`.github/scripts/publish-release.sh`; a tag with a hyphen is a pre-release). The notes of
  the first version are `docs/releases/v0.1.0-preview.1.md`, the procedure is `docs/RELEASING.en.md`.
  The builds are unsigned.

### Preparing for publication
- The history (97 commits) and the tree were scanned for secrets and personal data: no real links, keys or tokens, only test placeholders.
- Added `CODE_OF_CONDUCT.md`, `SUPPORT.md`, issue and pull request templates, `CODEOWNERS`, Dependabot; licence and metadata in `Cargo.toml`.

### Checks
- Native Linux build and tests were run for the first time (51 tests).

## Pre-releases

`v0.1.0-preview.1` … `preview.9` — pre-release Windows x64, Android arm64 (debug
APK), and Linux x86_64 builds. Details: [docs/BUILD-LOG.md](docs/BUILD-LOG.md) (Russian)
and [v0.1.0-preview.9](https://github.com/ERGFT/reality-client/releases/tag/v0.1.0-preview.9).
