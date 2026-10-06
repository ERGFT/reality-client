# Security / Безопасность

**Русский** | [English](#english)

## Сообщить об уязвимости

Не создавайте публичный issue. Напишите владельцу репозитория
([ERGFT](https://github.com/ERGFT)) через
[приватное уведомление об уязвимости GitHub](https://github.com/ERGFT/reality-client/security/advisories/new)
или личным сообщением. Укажите версию, платформу и шаги воспроизведения.

## Что считается уязвимостью

- утечка VLESS-ссылки, UUID, токена или пароля в журнал, аргументы процесса,
  сообщения об ошибках, временные файлы с широкими правами;
- сетевой трафик мимо туннеля при включённом VPN (утечка DNS, IPv6, сокетов ядра);
- остаточные изменения системного прокси или маршрутов после остановки или сбоя;
- обход проверок конфигурации (Android: `VpnService`, фильтр приложений).

## Что важно знать

- Проект на стадии предварительных сборок; стороннего аудита нет.
- Полный JSON-конфиг может содержать секреты открытым текстом — ограничьте
  доступ к файлу и используйте `link_file`.
- Модель безопасности самого ядра — в
  [vpn-core/docs/SECURITY-MODEL.md](https://github.com/ERGFT/vpn-core/blob/main/docs/SECURITY-MODEL.md).

---

## English

### Reporting a vulnerability

Do not open a public issue. Contact the repository owner
([ERGFT](https://github.com/ERGFT)) through GitHub's
[private vulnerability reporting](https://github.com/ERGFT/reality-client/security/advisories/new)
or a direct message. Include the version, platform and reproduction steps.

### What counts as a vulnerability

- a VLESS link, UUID, token or password leaking into logs, process arguments, error
  messages or temporary files with broad permissions;
- traffic bypassing the tunnel while the VPN is on (DNS, IPv6, core-socket leaks);
- leftover system-proxy or route changes after a stop or a crash;
- bypassing config validation (Android: `VpnService`, the app filter).

### Good to know

- The project is at the pre-release stage; there has been no third-party audit.
- A full JSON config may hold secrets in plain text: restrict access to the file and
  use `link_file`.
- The core's own security model is in
  [vpn-core/docs/SECURITY-MODEL.en.md](https://github.com/ERGFT/vpn-core/blob/main/docs/SECURITY-MODEL.en.md).
