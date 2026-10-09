//! Правка JSON-конфигурации ядра: параметры сети, fake-IP, правила маршрутизации.

pub(crate) const MANAGED_ROUTE_MARKER: &str = "//reality-client-ui";

/// Bind the selected server to the client's proxy outlet, without changing the
/// user's saved JSON or embedding a share link in the editor/configuration.
pub(crate) fn bind_selected_profile(
    text: &str,
    secret_path: &std::path::Path,
) -> Result<String, String> {
    let mut value = parse_jsonc_value(text)?;
    if is_xray_config(&value) {
        return Err("Для выбранного сервера используйте конфиг sing-box с VLESS-выходом proxy. Для самостоятельного Xray JSON отключите привязку выбранного сервера.".into());
    }
    let outlets = value
        .get_mut("outbounds")
        .and_then(serde_json::Value::as_array_mut)
        .ok_or("В конфиге нет массива outbounds.")?;
    let matches = outlets
        .iter()
        .filter(|o| o.get("tag").and_then(serde_json::Value::as_str) == Some("proxy"))
        .count();
    if matches != 1 {
        return Err("Для выбранного сервера нужен один VLESS-выход с тегом proxy. Исправьте JSON или отключите привязку выбранного сервера.".into());
    }
    let outlet = outlets
        .iter_mut()
        .find(|o| o.get("tag").and_then(serde_json::Value::as_str) == Some("proxy"))
        .unwrap();
    if outlet.get("type").and_then(serde_json::Value::as_str) != Some("vless") {
        return Err(
            "Выход proxy должен иметь тип vless для подключения выбранного сервера.".into(),
        );
    }
    let outlet = outlet
        .as_object_mut()
        .ok_or("Выход proxy должен быть объектом.")?;
    outlet.remove("link");
    outlet.insert("link_file".into(), serde_json::json!(secret_path));
    serde_json::to_string_pretty(&value)
        .map_err(|_| "Не удалось подготовить конфигурацию выбранного сервера.".into())
}

#[cfg(test)]
mod selected_profile_tests {
    use super::*;
    #[test]
    fn selected_profile_binding_keeps_tun_dns_routes_and_original_editor() {
        let text = r#"{"inbounds":[{"type":"tun","address":["172.19.0.1/30"]}],"outbounds":[{"type":"vless","tag":"proxy","link":"vless://old-secret"},{"type":"direct","tag":"direct"}],"route":{"final":"proxy","rules":[{"domain_suffix":["vk.com"],"outbound":"direct"}]},"dns":{"servers":[{"type":"https","server":"1.1.1.1","detour":"proxy"}]}}"#;
        let bound =
            bind_selected_profile(text, std::path::Path::new("selected-server.txt")).unwrap();
        let a = parse_jsonc_value(text).unwrap();
        let b = parse_jsonc_value(&bound).unwrap();
        for field in ["inbounds", "route", "dns"] {
            assert_eq!(a[field], b[field]);
        }
        assert!(b["outbounds"][0].get("link").is_none());
        assert_eq!(b["outbounds"][0]["link_file"], "selected-server.txt");
        assert!(text.contains("old-secret"));
        assert!(!bound.contains("old-secret"));
    }
    #[test]
    fn selected_profile_binding_refuses_ambiguous_or_wrong_outlet() {
        for text in [
            r#"{"outbounds":[{"type":"direct","tag":"proxy"}]}"#,
            r#"{"outbounds":[{"type":"vless","tag":"proxy"},{"type":"vless","tag":"proxy"}]}"#,
            r#"{"outbounds":[{"type":"vless","tag":"other"}]}"#,
        ] {
            assert!(bind_selected_profile(text, std::path::Path::new("fixture.txt")).is_err());
        }
    }
}

