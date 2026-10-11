# Проверка полной переработки интерфейса

Последнее обновление: 2026-10-11. Полный пакет пока локальный на `rust-rewrite`; проверки старого head PR23 не подтверждают этот пакет.

## Требования и фактические доказательства

| Требование | Доказательство | Состояние |
|---|---|---|
| Пять основных экранов и категории настроек | `main.slint`; host render всех страниц и категорий, обе темы | Матрица обеих тем и 360/412/600 px + landscape прошла |
| Серверы и подписки, независимый черновик импорта | `ui/import.rs`, `ui/subscriptions/tests.rs`; реальные callbacks, атомарность, отдельный loopback TCP | Проверено на host; GUI импорт VLESS/HTTPS проверен на Android16 |
| Поиск сохраняет исходные индексы | `ui/profiles.rs`, host callbacks и поиск с кириллицей | Проверено |
| Подключённый сервер отличается от следующего выбранного | `ui/session_view.rs`, `ui/connection.rs`, `ui/runtime.rs` | Android REALITY, пересоздание Activity и переподключение проверены |
| Скорость, трафик, потоки и журнал | `runtime_stats.rs`, `ui/runtime.rs`; лабораторный payload 262144 байт и реальные счётчики | Проверено в лаборатории; зарубежный IP не проверен |
| Windows Tab/Enter/Space и видимый фокус | Отдельный Windows GUI профиль на D; Enter меняет категорию, Space/Enter переключают полный конфиг | Нативная проверка пройдена; сеть Windows не изменялась |
| Android системные отступы и шрифт | JNI safeDrawingInsets/fontScale; Android16 portrait/landscape, вырез, IME; host render font 1.3 | Проверено на собственном эмуляторе; физические модели не проверены |
| TUN, Fake-IP, приложения | `ui/config_editor.rs`, `config_json.rs`; Android UI выбор Chrome и сохранённый `include_package` | Запись конфига проверена; фактическая фильтрация Chrome и Fake-IP/DNS отдельно не проверены |
| Домен/URL и IP/CIDR маршрутизация | Host callbacks URL vk.com/video и CIDR, сохранение; Android UI vk.com → direct | Запись и валидация проверены; URL-путь не является отдельным HTTP правилом |
| Удаление правил с подтверждением | Android UI подтверждение удаления, сохранение: managed rule count = 0; host сохранение внешних правил | Проверено |
| Редактор JSON не портит сохранённый файл | `with_config_draft`, атомарный rename; тесты неверного JSON, отклонённой проверки и уборки temp | Проверено на host |
| IP диагностика соответствует сессии | Desktop использует CoreSession; Android теперь хранит loopback proxy из реально подготовленного конфига | Исправлено; host snapshot regression, Android build и REALITY payload прошли |
| Версия, лицензия, репозиторий | Значения сборки и callback открытия URL, экран About | Android16: версия/лицензия видны; кнопка запускает браузер с правильным ACTION_VIEW URL. Страница остановилась на первоначальном экране Chrome |
| CI Windows/Linux/Android, слияние PR23 | Требуются проверки точного нового head и merge-коммита | Не выполнено |

## Где находятся результаты

- Host логи: `D:/reality-client-work/tmp/ui-settings-clippy.log`, `ui-config-settings-tests.log`, `ui-final-audit-tests.log`.
- Android настройки: `D:/reality-client-work/ui-final-proof/android16-settings-*.png`, `android16-app-filter-saved.png`, `android16-routes-added.png`, `android16-route-remove-confirm.png`, `android16-route-removed.png`.
- Android REALITY и смена Activity: `ui-final-proof/android16-session-*.png`, `tmp/ui-session-*.log`.
- Own visual references: `docs/design/references/`. Нативные снимки и программный рендер различаются в подписях плана.

APK `d04280509e9fd1753b43e0dccfd5604d9b6db4ab87a196d38370112e28d82abc` прошёл REALITY/Vision, payload SHA256 `2312394bd99545d9de131c24efb781e765ac1aec243f2ed9347597a793a415e9`, пересоздание Activity, поворот, отключение и повторное подключение.
APK `442b187af9ae0f3e2b2c40430e0ff1abb38f35fce5b472cc1cf6f8e33d57b56c` прошёл мобильные настройки, сохранение списка приложений и добавление/удаление маршрутов. Эти APK не выдаются за окончательную сборку после последней правки диагностики.

Happ/happd, настройки сети Windows, защиту и реальные профили пользователя не изменяли. Новый релиз в эту задачу не входит.

## Итоговая локальная сборка

Последний APK SHA256 `389527b6e20c2de2a59f5cf3289c54222d78577f1fcdfb1b667ffc193d88bd70` собран после правки диагностики и установлен с сохранением синтетических профилей. Kotlin unit tests и Android ARM64 build прошли. Контрольный payload и повторное подключение вернули HTTP 200 / 262144 байт с ожидаемым SHA256; после отключения `/exit` недоступен. Счётчики показали 256.1 КиБ полученных данных. Логи: `tmp/ui-final-apk-payload.log`, `ui-final-apk-stopped.log`, `ui-final-apk-reconnect.log`; снимки: `ui-final-proof/android16-final-apk-*.png`.

Полная host/render матрица после последних правок: 127 passed / 1 ignored, `tmp/ui-complete-matrix-tests.log`. Clippy: PASS, `tmp/ui-audit-clippy.log`; Windows build: PASS, `tmp/ui-audit-windows-build.log`. `cargo fmt --check` и `git diff --check` прошли. 128 снимков дополнительного прогона форм сохранены в `ui-final-proof/matrix-forms/`; тест прошёл за 133.31s. Вариант `matrix-final/` покрывает и пояснения недоступного режима.
