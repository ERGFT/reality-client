# Reality Client

**Русский** | [English](README.en.md)

![License: GPL v3+](https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg)
![Rust 2024](https://img.shields.io/badge/rust-2024%20edition-orange.svg?logo=rust)
![Platforms](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20Android-lightgrey.svg)

Графический VPN-клиент на **Rust + Slint** для сетевого ядра
[**vpn-core**](https://github.com/ERGFT/vpn-core) (VLESS, REALITY, XTLS Vision).
Один интерфейс на все платформы: добавьте ссылку `vless://…`, нажмите
кнопку питания — клиент запустит ядро, включит прокси или TUN и покажет
скорость, трафик и журнал.

## Платформы и статус

| Платформа | Сборка | Что проверено | Чего ещё нет |
|---|---|---|---|
| **Linux x86_64** | `rust-client/build-linux.sh` | сборка и 45 модульных тестов на Ubuntu 24.04; окно запускается и рисуется | трафик через настоящий сервер, TUN, Secret Service на реальном рабочем столе |
| **Windows x64** | `rust-client/build-windows.ps1` | сборка и тесты на машине разработчика (журнал — [BUILD-LOG.md](docs/BUILD-LOG.md)) | системный прокси и TUN на «чистой» машине, трафик через сервер |
| **Android arm64** | `rust-client/build-android.sh` | отладочный APK собирается, запускается в эмуляторе | `VpnService`/TUN на устройстве, трафик через сервер |
| macOS, iOS, tvOS | — | — | отложено, см. [PLAN.md](PLAN.md) |

Подробная матрица проверок, с указанием что и кем проверено, — в
[docs/STATUS.md](docs/STATUS.md). Проект на стадии **предварительных
сборок**: ни одна платформа пока не проверена полным сквозным сценарием
«клиент → настоящий сервер REALITY → сайт».

> [!WARNING]
> Это не замена зрелым клиентам. Стороннего аудита безопасности нет, ядро
> ещё не выпускало первую версию.

## Возможности

- несколько VLESS-профилей; секретная ссылка хранится в защищённом
  хранилище ОС (DPAPI, Secret Service, Android Keystore) и не попадает в журнал;
- проверка конфигурации ядром до запуска, понятные сообщения об ошибках;
- режимы: локальный прокси (SOCKS5/HTTP) и полный конфиг ядра с TUN, DNS
  fake-IP и правилами маршрутизации;
- системный прокси Windows с резервной копией и восстановлением после сбоя;
- Android: `VpnService`, фильтр приложений, передача TUN-дескриптора ядру;
- группы серверов и выбор узла, скорость и трафик, активные соединения;
- тёмная и светлая темы, адаптивный интерфейс для телефона и компьютера.

## Быстрый старт

### Linux

```sh
sudo apt install build-essential cmake nasm pkg-config unzip python3 \
    libdbus-1-dev libfontconfig1-dev libfreetype6-dev \
    libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libx11-dev libgl1-mesa-dev
cd rust-client
./build-linux.sh          # сборка и пакет в dist/linux-x86_64
./install-linux.sh        # установка в ~/.local/opt/reality-client
```

Только интерфейс и тесты, без сборки ядра: `cd rust-client && cargo test --locked`.

### Windows и Android

См. [rust-client/README.md](rust-client/README.md).

## Как это устроено

- **`rust-client/`** — приложение: общий Rust-код, интерфейс Slint, платформенные
  адаптеры и тонкий слой Kotlin для Android `VpnService`.
- **vpn-core** подключается как закреплённая версия (`third_party/`) и
  загружается как библиотека через C ABI. Планируется переход на прямую
  зависимость Cargo — [PLAN.md](PLAN.md).
- **`src/`** — старая версия на C# (архив): [docs/LEGACY-CSHARP.md](docs/LEGACY-CSHARP.md).

Подробнее: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Документация

| Файл | О чём |
|---|---|
| [PLAN.md](PLAN.md) | дорожная карта и этапы |
| [docs/STATUS.md](docs/STATUS.md) | что проверено и что нет |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | устройство клиента |
| [CONTRIBUTING.md](CONTRIBUTING.md) | как собирать, тестировать и вносить изменения |
| [CHANGELOG.md](CHANGELOG.md) | журнал изменений |
| [docs/BUILD-LOG.md](docs/BUILD-LOG.md) | архив: журнал проверок предварительных сборок |

## Лицензия

GPL-3.0-or-later (см. [LICENSE.txt](LICENSE.txt)). Клиент встраивает ядро
под GPL, поэтому распространяется на тех же условиях. Сторонние
компоненты — [THIRD_PARTY.md](THIRD_PARTY.md).