pub(crate) fn parse_jsonc_value(text: &str) -> Result<serde_json::Value, String> {
    let bytes = text.as_bytes();
    let mut cleaned = Vec::with_capacity(bytes.len());
    let mut index = 0;
    let mut in_string = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            cleaned.push(byte);
            if byte == b'\\' && index + 1 < bytes.len() {
                cleaned.push(bytes[index + 1]);
                index += 2;
                continue;
            }
            if byte == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        match byte {
            b'"' => {
                in_string = true;
                cleaned.push(byte);
                index += 1;
            }
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index += 2;
                while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/')
                {
                    if bytes[index] == b'\n' {
                        cleaned.push(b'\n');
                    }
                    index += 1;
                }
                index = (index + 2).min(bytes.len());
            }
            b',' => {
                let mut lookahead = index + 1;
                while lookahead < bytes.len() && bytes[lookahead].is_ascii_whitespace() {
                    lookahead += 1;
                }
                if !matches!(bytes.get(lookahead), Some(b']') | Some(b'}')) {
                    cleaned.push(byte);
                }
                index += 1;
            }
            _ => {
                cleaned.push(byte);
                index += 1;
            }
        }
    }
    let cleaned = String::from_utf8(cleaned)
        .map_err(|_| "Конфигурация содержит некорректный UTF-8.".to_owned())?;
    serde_json::from_str(&cleaned).map_err(|problem| format!("Некорректный JSON: {problem}"))
}

pub(crate) fn is_xray_config(value: &serde_json::Value) -> bool {
    value
        .get("outbounds")
        .and_then(serde_json::Value::as_array)
        .and_then(|items| items.first())
        .is_some_and(|item| item.get("protocol").is_some() && item.get("type").is_none())
}

pub(crate) fn value_has_tun(value: &serde_json::Value) -> bool {
    value
        .get("inbounds")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|inbound| {
                inbound.get("type").and_then(serde_json::Value::as_str) == Some("tun")
                    || inbound.get("protocol").and_then(serde_json::Value::as_str) == Some("tun")
            })
        })
}

pub(crate) fn value_has_fakeip(value: &serde_json::Value) -> bool {
    let dns = value.get("dns");
    if dns
        .and_then(|dns| dns.get("fakeip"))
        .and_then(|fakeip| fakeip.get("enabled"))
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        return true;
    }
    let has_sing_box_fakeip_server = dns
        .and_then(|dns| dns.get("servers"))
        .and_then(serde_json::Value::as_array)
        .is_some_and(|servers| servers.iter().any(is_sing_box_fakeip_server));
    if has_sing_box_fakeip_server {
        return true;
    }
    value
        .get("fakedns")
        .is_some_and(|pools| pools.as_array().is_none_or(|items| !items.is_empty()))
        && dns
            .and_then(|dns| dns.get("servers"))
            .and_then(serde_json::Value::as_array)
            .is_some_and(|servers| {
                servers.iter().any(|server| {
                    server.as_str() == Some("fakedns")
                        || server.get("address").and_then(serde_json::Value::as_str)
                            == Some("fakedns")
                })
            })
}

pub(crate) fn is_sing_box_fakeip_server(server: &serde_json::Value) -> bool {
    server.get("type").and_then(serde_json::Value::as_str) == Some("fakeip")
        || server.get("address").and_then(serde_json::Value::as_str) == Some("fakeip")
}

pub(crate) fn route_rules_mut(
    value: &mut serde_json::Value,
    xray: bool,
) -> Result<&mut Vec<serde_json::Value>, String> {
    let section = if xray { "routing" } else { "route" };
    let object = value
        .get_mut(section)
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            format!("В конфигурации нет объекта {section}; сначала добавьте его в JSON.")
        })?;
    let rules = object
        .entry("rules")
        .or_insert_with(|| serde_json::json!([]));
    rules
        .as_array_mut()
        .ok_or_else(|| format!("{section}.rules должен быть массивом."))
}

pub(crate) fn android_app_filter_values(value: &serde_json::Value) -> Vec<String> {
    value
        .get("inbounds")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .find(|inbound| {
            inbound.get("type").and_then(serde_json::Value::as_str) == Some("tun")
                || inbound.get("protocol").and_then(serde_json::Value::as_str) == Some("tun")
        })
        .and_then(|inbound| inbound.get("include_package"))
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::to_owned)
        .collect()
}

