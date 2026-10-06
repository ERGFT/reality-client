//! Адрес сервера из ссылки и проверка IP-адресов (без обращения к интерфейсу).

use std::{net::ToSocketAddrs, time::Duration};

use crate::config_json::parse_jsonc_value;

pub(crate) fn server_endpoint(link: &str) -> String {
    let Ok(url) = url::Url::parse(link) else {
        return "Некорректная ссылка".to_owned();
    };
    let Some(host) = url.host_str() else {
        return "Адрес не указан".to_owned();
    };
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    }
}

pub(crate) fn server_ip_initial(link: &str) -> String {
    let Ok(url) = url::Url::parse(link) else {
        return "Определите IP узла".to_owned();
    };
    url.host_str()
        .and_then(|host| {
            host.trim_start_matches('[')
                .trim_end_matches(']')
                .parse::<std::net::IpAddr>()
                .ok()
        })
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "Определите IP узла".to_owned())
}

pub(crate) fn server_socket_target(link: &str) -> Result<(String, u16), String> {
    let url = url::Url::parse(link)
        .map_err(|_| "В ссылке VLESS не удалось прочитать адрес узла.".to_owned())?;
    let host = url
        .host_str()
        .ok_or_else(|| "В ссылке VLESS не указан адрес узла.".to_owned())?;
    let port = url
        .port()
        .ok_or_else(|| "В ссылке VLESS не указан порт узла.".to_owned())?;
    Ok((
        host.trim_start_matches('[')
            .trim_end_matches(']')
            .to_owned(),
        port,
    ))
}

pub(crate) fn resolve_server_ips(host: &str, port: u16) -> Result<String, String> {
    let addresses = (host, port)
        .to_socket_addrs()
        .map_err(|problem| format!("Системный DNS не смог разрешить адрес узла: {problem}"))?;
    let mut ips = Vec::new();
    for address in addresses {
        let ip = address.ip().to_string();
        if !ips.contains(&ip) {
            ips.push(ip);
        }
        if ips.len() == 4 {
            break;
        }
    }
    if ips.is_empty() {
        return Err("Системный DNS не вернул IP-адрес для узла.".to_owned());
    }
    Ok(ips.join(" · "))
}

pub(crate) fn local_socks_proxy_uri(text: &str) -> Result<Option<String>, String> {
    let config = parse_jsonc_value(text)?;
    let Some(inbounds) = config.get("inbounds").and_then(serde_json::Value::as_array) else {
        return Ok(None);
    };
    for inbound in inbounds {
        let inbound_type = inbound
            .get("type")
            .or_else(|| inbound.get("protocol"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if !matches!(inbound_type, "mixed" | "socks") {
            continue;
        }
        let listen = inbound
            .get("listen")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("127.0.0.1");
        let is_loopback = listen == "localhost"
            || listen
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback());
        if !is_loopback {
            continue;
        }
        let port = inbound
            .get("listen_port")
            .or_else(|| inbound.get("port"))
            .and_then(serde_json::Value::as_u64)
            .and_then(|port| u16::try_from(port).ok())
            .filter(|port| *port != 0);
        let Some(port) = port else {
            continue;
        };
        let host = if listen == "localhost" {
            "127.0.0.1"
        } else {
            listen
        };
        let host = if host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_ipv6())
        {
            format!("[{host}]")
        } else {
            host.to_owned()
        };
        return Ok(Some(format!("socks5h://{host}:{port}")));
    }
    Ok(None)
}

pub(crate) fn fetch_public_ip_via_proxy(proxy_uri: &str) -> Result<String, String> {
    let proxy = ureq::Proxy::new(proxy_uri)
        .map_err(|problem| format!("Не удалось настроить локальный SOCKS-прокси: {problem}"))?;
    let agent = ureq::Agent::config_builder()
        .https_only(true)
        .timeout_global(Some(Duration::from_secs(10)))
        .proxy(Some(proxy))
        .build()
        .new_agent();
    let response = agent
        .get("https://api.ipify.org")
        .call()
        .map_err(|problem| {
            format!("Запрос к api.ipify.org через ядро завершился ошибкой: {problem}")
        })?;
    let body = response
        .into_body()
        .with_config()
        .limit(128)
        .read_to_string()
        .map_err(|problem| format!("Не удалось прочитать ответ сервиса проверки IP: {problem}"))?;
    body.trim()
        .parse::<std::net::IpAddr>()
        .map(|address| address.to_string())
        .map_err(|_| "Сервис проверки IP вернул ответ, который не является IP-адресом.".to_owned())
}

pub(crate) fn fetch_public_ip_direct() -> Result<String, String> {
    let agent = ureq::Agent::config_builder()
        .https_only(true)
        .timeout_global(Some(Duration::from_secs(10)))
        .proxy(None)
        .build()
        .new_agent();
    let response = agent
        .get("https://api.ipify.org")
        .call()
        .map_err(|problem| {
            format!("Запрос к api.ipify.org по обычному маршруту завершился ошибкой: {problem}")
        })?;
    let body = response
        .into_body()
        .with_config()
        .limit(128)
        .read_to_string()
        .map_err(|problem| format!("Не удалось прочитать ответ сервиса проверки IP: {problem}"))?;
    body.trim()
        .parse::<std::net::IpAddr>()
        .map(|address| address.to_string())
        .map_err(|_| "Сервис проверки IP вернул ответ, который не является IP-адресом.".to_owned())
}

#[cfg(test)]
mod public_ip_proxy_tests {
    use super::local_socks_proxy_uri;

    #[test]
    fn resolves_sing_box_mixed_inbound_to_loopback_socks_uri() {
        let config = r#"{"inbounds":[{"type":"mixed","listen_port":1080}]}"#;
        assert_eq!(
            local_socks_proxy_uri(config).unwrap().as_deref(),
            Some("socks5h://127.0.0.1:1080")
        );
    }

    #[test]
    fn resolves_xray_socks_inbound_and_ipv6_loopback() {
        let config = r#"{"inbounds":[{"protocol":"socks","listen":"::1","port":2080}]}"#;
        assert_eq!(
            local_socks_proxy_uri(config).unwrap().as_deref(),
            Some("socks5h://[::1]:2080")
        );
    }

    #[test]
    fn refuses_remote_and_non_socks_inbounds() {
        let remote = r#"{"inbounds":[{"type":"mixed","listen":"0.0.0.0","listen_port":1080}]}"#;
        let http = r#"{"inbounds":[{"protocol":"http","listen":"127.0.0.1","port":8080}]}"#;
        assert_eq!(local_socks_proxy_uri(remote).unwrap(), None);
        assert_eq!(local_socks_proxy_uri(http).unwrap(), None);
    }

    #[test]
    fn reports_invalid_full_config() {
        assert!(local_socks_proxy_uri("not JSON").is_err());
    }
}

#[cfg(test)]
mod server_address_tests {
    use super::{server_ip_initial, server_socket_target};

    #[test]
    fn extracts_domain_and_port_without_retaining_link_credentials() {
        let link = "vless://user-secret@example.com:443?security=reality";
        assert_eq!(
            server_socket_target(link).unwrap(),
            ("example.com".to_owned(), 443)
        );
        assert_eq!(server_ip_initial(link), "Определите IP узла");
    }

    #[test]
    fn uses_literal_ipv6_address_without_dns_lookup() {
        let link = "vless://user-secret@[2001:db8::1]:8443?security=reality";
        assert_eq!(
            server_socket_target(link).unwrap(),
            ("2001:db8::1".to_owned(), 8443)
        );
        assert_eq!(server_ip_initial(link), "2001:db8::1");
    }
}
