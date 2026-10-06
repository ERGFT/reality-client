# Устройство клиента

[← README](../README.md)

## Слои

```text
┌────────────────────────────────────────────────────────┐
│ ui/main.slint            интерфейс (Slint, один на все ОС)│
├────────────────────────────────────────────────────────┤
│ src/lib.rs               состояние, привязки UI ↔ логика  │
│ src/profiles.rs          профили и защищённое хранилище   │
│ src/security.rs          очистка секретов из текстов      │
│ src/core.rs, ffi_*.rs    запуск и управление ядром        │
├────────────────────────────────────────────────────────┤
│ src/windows_proxy.rs     системный прокси Windows         │
│ src/platform.rs          пути, окно, буфер обмена, ОС     │
│ src/android_bridge.rs    JNI-мост к Kotlin                │
│ android/…                Kotlin: VpnService, Activity     │
├────────────────────────────────────────────────────────┤
│ libreality (vpn-core, ffi/)   C ABI: rc_start / rc_request│
└────────────────────────────────────────────────────────┘
```

## Интерфейс

- Один файл `ui/main.slint`. Цвета и размеры — в глобальном `Theme`;
  компоненты (`Card`, `Btn`, `Toggle`, `PowerButton`, `RailItem`, `TabItem`, …)
  используют только его токены.
- Один адаптивный макет: на компьютере — боковая панель, на телефоне —
  нижняя навигация (`mobile-layout`, задаётся Android-входом).
- Интерфейс с Rust — набор `property`/`callback` корневого `MainWindow`.
  Строки состояния (`status-text`, `connect-button-text`) задаёт Rust;
  вёрстка выводит из них состояние кнопки питания.
- Для разработки интерфейса без запуска ядра: `slint-viewer ui/main.slint
  --component MainWindow --load-data demo.json` (JSON с значениями свойств).

## Ядро

vpn-core подключается как библиотека через C ABI (`rc_start`, `rc_request`,
`rc_reload`, `rc_stop`, колбэки событий и журнала, `rc_set_protect` для
Android, `rc_set_lock_dir` для Windows). Клиент загружает её динамически
(`libloading`). Конфигурация — JSON sing-box/Xray; из VLESS-ссылки он
строится в приватном временном файле (`link_file`).

Целевая схема (этап 4 в [PLAN.md](../PLAN.md)): `reality-core` как обычная
зависимость Cargo, без C ABI и без `unsafe`-обёрток на desktop; C ABI
остаётся только для Android, где ядро грузится в процесс с Kotlin-слоем.

## Android

Класс `VpnService` обязан быть на Kotlin/Java: он запрашивает разрешение,
создаёт TUN, держит foreground-уведомление и отдаёт дескриптор ядру.
Остальное — интерфейс, профили и логика — в Rust (`slint` + `android-activity`).
Передача конфигурации — через временный файл в приватном каталоге
приложения, а не через `Intent`.

## Хранение секретов

| ОС | Хранилище |
|---|---|
| Windows | DPAPI (CurrentUser), формат совместим со старой версией на C# |
| Linux | Secret Service (libsecret / D-Bus) |
| Android | AES-GCM + Android Keystore |