pub(crate) fn set_android_app_filter(text: &str, packages_text: &str) -> Result<String, String> {
    if !cfg!(target_os = "android") {
        return Err("Фильтр приложений доступен только в Android-сборке.".into());
    }
    let packages = split_rule_values(packages_text);
    for package in &packages {
        let valid = package.split('.').count() >= 2
            && package.split('.').all(|part| {
                !part.is_empty()
                    && part.as_bytes()[0].is_ascii_alphabetic()
                    && part
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            });
        if !valid {
            return Err(format!("Некорректный Android package ID «{package}»."));
        }
    }
    let mut value = parse_jsonc_value(text)?;
    let tun = value
        .get_mut("inbounds")
        .and_then(serde_json::Value::as_array_mut)
        .and_then(|inbounds| {
            inbounds.iter_mut().find(|inbound| {
                inbound.get("type").and_then(serde_json::Value::as_str) == Some("tun")
                    || inbound.get("protocol").and_then(serde_json::Value::as_str) == Some("tun")
            })
        })
        .ok_or_else(|| "Сначала включите TUN и примените настройки.".to_owned())?;
    let tun = tun
        .as_object_mut()
        .ok_or("Объект TUN должен быть JSON-объектом.")?;
    if packages.is_empty() {
        tun.remove("include_package");
    } else {
        tun.insert("include_package".to_owned(), serde_json::json!(packages));
    }
    serde_json::to_string_pretty(&value)
        .map_err(|problem| format!("Не удалось записать JSON: {problem}"))
}

pub(crate) fn set_network_options(
    text: &str,
    tun_enabled: bool,
    fakeip_enabled: bool,
) -> Result<String, String> {
    if tun_enabled && !cfg!(any(windows, target_os = "linux", target_os = "android")) {
        return Err("TUN в этой сборке недоступен на этой платформе.".into());
    }
    let mut value = parse_jsonc_value(text)?;
    let xray = is_xray_config(&value);
    let inbounds = value
        .as_object_mut()
        .ok_or_else(|| "Корень JSON-конфигурации должен быть объектом.".to_owned())?
        .entry("inbounds")
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .ok_or_else(|| "inbounds должен быть массивом.".to_owned())?;
    let key = if xray { "protocol" } else { "type" };
    if tun_enabled {
        if !inbounds
            .iter()
            .any(|inbound| inbound.get(key).and_then(serde_json::Value::as_str) == Some("tun"))
        {
            let tun = if xray {
                serde_json::json!({
                    "protocol": "tun", "tag": "tun",
                    "address": ["172.19.0.1/30", "fdfe:dcba:9876::1/126"], "mtu": 1500
                })
            } else {
                serde_json::json!({
                    "type": "tun", "tag": "tun",
                    "address": ["172.19.0.1/30", "fdfe:dcba:9876::1/126"], "mtu": 1500
                })
            };
            inbounds.push(tun);
        }
        ensure_dns_for_tun(&mut value, xray)?;
    } else {
        inbounds
            .retain(|inbound| inbound.get(key).and_then(serde_json::Value::as_str) != Some("tun"));
        if inbounds.is_empty() {
            inbounds.push(if xray {
                serde_json::json!({ "protocol": "mixed", "tag": "local", "listen": "127.0.0.1", "port": 1080 })
            } else {
                serde_json::json!({ "type": "mixed", "tag": "local", "listen": "127.0.0.1", "listen_port": 1080 })
            });
        }
    }
    set_fakeip(&mut value, xray, fakeip_enabled)?;
    serde_json::to_string_pretty(&value)
        .map_err(|problem| format!("Не удалось записать JSON: {problem}"))
}

