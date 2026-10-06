[Русский](CONTRIBUTING.md) | [English](CONTRIBUTING.en.md)

# Участие в разработке

Содержание:

1. [Сборка и проверки](#1-сборка-и-проверки)
2. [Интерфейс без ядра](#2-интерфейс-без-ядра)
3. [Правила](#3-правила)
4. [Pull request](#4-pull-request)

## 1. Сборка и проверки

```sh
scripts/fetch-core.sh third_party/vpn-core   # ядро — зависимость Cargo; один раз и после смены vpn-core.rev
cd rust-client
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings -A dead_code
cargo test --locked
cargo check --locked --features android-bridge-check   # JNI-мост без Android SDK
```

Зависимости Linux — в [README](README.md#быстрый-старт), пакеты под платформы —
в [docs/PLATFORMS.md](docs/PLATFORMS.md).

## 2. Интерфейс без ядра

```sh
cargo install slint-viewer --version 1.18.1 --locked
slint-viewer rust-client/ui/main.slint --component MainWindow --load-data demo.json
```

В `demo.json` — значения свойств `MainWindow` (например, `"active-tab": 1`,
`"mobile-layout": true`, `"dark-theme": false`). Подробнее — [docs/DESIGN.md](docs/DESIGN.md).

Приложение можно запустить без дисплея: `xvfb-run -a rust-client/target/debug/reality-client-rs`,
снимок — `import -window root shot.png` (ImageMagick).

## 3. Правила

- **Секреты** (VLESS-ссылки, UUID, токены) не пишем в журнал, аргументы процесса
  и сообщения об ошибках.
- **Проверки.** Всё, что вы запускали, записывается в [docs/STATUS.md](docs/STATUS.md)
  (и в английскую версию). То, что не запускалось, называется «не проверено».
- **Документация сразу.** Изменили код, сборку или поведение — в том же PR обновите
  документацию: README, `docs/`, `docs/STATUS.md`, `PLAN.md`, `CHANGELOG.md`.
  Отложенная документация считается незавершённой работой.
- **Двуязычие.** Документ меняется — меняется и его `.en.md`.
- **Цвета и размеры** в интерфейсе берём из `Theme`, а не пишем по месту.
- **Бинарные файлы** в git не добавляем (кроме небольших ресурсов вроде иконки и скриншотов).
- Сообщения коммитов — по существу изменения.

## 4. Pull request

- Описание: что изменено, как проверено, что не проверено.
- CI (GitHub Actions) должен быть зелёным на трёх платформах; локальные проверки из раздела 1 нужны до отправки.
