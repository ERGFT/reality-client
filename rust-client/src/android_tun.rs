//! Политика Android-TUN: что можно отдать `VpnService.Builder` по JSON-конфигурации ядра.
//!
//! Раньше жила в Kotlin (`AndroidTunPolicy.kt`, `TunAddress.kt`); теперь проверка и
//! расчёт адресов, DNS и маршрутов — здесь, и тесты идут на хосте без Android.
//! Kotlin только применяет готовый план к `Builder`.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use serde_json::{Map, Value, json};

const DEFAULT_IPV4: &str = "172.19.0.1/30";
const DEFAULT_IPV6: &str = "fdfe:dcba:9876::1/126";
const DEFAULT_MTU: u64 = 1500;

/// Параметры TUN, которые `VpnService.Builder` не умеет выразить или которых нет в ядре.
const UNSUPPORTED_TUN_FIELDS: &[&str] = &[
    "route_address",
    "inet4_route_address",
    "inet6_route_address",
    "route_address_set",
    "inet4_route_address_set",
    "inet6_route_address_set",
    "route_exclude_address",
    "inet4_route_exclude_address",
    "inet6_route_exclude_address",
    "route_exclude_address_set",
    "inet4_route_exclude_address_set",
    "inet6_route_exclude_address_set",
    "auto_redirect",
    "include_interface",
    "exclude_interface",
    "include_uid",
    "exclude_uid",
    "exclude_package",
    "include_android_user",
    "loopback_address",
    "iproute2_table_index",
    "iproute2_rule_index",
];

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Cidr {
    pub address: IpAddr,
    pub prefix: u8,
}

/// Готовый план для `VpnService.Builder`.
#[derive(Debug)]
pub(crate) struct TunPlan {
    /// Конфигурация для ядра: без `include_package` (его ядро не принимает).
    pub config: String,
    pub mtu: u32,
    /// `None` — все приложения.
    pub packages: Option<Vec<String>>,
    pub addresses: Vec<Cidr>,
    pub dns: Vec<IpAddr>,
    pub routes: Vec<Cidr>,
}

impl TunPlan {
    /// JSON для Kotlin-оболочки: `{"error": "…"}` или поля плана.
    pub(crate) fn to_json(&self) -> String {
        let cidr = |c: &Cidr| json!({ "ip": c.address.to_string(), "prefix": c.prefix });
        json!({
            "config": self.config,
            "mtu": self.mtu,
            "packages": self.packages,
            "addresses": self.addresses.iter().map(cidr).collect::<Vec<_>>(),
            "dns": self.dns.iter().map(IpAddr::to_string).collect::<Vec<_>>(),
            "routes": self.routes.iter().map(cidr).collect::<Vec<_>>(),
        })
        .to_string()
    }
}

pub(crate) fn plan_json(config: &str) -> String {
    match plan(config) {
        Ok(plan) => plan.to_json(),
        Err(error) => json!({ "error": error }).to_string(),
    }
}