pub(crate) fn default_dns_detour_tag(value: &serde_json::Value) -> Option<String> {
    let outbounds = value.get("outbounds")?.as_array()?;
    let has_tag = |tag: &str| {
        outbounds
            .iter()
            .any(|item| item.get("tag").and_then(serde_json::Value::as_str) == Some(tag))
    };
    value
        .get("route")
        .and_then(|route| route.get("final"))
        .and_then(serde_json::Value::as_str)
        .filter(|tag| has_tag(tag))
        .or_else(|| {
            outbounds
                .iter()
                .find(|item| {
                    matches!(
                        item.get("type").and_then(serde_json::Value::as_str),
                        Some("vless" | "trojan" | "selector" | "urltest")
                    )
                })
                .and_then(|item| item.get("tag"))
                .and_then(serde_json::Value::as_str)
        })
        .or_else(|| {
            outbounds
                .iter()
                .find_map(|item| item.get("tag").and_then(serde_json::Value::as_str))
        })
        .map(str::to_owned)
}

pub(crate) fn ensure_dns_for_tun(value: &mut serde_json::Value, xray: bool) -> Result<(), String> {
    let detour_tag = (!xray).then(|| default_dns_detour_tag(value)).flatten();
    let Some(root) = value.as_object_mut() else {
        return Err("Корень JSON-конфигурации должен быть объектом.".into());
    };
    let dns = root.entry("dns").or_insert_with(|| serde_json::json!({}));
    let dns = dns.as_object_mut().ok_or("dns должен быть объектом.")?;
    if !dns.contains_key("servers")
        || dns
            .get("servers")
            .is_some_and(|servers| servers.as_array().is_some_and(Vec::is_empty))
    {
        if xray {
            dns.insert(
                "servers".to_owned(),
                serde_json::json!(["https://1.1.1.1/dns-query"]),
            );
        } else {
            let detour_tag = detour_tag
                .as_deref()
                .ok_or("Для DNS в TUN нужен выход с непустым тегом в outbounds.")?;
            dns.insert("servers".to_owned(), serde_json::json!([{"type":"https","tag":"remote","server":"1.1.1.1","detour":detour_tag}]));
            dns.entry("final")
                .or_insert_with(|| serde_json::json!("remote"));
        }
    }
    Ok(())
}

