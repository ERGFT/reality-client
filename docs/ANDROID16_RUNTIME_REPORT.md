# Android 16 runtime verification (2026-10-10)

## Environment

- Official Google APIs Android 16 / API 36 image, x86_64 with ARM64 translation.
- Separate AVD `RealityClient36Release11`, emulator-5584; all new host artifacts on D.
- Exact published `v0.1.0-preview.11` ARM64 APK SHA256: `E6E79C78AD295BCFC838255D4105A873E7EBD7868282A67C9F2ADD5ACCBB7083`.
- Independent official Xray 26.3.27 REALITY/Vision server running locally; synthetic credentials only.
- HTTPS subscription uses httpbingo's echo endpoint with synthetic links to this server. It is not a remotely hosted VPS.
- Host Happ, proxy, DNS and routing were not modified. Android VPN permission was explicitly authorized by the user.

## Confirmed on the published APK

1. Installation and cold launch succeed on API 36.
2. HTTPS subscription import succeeds: one server, zero rejected, zero duplicates.
3. Selected subscription server persists through app process restart using Android Keystore.
4. VPN creates `tun0` with IPv4 and IPv6 addresses.
5. A request to reserved test destination `192.0.2.123:80` returns `LAB-EXIT-VERIFIED`. Xray logs the accepted destination on the restricted lab outbound. The destination does not return the marker without VPN.
6. A 262144-byte payload passes through the VPN. SHA256 `2312394bd99545d9de131c24efb781e765ac1aec243f2ed9347597a793a415e9` matches the fixture.
7. Disconnect removes `tun0`; the same test destination fails again.

## Reproduced defects and fixes under test

- After disconnect, the UI can still show connected. A poll racing the asynchronous stop restored the disconnect button, so the subsequent idle state was ignored. Preserve stopping UI while the native session remains active and reset any formerly connected UI when native state is idle; this also handles external session termination.
- Android traffic and connection UI reads the empty desktop CoreSession slot. Read a snapshot from the live Android session under its existing mutex and reuse the desktop snapshot parser. The lock protects FFI handle lifetime against concurrent stop.
- Added regression coverage for stopped/active/pending/error UI state and callback-based stats parsing/error propagation.

## Evidence outside the repository

`D:\reality-client-work\vm-lab\android`: android16-subscription-import.png, android16-vpn-connected.png, android16-vpn-exit.log, android16-vpn-payload.log, android16-disconnect-baseline.log, android16-regression-tests.log.

## Remaining checks

- Install and repeat runtime checks on the newly built APK containing these fixes; verify visible counters and disconnect/reconnect.
- Test subscription refresh, coexistence with manual profiles and negative server credentials/unavailability on API 36.
- Verify physical ARM64 phones, camera cutouts, font scaling, landscape, real remote provider exit and UDP/DNS behavior separately. Emulator TCP lab results do not prove these.
- Create a NEW PR on the existing branch; merge and release only after required GitHub checks pass. No new release is verified or published by this report.

## Final source verification

- Final local ARM64 debug APK (debug symbols removed) SHA256: 08A174990C488B931D8BDC91E7DEF2F307847D0C8B1BD2AB45A088F2273337A4. Size 184300853 bytes. Installed by update with the same local signing key; selected profile survived.
- Repeated native payload test on this final APK: HTTP 200, 262144 bytes, expected SHA256. Counters now display received traffic. Prior corrected build also passed UI disconnect and reconnect without restarting the app.
- Group selection now uses the same Android-owned FFI session; desktop and Android share the existing input validation and response handling in FfiCore. Multi-member selector behavior remains to be tested with a suitable config.
- Final library tests: 123 passed, 0 failed, 1 ignored (android-bridge-check). Ordinary all-target Clippy with -D warnings passed. Gradle Kotlin unit tests and APK packaging passed.
- Desktop android-bridge-check Clippy with -D warnings fails on pre-existing platform-only dead-code warnings; this feature is covered by compilation/tests, not claimed as a clean lint run. The ordinary CI lint configuration passed.
- Stopping/restarting the lab Xray process for an unavailable-server negative case was rejected by automatic approval review with a generic policy block; the server was left untouched. This case is not claimed as passed.
- Android release debug key differs between builds; upgrading the earlier published APK cannot preserve data until a persistent protected signing key is configured. Do not put a private key in repository files.

## Android 16: проверка 2026-10-10

На отдельном эмуляторе API36 проверены установка ARM64 APK, импорт и обновление HTTPS-подписки, Keystore, VLESS/REALITY через независимый локальный Xray, передача 256 КиБ с совпавшим SHA-256, отключение TUN и повторное подключение. Исправлены неверный статус после отключения, опрос счётчиков и выбор группы через Android-сессию. 123 теста прошли (1 пропущен), обычный Clippy прошёл. Это проверка TCP в лаборатории с ARM-трансляцией, не физического телефона или реального зарубежного IP. Постоянная подпись APK, удалённый провайдер, UDP/DNS, несколько узлов группы и физические телефоны остаются отдельными задачами. Подробности: [отчёт](ANDROID16_RUNTIME_REPORT.en.md).

## Additional API36 checks before PR merge

- Full Windows-host Rust test command (including binary/doc test targets): 123 passed, 0 failed, 1 ignored; binary/doc targets contain no tests. Log: android16-full-tests.log.
- CI-equivalent android-bridge-check Clippy with `-D warnings -A dead_code` passed. Dependency smoltcp still emits dependency warnings; no client lint errors. Log: android16-ci-clippy.log. The earlier strict dead_code run remains recorded separately.
- Final local APK disconnect shows Not connected; tun0 no longer exists and the restricted marker request fails. Screenshots/log: android16-final-disconnected.png, android16-final-disconnect-baseline.log.
- A separate manual VLESS profile to the closed lab port 51986 was imported beside the subscription. Its native VPN service starts, but the marker request fails as expected; a running VPN service is not proof of endpoint reachability. Log: android16-unavailable-probe.log. No lab server process was stopped.
- The stored HTTPS subscription survived same-key APK update and cold launch. Selecting it exposes the group; refresh succeeded with one server, zero rejected/duplicates, while the separate manual profile remained. Screenshot: android16-mixed-refresh.png.
- Wrong credentials, UDP/DNS, multiple selector members, real phones, landscape/font/cutout matrix and remote provider exit are still not covered by these additional checks.
