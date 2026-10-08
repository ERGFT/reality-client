//! Bounded subscription retrieval and parsing. Never include URLs or response
//! contents in errors: both can contain account credentials.

use std::{io::Read, time::Duration};

use base64::{Engine, engine::general_purpose};
use reality_core::vless::uri::{Security, VlessConfig};
use zeroize::Zeroizing;

pub(crate) const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const MAX_SERVERS: usize = 1000;
const MAX_ENTRIES: usize = 2000;
const MAX_LINK_BYTES: usize = 16 * 1024;
const MAX_URL_BYTES: usize = 16 * 1024;

pub(crate) struct ImportedServer {
    pub name: String,
    pub link: Zeroizing<String>,
}

pub(crate) struct RejectedEntry {
    pub line: usize,
    pub reason: &'static str,
}

pub(crate) struct ImportResult {
    pub servers: Vec<ImportedServer>,
    pub rejected: Vec<RejectedEntry>,
    pub duplicates: usize,
}

impl ImportResult {
    pub fn summary(&self) -> String {
        let mut result = format!(
            "Серверов: {}. Отклонено: {}. Дублей: {}.",
            self.servers.len(),
            self.rejected.len(),
            self.duplicates
        );
        // Keep the UI report bounded even when most of the input is invalid.
        for rejected in self.rejected.iter().take(8) {
            result.push_str(&format!("\nСтрока {}: {}", rejected.line, rejected.reason));
        }
        if self.rejected.len() > 8 {
            result.push_str("\nПоказаны первые 8 ошибок.");
        }
        result
    }
}

pub(crate) fn validate_url(value: &str) -> Result<url::Url, String> {
    if value.is_empty()
        || value.len() > MAX_URL_BYTES
        || value.chars().any(char::is_whitespace)
        || value.chars().any(char::is_control)
    {
        return Err("Введите HTTPS-ссылку подписки длиной не более 16 КиБ.".into());
    }
    let uri =
        url::Url::parse(value).map_err(|_| "Не удалось разобрать ссылку подписки.".to_owned())?;
    if uri.scheme() != "https" || uri.host_str().is_none() {
        return Err("Для подписки требуется HTTPS-ссылка.".into());
    }
    if !uri.username().is_empty() || uri.password().is_some() || uri.fragment().is_some() {
        return Err("Ссылка подписки не должна содержать userinfo или фрагмент #.".into());
    }
    Ok(uri)
}

pub(crate) fn download(value: &str) -> Result<ImportResult, String> {
    let agent = build_agent(Duration::from_secs(30), None, false);
    download_with_agent(value, &agent)
}

fn build_agent(timeout: Duration, tls: Option<ureq::tls::TlsConfig>, direct: bool) -> ureq::Agent {
    let mut config = ureq::Agent::config_builder()
        .https_only(true)
        .timeout_global(Some(timeout))
        .max_redirects(3);
    if let Some(tls) = tls {
        config = config.tls_config(tls);
    }
    if direct {
        config = config.proxy(None);
    }
    config.build().into()
}

fn download_with_agent(value: &str, agent: &ureq::Agent) -> Result<ImportResult, String> {
    let uri = validate_url(value)?;
    let mut response = agent
        .get(uri.as_str())
        .header("User-Agent", "RealityClient/0.1")
        .header(
            "Accept",
            "text/plain, application/octet-stream;q=0.9, */*;q=0.1",
        )
        .call()
        .map_err(network_error)?;
    if response.status() != 200 {
        return Err(format!(
            "Сервис подписки вернул HTTP {}.",
            response.status().as_u16()
        ));
    }
    let body = read_bounded(response.body_mut().as_reader())?;
    parse(&body)
}

fn network_error(error: ureq::Error) -> String {
    match error {
        ureq::Error::StatusCode(status) => format!("Сервис подписки вернул HTTP {status}."),
        ureq::Error::Timeout(_) => "Загрузка подписки превысила 30 секунд.".into(),
        ureq::Error::TooManyRedirects => "Сервис подписки перенаправил запрос более 3 раз.".into(),
        ureq::Error::HostNotFound => "Не удалось определить адрес сервиса подписки.".into(),
        _ => "Не удалось загрузить подписку. Проверьте сеть, HTTPS-ссылку и сертификат сервиса."
            .into(),
    }
}

fn read_bounded(reader: impl Read) -> Result<Zeroizing<Vec<u8>>, String> {
    let mut body = Zeroizing::new(Vec::new());
    reader
        .take(MAX_BODY_BYTES as u64 + 1)
        .read_to_end(&mut body)
        .map_err(|_| "Не удалось прочитать ответ сервиса подписки.".to_owned())?;
    if body.len() > MAX_BODY_BYTES {
        return Err("Ответ подписки превышает 2 МиБ.".into());
    }
    Ok(body)
}