pub(crate) fn set_fakeip(
    value: &mut serde_json::Value,
    xray: bool,
    enabled: bool,
) -> Result<(), String> {
    let detour_tag = default_dns_detour_tag(value);
    let root = value
        .as_object_mut()
        .ok_or_else(|| "Корень JSON-конфигурации должен быть объектом.".to_owned())?;
    if xray {
        if enabled {
            let pools = root.entry("fakedns").or_insert_with(|| {
                serde_json::json!([
                    {"ipPool":"198.18.0.0/15"}, {"ipPool":"fc00::/18"}
                ])
            });
            if pools.is_null() {
                *pools = serde_json::json!([{"ipPool":"198.18.0.0/15"}, {"ipPool":"fc00::/18"}]);
            }
            let dns = root
                .entry("dns")
                .or_insert_with(|| serde_json::json!({"servers":[]}));
            let dns = dns.as_object_mut().ok_or("dns должен быть объектом.")?;
            let servers = dns
                .entry("servers")
                .or_insert_with(|| serde_json::json!([]));
            let servers = servers
                .as_array_mut()
                .ok_or("dns.servers должен быть массивом.")?;
            prioritize_xray_fake_dns(servers);
        } else {
            root.remove("fakedns");
            if let Some(servers) = root
                .get_mut("dns")
                .and_then(|dns| dns.get_mut("servers"))
                .and_then(serde_json::Value::as_array_mut)
            {
                servers.retain(|server| {
                    server.as_str() != Some("fakedns")
                        && server.get("address").and_then(serde_json::Value::as_str)
                            != Some("fakedns")
                });
            }
        }
        return Ok(());
    }
    if enabled {
        let dns = root.entry("dns").or_insert_with(|| serde_json::json!({}));
        let dns = dns.as_object_mut().ok_or("dns должен быть объектом.")?;
        if !dns.contains_key("servers")
            || dns
                .get("servers")
                .is_some_and(|servers| servers.as_array().is_some_and(Vec::is_empty))
        {
            let detour_tag = detour_tag
                .as_deref()
                .ok_or("Для Fake-IP DNS нужен выход с непустым тегом в outbounds.")?;
            dns.insert("servers".to_owned(), serde_json::json!([{"type":"https","tag":"remote","server":"1.1.1.1","detour":detour_tag}]));
            dns.entry("final")
                .or_insert_with(|| serde_json::json!("remote"));
        }
        let fakeip_tag = {
            let servers = dns
                .get_mut("servers")
                .and_then(serde_json::Value::as_array_mut)
                .ok_or("dns.servers должен быть массивом.")?;
            if !servers.iter().any(is_sing_box_fakeip_server) {
                servers.push(serde_json::json!({"type":"fakeip","tag":"reality-client-fakeip"}));
            }
            servers
                .iter()
                .find(|server| is_sing_box_fakeip_server(server))
                .and_then(|server| server.get("tag"))
                .and_then(serde_json::Value::as_str)
                .ok_or("У Fake-IP DNS-сервера должен быть непустой tag.")?
                .to_owned()
        };
        let rules = dns
            .entry("rules")
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .ok_or("dns.rules должен быть массивом.")?;
        let has_fakeip_fallback = rules.iter().any(|rule| {
            rule.get("server").and_then(serde_json::Value::as_str) == Some(fakeip_tag.as_str())
                && rule
                    .get("domain_regex")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|patterns| {
                        patterns.len() == 1 && patterns[0].as_str() == Some(".*")
                    })
        });
        if !has_fakeip_fallback {
            rules.push(serde_json::json!({
                "domain_regex": [".*"],
                "server": fakeip_tag
            }));
        }
        let fakeip = dns.entry("fakeip").or_insert_with(|| serde_json::json!({}));
        let fakeip = fakeip
            .as_object_mut()
            .ok_or("dns.fakeip должен быть объектом.")?;
        fakeip.insert("enabled".to_owned(), serde_json::json!(true));
        fakeip
            .entry("inet4_range")
            .or_insert_with(|| serde_json::json!("198.18.0.0/15"));
        fakeip
            .entry("inet6_range")
            .or_insert_with(|| serde_json::json!("fc00::/18"));
    } else if let Some(dns) = root
        .get_mut("dns")
        .and_then(serde_json::Value::as_object_mut)
    {
        let removed_tags = dns
            .get("servers")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter(|server| is_sing_box_fakeip_server(server))
            .filter_map(|server| server.get("tag").and_then(serde_json::Value::as_str))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if let Some(servers) = dns
            .get_mut("servers")
            .and_then(serde_json::Value::as_array_mut)
        {
            servers.retain(|server| !is_sing_box_fakeip_server(server));
        }
        if let Some(rules) = dns
            .get_mut("rules")
            .and_then(serde_json::Value::as_array_mut)
        {
            rules.retain(|rule| {
                !rule
                    .get("server")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|tag| removed_tags.iter().any(|removed| removed == tag))
            });
        }
        let final_uses_removed_server = dns
            .get("final")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|tag| removed_tags.iter().any(|removed| removed == tag));
        if final_uses_removed_server {
            let fallback = dns
                .get("servers")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .find_map(|server| server.get("tag").and_then(serde_json::Value::as_str));
            if let Some(fallback) = fallback {
                dns.insert("final".to_owned(), serde_json::json!(fallback));
            } else {
                dns.remove("final");
            }
        }
        if let Some(fakeip) = dns
            .get_mut("fakeip")
            .and_then(serde_json::Value::as_object_mut)
        {
            fakeip.insert("enabled".to_owned(), serde_json::json!(false));
        }
    }
    Ok(())
}

