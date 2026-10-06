# rust-client

Приложение Reality Client: Rust + Slint. Общее описание — в [корневом README](../README.md),
устройство — [docs/ARCHITECTURE.md](../docs/ARCHITECTURE.md).

## Проверки (любой хост)

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings -A dead_code
cargo test --locked
```

## Linux

Зависимости и сборка пакета — в [корневом README](../README.md#linux).

```sh
./build-linux.sh      # dist/linux-x86_64/
./install-linux.sh    # ~/.local/opt/reality-client
```

Для TUN нужны root или `CAP_NET_ADMIN`:
`sudo setcap cap_net_admin+ep "$HOME/.local/opt/reality-client/RealityClient"`.
Не включайте `strict_route`, не понимая поведения kill switch: после аварии
остатки убираются командой ядра `reality-client --tun-cleanup` от root.

## Windows

```powershell
./build-windows.ps1                 # dist\windows-x64\RealityClient-Rust.exe
./tests/windows-test.ps1            # тесты на GNU-тулчейне без MSVC link.exe
```

Для TUN нужен `wintun.dll` рядом с приложением и запуск от администратора.

## Android

Нужны Rust target `aarch64-linux-android`, `cargo-ndk`, JDK 17, Gradle 8.9,
Android SDK platform 35, Build-Tools 35.0.0 и NDK r25+.

```sh
cargo install cargo-ndk
rustup target add aarch64-linux-android
./build-android.sh                  # отладочный APK, arm64-v8a
```

Лицензии Android SDK принимаются вручную. Тесты Kotlin-слоя: `gradle test` в `android/`.

## Закреплённое ядро

`../third_party/vpn-core-source.zip` + `.commit` — версия vpn-core, из которой
собираются скрипты. Скрипты проверяют коммит и SHA-256 и применяют
`patches/apply_core_tun_fd_ownership.py` (владение TUN-дескриптором).
Переход на зависимость Cargo — этап 4 в [PLAN.md](../PLAN.md).

Архив журнала проверок предварительных сборок — [docs/BUILD-LOG.md](../docs/BUILD-LOG.md).