pub(crate) fn parse(body: &[u8]) -> Result<ImportResult, String> {
    if body.len() > MAX_BODY_BYTES {
        return Err("Ответ подписки превышает 2 МиБ.".into());
    }
    let text = std::str::from_utf8(body)
        .map_err(|_| "Подписка должна содержать текст UTF-8 или Base64.".to_owned())?
        .trim_start_matches('\u{feff}')
        .trim();
    if text.is_empty() {
        return Err("Сервис вернул пустую подписку; предыдущие серверы сохранены.".into());
    }
    let decoded;
    let text = if text.contains("://") {
        text
    } else {
        let compact = Zeroizing::new(
            text.chars()
                .filter(|c| !c.is_ascii_whitespace())
                .collect::<String>(),
        );
        decoded = [&general_purpose::STANDARD, &general_purpose::STANDARD_NO_PAD,
            &general_purpose::URL_SAFE, &general_purpose::URL_SAFE_NO_PAD]
            .iter().find_map(|engine| engine.decode(compact.as_bytes()).ok())
            .map(Zeroizing::new)
            .ok_or_else(|| "Неизвестный формат подписки. Поддерживаются списки ссылок и Base64; JSON/YAML/HTML не импортируются.".to_owned())?;
        std::str::from_utf8(&decoded)
            .map_err(|_| "Base64-подписка не содержит текст UTF-8.".to_owned())?
            .trim_start_matches('\u{feff}')
            .trim()
    };
    if text.is_empty() {
        return Err("Сервис вернул пустую подписку; предыдущие серверы сохранены.".into());
    }
    if text.starts_with(['<', '{', '[']) || text.starts_with("proxies:") {
        return Err(
            "Сервис вернул HTML, JSON или YAML. Выберите формат подписки со списком ссылок.".into(),
        );
    }
    let mut result = ImportResult {
        servers: Vec::new(),
        rejected: Vec::new(),
        duplicates: 0,
    };
    let mut identities: Vec<Zeroizing<String>> = Vec::new();
    let mut entries = 0;
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        entries += 1;
        if entries > MAX_ENTRIES {
            return Err("Подписка содержит более 2000 записей.".into());
        }
        let reason = if line.len() > MAX_LINK_BYTES {
            Some("Ссылка превышает 16 КиБ.")
        } else if !line.starts_with("vless://") {
            Some("Протокол не поддерживается импортом клиента; требуется vless://.")
        } else if line.chars().any(char::is_control) || line.chars().any(char::is_whitespace) {
            Some("Ссылка содержит пробелы или управляющие символы.")
        } else {
            None
        };
        if let Some(reason) = reason {
            result.rejected.push(RejectedEntry {
                line: index + 1,
                reason,
            });
            continue;
        }
        let parsed = VlessConfig::parse(line).and_then(|config| {
            config.validate()?;
            if config.security == Security::Reality {
                config.reality_params()?;
            }
            Ok(config)
        });
        let Ok(config) = parsed else {
            result.rejected.push(RejectedEntry {
                line: index + 1,
                reason: "Некорректные или неподдерживаемые параметры VLESS/REALITY/транспорта.",
            });
            continue;
        };
        if config.port == 0 {
            result.rejected.push(RejectedEntry {
                line: index + 1,
                reason: "Порт сервера должен быть в диапазоне 1–65535.",
            });
            continue;
        }
        // This key excludes the display name and normalizes query ordering.
        // It contains credentials, so clear it when parsing finishes.
        let identity = Zeroizing::new(config.pool_key());
        if identities.iter().any(|existing| **existing == *identity) {
            result.duplicates += 1;
            continue;
        }
        if result.servers.len() >= MAX_SERVERS {
            return Err("Подписка содержит более 1000 серверов.".into());
        }
        identities.push(identity);
        let name = config
            .remark
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(&config.host)
            .chars()
            .filter(|c| !c.is_control())
            .take(100)
            .collect::<String>();
        result.servers.push(ImportedServer {
            name,
            link: Zeroizing::new(line.to_owned()),
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscription_urls_cannot_be_exposed_by_dependency_trace_logging() {
        // ureq's DebugUri only redacts path/query when TRACE is disabled.
        let maximum = std::hint::black_box(log::STATIC_MAX_LEVEL);
        assert!(maximum <= log::LevelFilter::Debug);
    }
    const LINK: &str = "vless://00000000-0000-4000-8000-000000000000@edge.example.org:443?encryption=none#%D0%A1%D0%B5%D1%80%D0%B2%D0%B5%D1%80";

    #[test]
    fn imports_utf8_names_and_crlf_comments() {
        let body = format!("\u{feff}# comment\r\n{LINK}\r\n\r\n");
        let result = parse(body.as_bytes()).unwrap();
        assert_eq!(result.servers.len(), 1);
        assert_eq!(result.servers[0].name, "Сервер");
        assert_eq!(result.servers[0].link.as_str(), LINK);
    }

    #[test]
    fn imports_all_base64_variants_and_wrapped_input() {
        for engine in [
            &general_purpose::STANDARD,
            &general_purpose::STANDARD_NO_PAD,
            &general_purpose::URL_SAFE,
            &general_purpose::URL_SAFE_NO_PAD,
        ] {
            let encoded = engine.encode(format!("{LINK}\n"));
            let wrapped = format!("{}\r\n{}", &encoded[..20], &encoded[20..]);
            assert_eq!(parse(wrapped.as_bytes()).unwrap().servers[0].name, "Сервер");
        }
    }

    #[test]
    fn deduplicates_servers_even_when_names_differ() {
        let other = LINK.replace("#%D0%A1%D0%B5%D1%80%D0%B2%D0%B5%D1%80", "#Other");
        let result = parse(format!("{LINK}\n{other}").as_bytes()).unwrap();
        assert_eq!(result.servers.len(), 1);
        assert_eq!(result.duplicates, 1);
    }

    #[test]
    fn reports_bad_entries_without_echoing_secrets() {
        let input =
            format!("trojan://secret@example.org:443\nvless://secret@example.org:443\n{LINK}");
        let result = parse(input.as_bytes()).unwrap();
        assert_eq!(result.servers.len(), 1);
        assert_eq!(result.rejected.len(), 2);
        assert!(!result.summary().contains("secret"));
        assert_eq!(result.rejected[0].line, 1);
    }

    #[test]
    fn validates_reality_and_transport_with_core_parser() {
        for query in [
            "security=reality",
            "type=kcp",
            "flow=unknown",
            "encryption=unknown",
        ] {
            let input = format!(
                "vless://00000000-0000-4000-8000-000000000000@edge.example.org:443?{query}"
            );
            assert_eq!(parse(input.as_bytes()).unwrap().rejected.len(), 1);
        }
    }

    #[test]
    fn rejects_empty_unknown_and_oversized_responses() {
        for body in [
            b"".as_slice(),
            b"   ",
            b"{}",
            b"<html>error</html>",
            b"////",
        ] {
            assert!(parse(body).is_err());
        }
        assert!(parse(&vec![b'x'; MAX_BODY_BYTES + 1]).is_err());
        assert!(read_bounded(&vec![b'x'; MAX_BODY_BYTES + 1][..]).is_err());
        assert_eq!(read_bounded(&b"abc"[..]).unwrap().as_slice(), b"abc");
    }

    #[test]
    fn rejects_excessive_entries() {
        assert!(parse("invalid\n".repeat(MAX_ENTRIES + 1).as_bytes()).is_err());
    }

    #[test]
    fn only_accepts_https_subscription_urls_without_userinfo_or_fragments() {
        assert!(validate_url("https://service.example.org/sublink/fixture?token=fixture").is_ok());
        for input in [
            "http://example.org/sub",
            "file:///secret",
            "https://user:secret@example.org/sub",
            "https://example.org/sub#token",
            "https://example.org/sub\n",
        ] {
            let error = validate_url(input).unwrap_err();
            assert!(!error.contains("secret"));
        }
    }
}

#[cfg(test)]
mod https_tests {
    use super::*;
    use rustls::{
        ServerConfig, ServerConnection, StreamOwned,
        pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
    };
    use std::{io::Write, net::TcpListener, sync::Arc, thread, time::Instant};

    const CA: &[u8] = include_bytes!("../tests/fixtures/subscription-ca.der");
    const CERT: &[u8] = include_bytes!("../tests/fixtures/subscription-server.der");
    const KEY: &[u8] = include_bytes!("../tests/fixtures/subscription-test-key.der");
    const LINK: &str =
        "vless://00000000-0000-4000-8000-000000000000@fixture.example.org:443#Fixture";

    fn trusted_agent(timeout: Duration) -> ureq::Agent {
        let tls = ureq::tls::TlsConfig::builder()
            .root_certs(ureq::tls::RootCerts::new_with_certs(&[
                ureq::tls::Certificate::from_der(CA),
            ]))
            .build();
        build_agent(timeout, Some(tls), true)
    }

    fn fixture(
        reply: impl Fn(&str) -> Vec<u8>,
        count: usize,
        delay: Duration,
    ) -> (String, thread::JoinHandle<usize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!(
            "https://127.0.0.1:{}/sublink/fixture",
            listener.local_addr().unwrap().port()
        );
        let response = reply(&url);
        let config = Arc::new(
            ServerConfig::builder_with_provider(Arc::new(
                rustls::crypto::aws_lc_rs::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(CERT.to_vec())],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(KEY.to_vec())),
            )
            .unwrap(),
        );
        let worker = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(4);
            let mut served = 0;
            while served < count && Instant::now() < deadline {
                let socket = match listener.accept() {
                    Ok((socket, _)) => socket,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(error) => panic!("fixture accept: {error}"),
                };
                served += 1;
                // Winsock accepts inherit listener nonblocking mode.
                socket.set_nonblocking(false).unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                socket
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut stream =
                    StreamOwned::new(ServerConnection::new(config.clone()).unwrap(), socket);
                let mut request = Vec::new();
                let mut byte = [0];
                while request.len() < 8192 && !request.ends_with(b"\r\n\r\n") {
                    if stream.read_exact(&mut byte).is_err() {
                        break;
                    }
                    request.push(byte[0]);
                }
                if !request.ends_with(b"\r\n\r\n") {
                    continue;
                }
                assert!(request.starts_with(b"GET /sublink/fixture"));
                let split = if delay.is_zero() {
                    0
                } else {
                    response
                        .windows(4)
                        .position(|part| part == b"\r\n\r\n")
                        .map_or(0, |offset| offset + 4)
                };
                if split > 0 {
                    let _ = stream.write_all(&response[..split]);
                    let _ = stream.flush();
                }
                thread::sleep(delay);
                let _ = stream.write_all(&response[split..]);
                stream.conn.send_close_notify();
                let _ = stream.flush();
            }
            served
        });
        (url, worker)
    }

    fn response(body: &[u8]) -> Vec<u8> {
        let mut bytes = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        bytes.extend_from_slice(body);
        bytes
    }

    #[test]
    fn subscription_https_download_checks_certificates_and_parses_response() {
        let (url, worker) = fixture(|_| response(LINK.as_bytes()), 1, Duration::ZERO);
        let result = download_with_agent(&url, &trusted_agent(Duration::from_secs(2))).unwrap();
        assert_eq!(result.servers[0].name, "Fixture");
        assert_eq!(worker.join().unwrap(), 1);
        let (url, worker) = fixture(|_| response(LINK.as_bytes()), 1, Duration::ZERO);
        let agent = build_agent(Duration::from_secs(2), None, true);
        assert!(download_with_agent(&url, &agent).is_err());
        assert_eq!(worker.join().unwrap(), 1);
    }

    #[test]
    fn subscription_https_limits_redirects_and_rejects_http_downgrade() {
        let (url, worker) = fixture(
            |url| {
                format!("HTTP/1.1 302 Found\r\nLocation: {url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").into_bytes()
            },
            4,
            Duration::ZERO,
        );
        let error = download_with_agent(&url, &trusted_agent(Duration::from_secs(2)))
            .err()
            .unwrap();
        assert!(error.contains("3"));
        assert_eq!(worker.join().unwrap(), 4);
        let (url, worker) = fixture(
            |_| {
                b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/private-fixture\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
            },
            1,
            Duration::ZERO,
        );
        let error = download_with_agent(&url, &trusted_agent(Duration::from_secs(2)))
            .err()
            .unwrap();
        assert!(!error.contains("private-fixture"));
        assert_eq!(worker.join().unwrap(), 1);
    }

    #[test]
    fn subscription_https_bounds_response_and_total_time_and_reports_status() {
        let (url, worker) = fixture(
            |_| response(&vec![b'x'; MAX_BODY_BYTES + 1]),
            1,
            Duration::ZERO,
        );
        assert!(
            download_with_agent(&url, &trusted_agent(Duration::from_secs(2)))
                .err()
                .unwrap()
                .contains("2 МиБ")
        );
        assert_eq!(worker.join().unwrap(), 1);
        let (url, worker) = fixture(|_| response(LINK.as_bytes()), 1, Duration::from_millis(300));
        assert!(download_with_agent(&url, &trusted_agent(Duration::from_millis(100))).is_err());
        assert_eq!(worker.join().unwrap(), 1);
        let (url, worker) = fixture(
            |_| {
                b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
            },
            1,
            Duration::ZERO,
        );
        assert!(
            download_with_agent(&url, &trusted_agent(Duration::from_secs(2)))
                .err()
                .unwrap()
                .contains("403")
        );
        assert_eq!(worker.join().unwrap(), 1);
    }
}