pub(crate) fn prioritize_xray_fake_dns(servers: &mut Vec<serde_json::Value>) {
    let fake_dns_index = servers.iter().position(|server| {
        server.as_str() == Some("fakedns")
            || server.get("address").and_then(serde_json::Value::as_str) == Some("fakedns")
    });
    let fake_dns = fake_dns_index
        .map(|index| servers.remove(index))
        .unwrap_or_else(|| serde_json::json!("fakedns"));
    servers.insert(0, fake_dns);
}

pub(crate) fn split_rule_values(text: &str) -> Vec<String> {
    text.split(|character: char| character.is_whitespace() || character == ',' || character == ';')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

pub(crate) fn valid_ip_cidr(value: &str) -> bool {
    let Some((address, prefix)) = value.split_once('/') else {
        return false;
    };
    let Ok(address) = address.parse::<std::net::IpAddr>() else {
        return false;
    };
    let Ok(prefix) = prefix.parse::<u8>() else {
        return false;
    };
    prefix <= if address.is_ipv4() { 32 } else { 128 }
}

pub(crate) fn valid_domain_suffix(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && !value
            .chars()
            .any(|character| matches!(character, '/' | ':' | '@' | '?' | '#'))
        && value.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && label
                    .as_bytes()
                    .first()
                    .is_some_and(|byte| byte.is_ascii_alphanumeric())
                && label
                    .as_bytes()
                    .last()
                    .is_some_and(|byte| byte.is_ascii_alphanumeric())
        })
}

pub(crate) fn normalize_routing_domain(value: &str) -> Result<String, String> {
    let input = value.trim();
    let candidate = if input.contains("://") {
        input.to_owned()
    } else {
        format!("https://{input}")
    };
    let parsed = url::Url::parse(&candidate)
        .map_err(|_| format!("«{input}» не похоже на домен или URL сайта."))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(format!(
            "«{input}»: укажите обычный HTTP/HTTPS сайт без логина и пароля."
        ));
    }
    let domain = parsed
        .host_str()
        .ok_or_else(|| format!("«{input}» не содержит имени сайта."))?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if !valid_domain_suffix(&domain) {
        return Err(format!("«{input}» не похоже на доменное имя."));
    }
    Ok(domain)
}

pub(crate) fn add_routing_rule(
    text: &str,
    domains_text: &str,
    ips_text: &str,
    outbound: &str,
) -> Result<String, String> {
    let domains = split_rule_values(domains_text)
        .iter()
        .map(|domain| normalize_routing_domain(domain))
        .collect::<Result<Vec<_>, _>>()?;
    let ips = split_rule_values(ips_text);
    if domains.is_empty() && ips.is_empty() {
        return Err("Укажите хотя бы один домен или IP/CIDR.".into());
    }
    if let Some(ip) = ips.iter().find(|ip| !valid_ip_cidr(ip)) {
        return Err(format!("«{ip}» — некорректный IP/CIDR."));
    }
    let outbound = outbound.trim();
    if outbound.is_empty() {
        return Err("Укажите тег выходного узла.".into());
    }
    let mut value = parse_jsonc_value(text)?;
    let xray = is_xray_config(&value);
    let has_outbound = value
        .get("outbounds")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|item| {
                item.get("tag").and_then(serde_json::Value::as_str) == Some(outbound)
                    || (xray
                        && item.get("protocol").and_then(serde_json::Value::as_str)
                            == Some(outbound))
            })
        });
    if !has_outbound {
        return Err(format!("Выход «{outbound}» не найден в outbounds JSON."));
    }
    let mut rule = if xray {
        serde_json::json!({ "type":"field", "outboundTag":outbound })
    } else {
        serde_json::json!({ "action":"route", "outbound":outbound })
    };
    rule[MANAGED_ROUTE_MARKER] = serde_json::json!("v1");
    if xray {
        if !domains.is_empty() {
            rule["domain"] = serde_json::json!(
                domains
                    .iter()
                    .map(|domain| format!("domain:{domain}"))
                    .collect::<Vec<_>>()
            );
        }
        if !ips.is_empty() {
            rule["ip"] = serde_json::json!(ips);
        }
    } else {
        if !domains.is_empty() {
            rule["domain_suffix"] = serde_json::json!(domains);
        }
        if !ips.is_empty() {
            rule["ip_cidr"] = serde_json::json!(ips);
        }
    }
    let rules = route_rules_mut(&mut value, xray)?;
    let insert_at = if xray {
        0
    } else {
        rules
            .iter()
            .take_while(|rule| {
                matches!(
                    rule.get("action").and_then(serde_json::Value::as_str),
                    Some("sniff" | "hijack-dns")
                )
            })
            .count()
    };
    rules.insert(insert_at, rule);
    serde_json::to_string_pretty(&value)
        .map_err(|problem| format!("Не удалось записать JSON: {problem}"))
}

