[Русский](PLATFORMS.md) | [English](PLATFORMS.en.md)

# Платформы: сборка, запуск, права

Содержание:

1. [Linux](#1-linux)
2. [Windows](#2-windows)
3. [Android](#3-android)
4. [Режимы подключения и права](#4-режимы-подключения-и-права)
5. [Общие проверки](#5-общие-проверки)

> [!WARNING]
> Android 16 проверен через независимый локальный Xray: HTTPS-подписка,
> VLESS/REALITY/Vision, TCP-трафик, отключение и повторное подключение.
> Физические телефоны, удалённый публичный выход и UDP/DNS этим не проверены.
> Полные границы проверки — в [STATUS.md](STATUS.md) и [отчёте API36](ANDROID16_RUNTIME_REPORT.md).

## 1. Linux

**Зависимости** (Debian/Ubuntu):

```sh
sudo apt install build-essential cmake nasm pkg-config git python3 \
    libdbus-1-dev libfontconfig1-dev libfreetype6-dev \
    libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libx11-dev libgl1-mesa-dev
```

Нужен Rust stable (редакция 2024). Для хранения профилей нужен запущенный
Secret Service (GNOME Keyring, KWallet).

**Сборка и установка:**

```sh
cd rust-client
./build-linux.sh          # dist/linux-x86_64/ — сборка проверяет закреплённое ядро
./install-linux.sh        # ~/.local/opt/reality-client, ярлык и иконка пользователя
```

Скрипт сборки скачивает vpn-core ровно по хешу коммита из `third_party/vpn-core.rev`
в `third_party/vpn-core/` (`cargo xtask fetch-core`), собирает консольную программу ядра
и интерфейс; само ядро линкуется в `RealityClient` как зависимость Cargo (отдельной
`libreality.so` нет). Чтобы собрать с локальной копией ядра:
`REALITY_CORE_URL=/путь/к/vpn-core ./build-linux.sh` (каталог `third_party/vpn-core` при
этом должен быть пуст или отсутствовать).

**Только тесты клиента:** сначала `cargo xtask fetch-core`, затем
`cargo test --locked` в `rust-client/` (ядро компилируется вместе с клиентом; нужны `cmake`
и `nasm`, как для `aws-lc`).

**TUN** требует `root` или `CAP_NET_ADMIN`:

```sh
sudo setcap cap_net_admin+ep "$HOME/.local/opt/reality-client/RealityClient"
```

Повторяйте после замены бинарника. Не включайте `strict_route`, не понимая
поведения kill switch: после аварии остатки убираются командой ядра
`reality-client --tun-cleanup` от root.

## 2. Windows

**Кросс-сборка с Linux** (проверка компиляции без Windows): `rustup target add x86_64-pc-windows-gnu`, `apt install gcc-mingw-w64-x86-64`, затем
`CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc cargo build --locked --release --target x86_64-pc-windows-gnu`.

**Зависимости:** Rust (`stable-x86_64-pc-windows-gnu`), MSYS2 MinGW-w64 GCC
(путь по умолчанию `%LOCALAPPDATA%\Programs\msys64\mingw64\bin`, переопределяется
`REALITY_MINGW_BIN`).

```powershell
cargo xtask core-cli                     # скачивает ядро по хешу коммита в third_party\vpn-core и собирает rust-client\third_party\reality-client.exe
cargo xtask package-windows              # ядро + клиент + пакет: rust-client\dist\windows-x64\RealityClient-Rust.exe
cargo xtask test-windows                 # тесты и Clippy на GNU-тулчейне, без MSVC link.exe
```

Пакет содержит `RealityClient-Rust.exe` (ядро внутри него) и `third_party\reality-client.exe`
(обе части собраны из одного коммита ядра), `wintun.dll` (официальный, проверяется по
SHA-256), лицензии и `CORE-SOURCE.txt` — адрес и коммит исходников ядра (GPL).

**Системный прокси** включается только в режиме профиля и возвращает прежние
значения при отключении. Резервная копия хранится на диске; после аварийного
завершения клиент предлагает восстановление.

**TUN** требует запуска от имени администратора и `wintun.dll` рядом с
приложением. Не держите весь интерфейс постоянно с повышенными правами без нужды.

## 3. Android

**Зависимости:** Rust target `aarch64-linux-android` (для эмулятора x86_64 —
`x86_64-linux-android`), `cargo-ndk`, JDK 17, Gradle 8.9, Android SDK platform 35,
Build-Tools 35.0.0, NDK r25+.

```sh
cargo install cargo-ndk
rustup target add aarch64-linux-android
cd rust-client
./build-android.sh                       # отладочный APK, arm64-v8a
ANDROID_ABI=x86_64 ./build-android.sh    # для эмулятора
```

Лицензии Android SDK принимаются вручную. Тесты Kotlin-слоя — `gradle test` в
`rust-client/android/`.

- Установка APK сама VPN не включает: Android запрашивает разрешение отдельно.
- Минимальная версия Android — 8.0 (API 26). Динамические цвета Material You —
  с Android 12 (API 31); на более старых версиях используется встроенная палитра.
- Служба объявлена как `systemExempted`, как для VPN-приложений.

## 4. Режимы подключения и права

| Режим | Что делает | Права |
|---|---|---|
| Профиль (прокси) | ядро слушает `127.0.0.1:1080` (SOCKS5/HTTP); на Windows можно включить системный прокси | обычные |
| Полный конфиг, без TUN | любой конфиг sing-box/Xray без `tun`-входа | обычные |
| Полный конфиг с TUN, Windows | Wintun, маршруты `/1`; kill switch при `strict_route` | администратор |
| Полный конфиг с TUN, Linux | TUN-устройство и маршруты ядра | `root` или `CAP_NET_ADMIN` |
| Android | `VpnService` отдаёт ядру TUN-дескриптор | разрешение VPN от пользователя |

На Android неподдерживаемые параметры маршрутизации (исключения маршрутов,
несколько `tun`-входов и т. п.) отклоняются до создания интерфейса.

## 5. Общие проверки

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings -A dead_code
cargo test --locked
cargo check --locked --features android-bridge-check    # JNI-мост без Android SDK
```

Подробности и правила внесения изменений — [CONTRIBUTING.md](../CONTRIBUTING.md).
