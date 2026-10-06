# Участие в разработке

## Сборка и проверки

```sh
cd rust-client
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings -A dead_code
cargo test --locked
cargo check --locked --features android-bridge-check   # JNI-мост без Android SDK
```

Зависимости Linux — в [README](README.md#linux). Сборка пакетов:
`build-linux.sh`, `build-windows.ps1`, `build-android.sh`.

## Интерфейс без ядра

```sh
cargo install slint-viewer --version 1.18.1 --locked
slint-viewer rust-client/ui/main.slint --component MainWindow --load-data demo.json
```

В `demo.json` — значения свойств `MainWindow` (например, `"active-tab": 1`,
`"mobile-layout": true`, `"dark-theme": false`).

## Правила

- Секреты (VLESS-ссылки, UUID, токены) не пишем в журнал, аргументы
  процесса и сообщения об ошибках.
- Любая проверка, которую вы запускали, записывается в
  [docs/STATUS.md](docs/STATUS.md). То, что не запускалось, называется
  «не проверено».
- Бинарные файлы в git не добавляем.
- Сообщения коммитов — по существу изменения.
