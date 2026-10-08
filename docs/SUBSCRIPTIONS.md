# HTTPS-подписки / HTTPS subscriptions

## Проверка Android / Android runtime regression

Функция Cargo `subscription-device-check` выключена по умолчанию. Она запускает
проверку настоящих обработчиков Slint и Android Keystore только если приватный
каталог принадлежит `com.ergft.realityclient.subscriptiontest`. Обычные сборки
этот код не включают. Используются синтетические серверы `.invalid`; VPN не
запускается. Результат — `files/subscription-device-check.json` тестового APK.

The `subscription-device-check` Cargo feature is disabled by default. It runs
real Slint callbacks and Android Keystore checks only inside the isolated
`com.ergft.realityclient.subscriptiontest` app sandbox. Normal builds exclude
the driver. It uses synthetic `.invalid` servers, never starts VPN and writes
`files/subscription-device-check.json` inside the test APK sandbox.

Build the x86_64 native library with `cargo ndk -t x86_64 --platform 26 -o
android/app/build/generated/jniLibs build --locked --lib --features
subscription-device-check`. Package a debug APK with an external Gradle init
script (`gradle -I isolated.gradle -p android :app:assembleDebug`):

```groovy
allprojects {
    afterEvaluate { project ->
        if (project.plugins.hasPlugin('com.android.application')) {
            project.android.defaultConfig.applicationId = 'com.ergft.realityclient.subscriptiontest'
        }
    }
}
```

Install only onto the designated emulator, launch
`com.ergft.realityclient.subscriptiontest/com.ergft.realityclient.MainActivity`,
then read the report with `adb -s <emulator> shell run-as
com.ergft.realityclient.subscriptiontest cat files/subscription-device-check.json`.
`PASS` covers add/select/reorder/rename/delete, restart reads of protected URL
and server, stable IDs and unchanged manual vault. It does not cover native
keyboard/clipboard input, provider compatibility or remote VPN traffic.

## Русский

TRACE-логирование зависимостей через `log` отключено при сборке: ureq на этом
уровне раскрывает URL. Это действует и при `RUST_LOG=trace`. Прямые события
ядра через `tracing` не ограничиваются этой настройкой.

В исходниках клиента добавлен импорт подписок; опубликованные сборки могут
ещё не содержать эту функцию. Состояние проверки — в
[плане реализации](SUBSCRIPTIONS_WORK_PLAN.md).

На странице **Серверы → Подписки** введите название и HTTPS-ссылку, затем
нажмите **Добавить**. Ссылка скрыта; кнопка **Вставить** читает её из буфера
обмена. Выберите сервер в общем списке: его имя начинается с названия подписки.

Выпадающий список подписок позволяет выбрать группу для **Обновить**,
**Переименовать** и **Удалить подписку**. Обновление использует сохранённую
ссылку; поле HTTPS предназначено для добавления новой подписки. Удаление
требует подтверждения. Для отдельной VLESS-ссылки используйте **Новый профиль**.

### Форматы и ограничения

- UTF-8 список `vless://…`, одна ссылка на строку; пустые строки и строки
  комментариев, начинающиеся с `#`, пропускаются. Поддерживается UTF-8 BOM.
- Base64 такого списка: обычный или URL-safe алфавит, с padding или без него.
  Переносы строк между блоками Base64 разрешены.
- Имена из `#fragment` декодируются, включая кириллицу. При отсутствии имени
  используется адрес узла. Повторяющиеся подключения с разными именами
  импортируются один раз.
- JSON, YAML/Clash и HTML не импортируются. Другие протоколы, включая Trojan
  и VMess, показываются как отклонённые записи: импорт профилей этого клиента
  сейчас поддерживает VLESS. Это не утверждение об ограничениях самого ядра.
- VLESS проверяется парсером закреплённой версии ядра: UUID, параметры
  транспорта, flow и REALITY. Успешный импорт не доказывает доступность сервера.
- HTTPS с проверкой сертификата, не более 3 перенаправлений, запрет перехода
  на HTTP, 30 секунд на запрос и чтение ответа, до 2 МиБ ответа.
- До 2000 записей во входном списке, до 1000 серверов суммарно в подписках,
  до 50 подписок; отдельная ссылка — до 16 КиБ. Итоговый файл метаданных
  ограничен 32 МиБ. Ссылки с userinfo и `#fragment` для адреса подписки отвергаются.

### Обновление и хранение

Обновление ручное. При ошибке загрузки, пустом ответе или отсутствии
поддерживаемых серверов предыдущий список сохраняется. При частично корректном
списке сохраняются корректные серверы, а причины пропуска показываются в отчёте.
Выбор сервера сохраняется, если параметры подключения не изменились; имя и
порядок строк могут меняться. При удалении выбранного сервера нужно выбрать новый.

Обновление и удаление подписки не останавливают и не перезагружают запущенное
ядро. Новые параметры используются при следующем подключении.

Адрес подписки и серверные ссылки защищены Windows DPAPI, Linux Secret Service
или Android Keystore. Метаданные и ссылки на секреты записываются атомарно в
`subscriptions.json` рядом с `profiles.dat`. Названия, идентификаторы и состояние
обновления не шифруются. Старый формат ручных профилей RCLIENT1 не меняется.
Повреждённый файл подписок не должен блокировать чтение ручных профилей.
Не отправляйте полную ссылку подписки в отчёты об ошибках.

## English

Dependency TRACE events through `log` are disabled at compile time because
ureq reveals URI paths/queries at that level, including with `RUST_LOG=trace`.
The core's direct `tracing` events are not capped by this setting.

Subscription support is present in the source; published builds may not yet
include it. See the [implementation plan](SUBSCRIPTIONS_WORK_PLAN.md) for
verification status.

On **Servers → Subscriptions**, enter a name and an HTTPS URL, then choose Add.
Paste reads the URL from the clipboard; the field is masked. Server names in
the shared list are prefixed with their subscription name. Select a subscription
to refresh, rename or delete it. Refresh uses its stored URL. Use New profile
for an independent manual VLESS server.

Supported inputs are UTF-8 VLESS URI lists and their standard/URL-safe Base64
encoding, padded or unpadded. Empty lines, `#` comments and UTF-8 BOM are accepted.
JSON, Clash/YAML and HTML are unsupported. Unsupported URI protocols and invalid
core parameters are reported without echoing secret input. Import does not
prove that a server can be reached.

Downloads verify HTTPS certificates, reject HTTP redirects, allow up to three
redirects, have a 30-second total timeout and a 2 MiB body limit. Limits:
2000 input records, 1000 subscription servers in total, 50 subscriptions,
16 KiB per URI, 32 MiB metadata file. Subscription URLs containing userinfo or
a fragment are rejected.

Refresh is manual. Failed/empty updates retain the old group. Partially valid
updates keep valid entries and report skipped records. Stable connection
identities preserve selection across renames/reordering. Updating or deleting
a group never stops/reloads a running core; new parameters apply on reconnect.

URLs and server links use existing platform secret protection. Atomic
`subscriptions.json` stores protected blobs/references, names, IDs and update
status beside the unchanged legacy `profiles.dat`. Metadata is not encrypted.
Never include a personal subscription URL in an issue or log attachment.