pub(crate) fn plan(config: &str) -> Result<TunPlan, String> {
    let mut root: Value = serde_json::from_str(config)
        .map_err(|e| format!("Некорректный JSON Android-клиента: {e}"))?;
    let inbounds = root
        .get("inbounds")
        .and_then(Value::as_array)
        .ok_or("В конфигурации отсутствуют inbounds")?;
    let types: Vec<&str> = inbounds
        .iter()
        .map(|inbound| {
            let kind = |key: &str| inbound.get(key).and_then(Value::as_str).unwrap_or("");
            match kind("type") {
                "" => kind("protocol"),
                other => other,
            }
        })
        .collect();
    let tun_index = single_tun_inbound_index(&types)?;
    let has_dns_module = root.get("dns").is_some_and(Value::is_object);

    let tun = root["inbounds"][tun_index]
        .as_object()
        .ok_or("Входящий tun должен быть объектом")?;
    if !bool_option(tun, "dns_hijack", true)? || !has_dns_module {
        return Err("Android требует DNS-модуль и перехват DNS-запросов в TUN".into());
    }
    validate_tun_options(
        tun,
        bool_option(tun, "auto_route", true)?,
        bool_option(tun, "strict_route", false)?,
    )?;
    let mtu = mtu(tun)?;
    let packages = included_packages(tun)?;
    let addresses = effective_tun_addresses(&address_values(tun)?)?;

    let mut dns = Vec::new();
    for cidr in &addresses {
        let max = if cidr.address.is_ipv4() { 32 } else { 128 };
        if cidr.prefix < max {
            dns.push(dns_peer_address(cidr)?);
        }
    }
    if dns.is_empty() {
        return Err(
            "Для Android DNS требуется адрес TUN с доступным адресом DNS внутри подсети".into(),
        );
    }
    let routes = vec![
        Cidr {
            address: Ipv4Addr::UNSPECIFIED.into(),
            prefix: 0,
        },
        Cidr {
            address: Ipv6Addr::UNSPECIFIED.into(),
            prefix: 0,
        },
    ];

    // `include_package` — свойство только клиента; ядро его не принимает.
    // Убираем после полной проверки: неверный фильтр не должен молча пропадать.
    if let Some(tun) = root["inbounds"][tun_index].as_object_mut() {
        tun.remove("include_package");
    }
    Ok(TunPlan {
        config: root.to_string(),
        mtu,
        packages,
        addresses,
        dns,
        routes,
    })
}

fn single_tun_inbound_index(types: &[&str]) -> Result<usize, String> {
    let mut found = types
        .iter()
        .enumerate()
        .filter(|(_, kind)| **kind == "tun")
        .map(|(index, _)| index);
    match (found.next(), found.next()) {
        (None, _) => Err("Для Android требуется входящий тип tun".into()),
        (Some(index), None) => Ok(index),
        _ => Err("Android поддерживает ровно один входящий тип tun".into()),
    }
}

/// Логический параметр: нет — значение по умолчанию, не bool — ошибка (без молчаливых приведений).
fn bool_option(tun: &Map<String, Value>, key: &str, default: bool) -> Result<bool, String> {
    match tun.get(key) {
        None => Ok(default),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(format!("Параметр TUN {key} должен быть true или false")),
    }
}

fn mtu(tun: &Map<String, Value>) -> Result<u32, String> {
    let value = match tun.get("mtu") {
        None => DEFAULT_MTU,
        Some(value) => value
            .as_u64()
            .ok_or("Параметр TUN mtu должен быть целым числом")?,
    };
    if !(1..=65535).contains(&value) {
        return Err("Параметр TUN mtu должен быть от 1 до 65535".into());
    }
    Ok(value as u32)
}

fn validate_tun_options(
    tun: &Map<String, Value>,
    auto_route: bool,
    strict_route: bool,
) -> Result<(), String> {
    if let Some(field) = UNSUPPORTED_TUN_FIELDS
        .iter()
        .find(|field| tun.contains_key(**field))
    {
        return Err(format!("Android пока не поддерживает параметр TUN {field}"));
    }
    if !auto_route {
        return Err("Android пока не поддерживает отключение автоматических TUN-маршрутов".into());
    }
    if strict_route {
        return Err("Android пока не поддерживает strict_route для TUN".into());
    }
    Ok(())
}