pub(crate) fn clear_managed_routing_rules(text: &str) -> Result<String, String> {
    let mut value = parse_jsonc_value(text)?;
    let xray = is_xray_config(&value);
    let rules = route_rules_mut(&mut value, xray)?;
    rules.retain(|rule| rule.get(MANAGED_ROUTE_MARKER).is_none());
    serde_json::to_string_pretty(&value)
        .map_err(|problem| format!("Не удалось записать JSON: {problem}"))
}

#[cfg(test)]
mod network_options_tests {
    use super::{default_dns_detour_tag, set_network_options, value_has_fakeip};

    const DIRECT_ONLY_CONFIG: &str = r#"{
        "inbounds": [{"type":"mixed","tag":"local","listen":"127.0.0.1","listen_port":1080}],
        "outbounds": [{"type":"direct","tag":"direct"}],
        "route": {"final":"direct"}
    }"#;

    const XRAY_DIRECT_CONFIG: &str = r#"{
        "inbounds": [{"listen":"127.0.0.1","port":1080,"protocol":"socks"}],
        "outbounds": [{"protocol":"freedom","tag":"direct","settings":{}}],
        "dns": {"servers":["9.9.9.9"]},
        "routing": {"rules":[]}
    }"#;

    #[test]
    fn xray_fake_dns_precedes_existing_catch_all_dns_and_is_idempotent() {
        let enabled = set_network_options(XRAY_DIRECT_CONFIG, false, true).unwrap();
        let enabled_again = set_network_options(&enabled, false, true).unwrap();
        let value: serde_json::Value = serde_json::from_str(&enabled_again).unwrap();
        let servers = value["dns"]["servers"].as_array().unwrap();
        assert_eq!(servers[0], "fakedns");
        assert_eq!(servers[1], "9.9.9.9");
        assert_eq!(
            servers.iter().filter(|server| *server == "fakedns").count(),
            1
        );
        assert_eq!(value["fakedns"][0]["ipPool"], "198.18.0.0/15");
        assert!(value_has_fakeip(&value));
    }

    #[test]
    fn xray_fake_dns_toggle_off_removes_managed_dns_entries() {
        let enabled = set_network_options(XRAY_DIRECT_CONFIG, false, true).unwrap();
        let disabled = set_network_options(&enabled, false, false).unwrap();
        let value: serde_json::Value = serde_json::from_str(&disabled).unwrap();
        assert!(value.get("fakedns").is_none());
        assert_eq!(value["dns"]["servers"], serde_json::json!(["9.9.9.9"]));
        assert!(!value_has_fakeip(&value));
    }

    #[test]
    fn xray_fake_dns_existing_custom_server_is_moved_to_first_without_duplication() {
        let config = r#"{
            "inbounds": [],
            "outbounds": [{"protocol":"freedom","tag":"direct"}],
            "dns": {"servers":["1.1.1.1",{"address":"fakedns","domains":["domain:example.org"]}]}
        }"#;
        let enabled = set_network_options(config, false, true).unwrap();
        let value: serde_json::Value = serde_json::from_str(&enabled).unwrap();
        let servers = value["dns"]["servers"].as_array().unwrap();
        assert_eq!(servers[0]["address"], "fakedns");
        assert_eq!(servers[0]["domains"][0], "domain:example.org");
        assert_eq!(servers[1], "1.1.1.1");
    }

    #[test]
    fn dns_detour_uses_the_configured_final_outbound() {
        let value: serde_json::Value = serde_json::from_str(DIRECT_ONLY_CONFIG).unwrap();
        assert_eq!(default_dns_detour_tag(&value).as_deref(), Some("direct"));
    }

    #[test]
    fn fakeip_adds_a_dns_server_with_an_existing_outbound_tag() {
        let configured = set_network_options(DIRECT_ONLY_CONFIG, false, true).unwrap();
        let value: serde_json::Value = serde_json::from_str(&configured).unwrap();
        assert_eq!(
            value["dns"]["servers"][0]["detour"].as_str(),
            Some("direct")
        );
        assert_eq!(value["dns"]["final"].as_str(), Some("remote"));
        assert_eq!(
            value["dns"]["rules"][0]["domain_regex"][0].as_str(),
            Some(".*")
        );
        assert_eq!(
            value["dns"]["rules"][0]["server"].as_str(),
            Some("reality-client-fakeip")
        );
    }

    #[test]
    fn disabling_fakeip_removes_its_server_and_returns_dns_final_to_upstream() {
        let enabled = set_network_options(DIRECT_ONLY_CONFIG, false, true).unwrap();
        let disabled = set_network_options(&enabled, false, false).unwrap();
        let value: serde_json::Value = serde_json::from_str(&disabled).unwrap();
        let servers = value["dns"]["servers"].as_array().unwrap();
        assert!(
            servers
                .iter()
                .all(|server| !super::is_sing_box_fakeip_server(server))
        );
        assert_eq!(value["dns"]["final"].as_str(), Some("remote"));
        assert_eq!(value["dns"]["rules"].as_array().unwrap().len(), 0);
        assert!(!value_has_fakeip(&value));
    }

    #[test]
    fn tun_dns_uses_a_real_outbound_when_route_final_is_absent() {
        let config = r#"{
            "inbounds": [],
            "outbounds": [{"type":"direct","tag":"internet"}]
        }"#;
        let configured = set_network_options(config, true, false).unwrap();
        let value: serde_json::Value = serde_json::from_str(&configured).unwrap();
        assert_eq!(
            value["dns"]["servers"][0]["detour"].as_str(),
            Some("internet")
        );
    }

    #[cfg(windows)]
    #[test]
    fn pinned_core_accepts_generated_fakeip_config_without_proxy_named_outbound() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let configured = set_network_options(DIRECT_ONLY_CONFIG, true, true).unwrap();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "reality-fakeip-config-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("config.json");
        std::fs::write(&config_path, configured).unwrap();
        let result = crate::core::check_config_file(&config_path);
        let _ = std::fs::remove_dir_all(&dir);
        result.unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn pinned_core_accepts_generated_xray_fake_dns_config() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let configured = set_network_options(XRAY_DIRECT_CONFIG, false, true).unwrap();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "reality-xray-fakedns-config-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("config.json");
        std::fs::write(&config_path, configured).unwrap();
        let result = crate::core::check_config_file(&config_path);
        let _ = std::fs::remove_dir_all(&dir);
        result.unwrap();
    }
}

#[cfg(test)]
mod routing_domain_tests {
    use super::normalize_routing_domain;

    #[test]
    fn routing_domain_accepts_hostnames_and_website_urls() {
        assert_eq!(normalize_routing_domain("vk.com").unwrap(), "vk.com");
        assert_eq!(
            normalize_routing_domain("https://VK.com/video?clip=1").unwrap(),
            "vk.com"
        );
        assert_eq!(
            normalize_routing_domain("http://news.example.org/").unwrap(),
            "news.example.org"
        );
    }

    #[test]
    fn routing_domain_rejects_credentials_and_non_web_schemes() {
        assert!(normalize_routing_domain("https://user:password@vk.com/").is_err());
        assert!(normalize_routing_domain("ftp://vk.com/").is_err());
        assert!(normalize_routing_domain("*.vk.com").is_err());
    }
}
