[Русский](#журнал-изменений) | [English](#changelog)

# Журнал изменений

Формат — [Keep a Changelog](https://keepachangelog.com/ru/1.1.0/). Версии —
[SemVer](https://semver.org/lang/ru/); первый релиз ещё не выпущен.

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
- Иконка приложения; на Linux — привязка окна к `.desktop` (app-id) и установка иконки.

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

### Подготовка к публикации
- Проверена история (97 коммитов) и дерево на секреты и личные данные: настоящих ссылок, ключей и токенов нет, только тестовые заглушки.
- Добавлены `CODE_OF_CONDUCT.md`, `SUPPORT.md`, шаблоны issue и pull request, `CODEOWNERS`, Dependabot; в `Cargo.toml` — лицензия и метаданные.

### Проверки
- Впервые запущены нативная сборка и тесты на Linux (51 тест).

## Предварительные выпуски

`v0.1.0-preview.1` … `preview.8` — предварительные сборки Windows x64 и Android
arm64 (отладочный APK). Подробности — [docs/BUILD-LOG.md](docs/BUILD-LOG.md).

---

# Changelog

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Versions follow
[SemVer](https://semver.org/); the first release has not been cut yet.

## [Unreleased]

### UI
- The UI was rewritten: a global theme with tokens, a set of shared components, one
  adaptive layout (sidebar on desktop, bottom navigation on phones), a power-button
  home screen, a server list, tidier settings.
- Android: Material 3 Expressive styling: tonal surfaces, large corner radii, pill
  buttons, an M3 switch and navigation bar, a power button that changes shape on connect.
- Android 12+: dynamic colours from the Material You system palette
  (`MainActivity.readSystemPalette` + the tested `material.rs` module).
- An app icon; on Linux the window is tied to the `.desktop` entry (app-id) and the
  icon is installed.

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

### Preparing for publication
- The history (97 commits) and the tree were scanned for secrets and personal data: no real links, keys or tokens, only test placeholders.
- Added `CODE_OF_CONDUCT.md`, `SUPPORT.md`, issue and pull request templates, `CODEOWNERS`, Dependabot; licence and metadata in `Cargo.toml`.

### Checks
- Native Linux build and tests were run for the first time (51 tests).

## Pre-releases

`v0.1.0-preview.1` … `preview.8` — pre-release Windows x64 and Android arm64 (debug
APK) builds. Details: [docs/BUILD-LOG.md](docs/BUILD-LOG.md) (Russian).