/// `None` — фильтра нет (все приложения); любой неверный фильтр — ошибка.
fn included_packages(tun: &Map<String, Value>) -> Result<Option<Vec<String>>, String> {
    let Some(configured) = tun.get("include_package") else {
        return Ok(None);
    };
    let list = configured
        .as_array()
        .ok_or("include_package должен быть массивом Android package ID")?;
    if list.is_empty() {
        return Err("include_package должен содержать хотя бы одно приложение".into());
    }
    list.iter()
        .map(|value| {
            let name = value
                .as_str()
                .ok_or("Каждый элемент include_package должен быть строкой")?
                .trim();
            if name.is_empty() {
                return Err("Пустой Android package ID в include_package".to_string());
            }
            Ok(name.to_owned())
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

fn address_values(tun: &Map<String, Value>) -> Result<Vec<String>, String> {
    let mut values = Vec::new();
    for field in ["address", "inet4_address", "inet6_address"] {
        match tun.get(field) {
            None => {}
            Some(Value::String(value)) => values.push(value.clone()),
            Some(Value::Array(items)) => {
                for item in items {
                    values.push(
                        item.as_str()
                            .ok_or_else(|| {
                                format!("Поле {field} TUN должно быть строкой или массивом строк")
                            })?
                            .to_owned(),
                    );
                }
            }
            Some(_) => {
                return Err(format!(
                    "Поле {field} TUN должно быть строкой или массивом строк"
                ));
            }
        }
    }
    Ok(values)
}

/// Как ядро: первый IPv4 и первый IPv6 из настроек, иначе значения по умолчанию.
fn effective_tun_addresses(configured: &[String]) -> Result<Vec<Cidr>, String> {
    let parsed = configured
        .iter()
        .map(|value| parse_cidr(value))
        .collect::<Result<Vec<_>, _>>()?;
    let pick = |want_v4: bool, default: &str| -> Result<Cidr, String> {
        match parsed.iter().position(|c| c.address.is_ipv4() == want_v4) {
            Some(index) => Ok(Cidr {
                address: parsed[index].address,
                prefix: parsed[index].prefix,
            }),
            None => parse_cidr(default),
        }
    };
    let ipv4 = pick(true, DEFAULT_IPV4)?;
    let ipv6 = pick(false, DEFAULT_IPV6)?;
    if ipv4.prefix > 30 {
        return Err("IPv4-адрес TUN должен иметь префикс не больше /30".into());
    }
    Ok(vec![ipv4, ipv6])
}

fn parse_cidr(value: &str) -> Result<Cidr, String> {
    let (address, prefix) = value
        .split_once('/')
        .filter(|(_, prefix)| !prefix.contains('/'))
        .ok_or("Некорректный адрес TUN")?;
    if address.is_empty() || address.contains('%') {
        return Err(
            "Адрес TUN должен быть числовым IPv4 или IPv6-адресом без зоны интерфейса".into(),
        );
    }
    let address: IpAddr = address
        .parse()
        .map_err(|_| "Адрес TUN должен быть числовым IPv4 или IPv6-адресом".to_string())?;
    let prefix: u8 = prefix
        .parse()
        .map_err(|_| "Некорректный префикс адреса TUN".to_string())?;
    let max = if address.is_ipv4() { 32 } else { 128 };
    if prefix > max {
        return Err("Некорректный префикс адреса TUN".into());
    }
    if address.is_unspecified() || address.is_multicast() {
        return Err("Адрес TUN должен быть адресом узла, а не unspecified или multicast".into());
    }
    if let IpAddr::V4(v4) = address
        && prefix < 31
        && !is_ipv4_host(v4, prefix)
    {
        return Err("IPv4-адрес TUN не должен совпадать с network или broadcast адресом".into());
    }
    Ok(Cidr { address, prefix })
}

/// Не адрес сети и не широковещательный (для /31 и /32 различия нет).
fn is_ipv4_host(address: Ipv4Addr, prefix: u8) -> bool {
    let host_mask = u32::MAX.checked_shr(u32::from(prefix)).unwrap_or(0);
    let host_bits = u32::from(address) & host_mask;
    host_bits != 0 && host_bits != host_mask
}

/// Соседний адрес в той же подсети, который можно отдать как DNS: сначала следующий, потом предыдущий.
fn dns_peer_address(cidr: &Cidr) -> Result<IpAddr, String> {
    let (bits, width): (u128, u32) = match cidr.address {
        IpAddr::V4(v4) => (u128::from(u32::from(v4)), 32),
        IpAddr::V6(v6) => (u128::from(v6), 128),
    };
    let width_mask = u128::MAX >> (128 - width);
    let net_mask = if cidr.prefix == 0 {
        0
    } else {
        (u128::MAX << (width - u32::from(cidr.prefix))) & width_mask
    };
    let to_ip = |value: u128| -> IpAddr {
        if width == 32 {
            Ipv4Addr::from(value as u32).into()
        } else {
            Ipv6Addr::from(value).into()
        }
    };
    let top = if width == 32 {
        u128::from(u32::MAX)
    } else {
        u128::MAX
    };
    let candidates = [(bits < top).then(|| bits + 1), bits.checked_sub(1)];
    for candidate in candidates.into_iter().flatten() {
        if candidate & net_mask != bits & net_mask {
            continue;
        }
        let ip = to_ip(candidate);
        if let IpAddr::V4(v4) = ip
            && cidr.prefix < 31
            && !is_ipv4_host(v4, cidr.prefix)
        {
            continue;
        }
        if ip.is_unspecified() || ip.is_multicast() {
            continue;
        }
        return Ok(ip);
    }
    Err("У адреса TUN нет свободного адреса DNS в своей подсети".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cidr(value: &str) -> Cidr {
        parse_cidr(value).unwrap()
    }

    fn tun_config(extra: &str) -> String {
        format!(
            r#"{{"dns":{{"servers":[]}},"inbounds":[{{"type":"mixed","listen_port":1080}},{{"type":"tun"{extra}}}],"outbounds":[]}}"#
        )
    }

    fn rejected(config: &str) -> String {
        plan(config).expect_err("конфигурация должна быть отклонена")
    }

    #[test]
    fn package_filter_is_trimmed_and_removed_from_the_core_config() {
        let plan = plan(&tun_config(
            r#","include_package":[" com.example.vpn ","org.example.chat"]"#,
        ))
        .unwrap();
        assert_eq!(
            plan.packages,
            Some(vec!["com.example.vpn".into(), "org.example.chat".into()])
        );
        assert!(!plan.config.contains("include_package"));
    }

    #[test]
    fn absent_package_filter_means_all_applications() {
        let plan = plan(&tun_config("")).unwrap();
        assert_eq!(plan.packages, None);
    }

    #[test]
    fn malformed_package_filters_fail_closed() {
        for filter in [
            r#""com.example.vpn""#,
            "{}",
            "null",
            "[]",
            r#"[""]"#,
            "[7]",
            r#"["  "]"#,
        ] {
            rejected(&tun_config(&format!(r#","include_package":{filter}"#)));
        }
    }

    #[test]
    fn exactly_one_tun_inbound_is_required() {
        assert_eq!(single_tun_inbound_index(&["mixed", "tun", "direct"]), Ok(1));
        assert!(single_tun_inbound_index(&["mixed", "direct"]).is_err());
        assert!(single_tun_inbound_index(&["tun", "tun"]).is_err());
    }

    #[test]
    fn inbound_kind_may_come_from_protocol() {
        let config = r#"{"dns":{},"inbounds":[{"protocol":"tun"}]}"#;
        assert!(plan(config).is_ok());
    }

    #[test]
    fn unsupported_tun_options_are_rejected_before_establish() {
        for field in UNSUPPORTED_TUN_FIELDS {
            let error = rejected(&tun_config(&format!(r#","{field}":[]"#)));
            assert!(error.contains(field), "{field}: {error}");
        }
        rejected(&tun_config(r#","auto_route":false"#));
        rejected(&tun_config(r#","strict_route":true"#));
    }

    #[test]
    fn non_boolean_flags_are_rejected_instead_of_coerced() {
        rejected(&tun_config(r#","strict_route":"false""#));
        rejected(&tun_config(r#","dns_hijack":0"#));
    }

    #[test]
    fn dns_module_and_hijack_are_required() {
        rejected(r#"{"inbounds":[{"type":"tun"}]}"#);
        rejected(&tun_config(r#","dns_hijack":false"#));
    }

    #[test]
    fn mtu_defaults_and_is_validated() {
        assert_eq!(plan(&tun_config("")).unwrap().mtu, 1500);
        assert_eq!(plan(&tun_config(r#","mtu":1400"#)).unwrap().mtu, 1400);
        rejected(&tun_config(r#","mtu":0"#));
        rejected(&tun_config(r#","mtu":70000"#));
        rejected(&tun_config(r#","mtu":"1500""#));
    }

    #[test]
    fn omitted_addresses_use_core_defaults() {
        let plan = plan(&tun_config("")).unwrap();
        assert_eq!(
            plan.addresses,
            vec![cidr("172.19.0.1/30"), cidr("fdfe:dcba:9876::1/126")]
        );
        assert_eq!(
            plan.dns,
            vec![
                "172.19.0.2".parse::<IpAddr>().unwrap(),
                "fdfe:dcba:9876::2".parse::<IpAddr>().unwrap()
            ]
        );
        assert_eq!(plan.routes.len(), 2);
        assert!(plan.routes.iter().all(|route| route.prefix == 0));
    }

    #[test]
    fn address_fields_mirror_the_core_first_of_each_family_wins() {
        let addresses = effective_tun_addresses(&[
            "fd00::1/126".into(),
            "10.9.0.1/24".into(),
            "10.10.0.1/24".into(),
        ])
        .unwrap();
        assert_eq!(addresses, vec![cidr("10.9.0.1/24"), cidr("fd00::1/126")]);
    }

    #[test]
    fn address_fields_accept_string_or_array() {
        let plan = plan(&tun_config(
            r#","address":"10.8.0.1/24","inet6_address":["fd01::1/126"]"#,
        ))
        .unwrap();
        assert_eq!(
            plan.addresses,
            vec![cidr("10.8.0.1/24"), cidr("fd01::1/126")]
        );
        rejected(&tun_config(r#","address":7"#));
        rejected(&tun_config(r#","address":[7]"#));
    }

    #[test]
    fn ipv4_prefix_longer_than_30_is_rejected() {
        assert!(effective_tun_addresses(&["192.0.2.1/31".into()]).is_err());
    }

    #[test]
    fn derives_dns_peer_inside_the_tunnel_subnet() {
        assert_eq!(
            dns_peer_address(&cidr("172.19.0.1/30")).unwrap(),
            "172.19.0.2".parse::<IpAddr>().unwrap()
        );
        assert_eq!(
            dns_peer_address(&cidr("fd00::1/126")).unwrap(),
            "fd00::2".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn dns_peer_avoids_the_ipv4_broadcast_address() {
        assert_eq!(
            dns_peer_address(&cidr("192.0.2.2/30")).unwrap(),
            "192.0.2.1".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn dns_peer_works_on_either_side_of_a_point_to_point_subnet() {
        assert_eq!(
            dns_peer_address(&cidr("192.0.2.0/31")).unwrap(),
            "192.0.2.1".parse::<IpAddr>().unwrap()
        );
        assert_eq!(
            dns_peer_address(&cidr("192.0.2.1/31")).unwrap(),
            "192.0.2.0".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn dns_peer_never_leaves_the_subnet_or_wraps_around() {
        assert!(
            dns_peer_address(&Cidr {
                address: "255.255.255.255".parse().unwrap(),
                prefix: 32
            })
            .is_err()
        );
        assert!(dns_peer_address(&cidr("10.0.0.1/32")).is_err());
    }

    #[test]
    fn hostnames_scopes_and_special_addresses_are_rejected_without_lookup() {
        for value in [
            "vpn.example/24",
            "fe80::1%wlan0/64",
            "0.0.0.0/24",
            "224.0.0.1/24",
            "ff02::1/64",
            "10.0.0.1",
            "10.0.0.1/24/1",
            "/24",
        ] {
            assert!(parse_cidr(value).is_err(), "{value}");
        }
    }

    #[test]
    fn invalid_prefix_and_network_or_broadcast_addresses_are_rejected() {
        for value in [
            "192.0.2.1/33",
            "192.0.2.1/x",
            "::1/129",
            "192.0.2.0/30",
            "192.0.2.3/30",
        ] {
            assert!(parse_cidr(value).is_err(), "{value}");
        }
    }

    #[test]
    fn plan_json_carries_the_plan_or_an_error() {
        let ok: Value = serde_json::from_str(&plan_json(&tun_config(""))).unwrap();
        assert_eq!(ok["mtu"], 1500);
        assert_eq!(ok["addresses"][0]["ip"], "172.19.0.1");
        assert_eq!(ok["addresses"][0]["prefix"], 30);
        assert_eq!(ok["dns"][0], "172.19.0.2");
        assert!(ok["packages"].is_null());
        let bad: Value = serde_json::from_str(&plan_json("не json")).unwrap();
        assert!(bad["error"].as_str().is_some_and(|e| e.contains("JSON")));
    }
}
