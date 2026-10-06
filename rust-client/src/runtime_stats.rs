//! Снимок состояния ядра: группы серверов, трафик, активные соединения.

use crate::ffi_session::CoreSession;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SelectableGroup {
    pub(crate) tag: String,
    pub(crate) members: Vec<String>,
    pub(crate) current: Option<String>,
}

pub(crate) struct RuntimeSnapshot {
    pub(crate) groups: Vec<SelectableGroup>,
    pub(crate) uploaded: u64,
    pub(crate) downloaded: u64,
    pub(crate) connections: u64,
    pub(crate) connection_rows: Vec<String>,
}

pub(crate) fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["Б", "КБ", "МБ", "ГБ", "ТБ"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

pub(crate) fn fetch_runtime_snapshot(session: &CoreSession) -> Result<RuntimeSnapshot, String> {
    let groups_value = session.runtime_api("/groups")?;
    let groups = groups_value
        .get("groups")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            if item.get("type").and_then(serde_json::Value::as_str) != Some("selector") {
                return None;
            }
            let tag = item.get("tag")?.as_str()?.to_owned();
            let members = item
                .get("members")?
                .as_array()?
                .iter()
                .filter_map(|member| {
                    member
                        .get("tag")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                })
                .collect();
            let current = item
                .get("current")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            Some(SelectableGroup {
                tag,
                members,
                current,
            })
        })
        .collect();
    let stats = session.runtime_api("/stats")?;
    let connection_list = session.runtime_api("/connections")?;
    let connection_rows = connection_list
        .get("connections")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .map(format_connection_row)
        .collect();
    Ok(RuntimeSnapshot {
        groups,
        uploaded: stats
            .get("up")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        downloaded: stats
            .get("down")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        connections: stats
            .get("connections")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        connection_rows,
    })
}

pub(crate) fn format_connection_row(connection: &serde_json::Value) -> String {
    let metadata = connection.get("metadata");
    let host = metadata
        .and_then(|metadata| metadata.get("host"))
        .and_then(serde_json::Value::as_str)
        .filter(|host| !host.is_empty())
        .or_else(|| {
            metadata
                .and_then(|metadata| metadata.get("destinationIP"))
                .and_then(serde_json::Value::as_str)
                .filter(|host| !host.is_empty())
        })
        .unwrap_or("Неизвестное назначение");
    let port = metadata
        .and_then(|metadata| metadata.get("destinationPort"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let network = metadata
        .and_then(|metadata| metadata.get("network"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("?");
    let inbound = metadata
        .and_then(|metadata| metadata.get("type"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("вход неизвестен");
    let chain = connection
        .get("chains")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()
                .join(" → ")
        })
        .filter(|chain| !chain.is_empty())
        .unwrap_or_else(|| "выход неизвестен".to_owned());
    let upload = connection
        .get("upload")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let download = connection
        .get("download")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    format!(
        "{host}{port_suffix}  ·  {network}  ·  {inbound}  ·  {chain}  ·  ↑ {}  ↓ {}",
        format_bytes(upload),
        format_bytes(download),
        port_suffix = if port.is_empty() {
            String::new()
        } else {
            format!(":{port}")
        },
    )
}

#[cfg(test)]
mod connection_row_tests {
    use super::format_connection_row;

    #[test]
    fn formats_core_connection_metadata_and_traffic() {
        let connection = serde_json::json!({
            "metadata": {
                "host": "vk.com",
                "destinationPort": "443",
                "network": "tcp",
                "type": "tun/tun"
            },
            "chains": ["selector", "reality-out"],
            "upload": 1024,
            "download": 2048
        });

        assert_eq!(
            format_connection_row(&connection),
            "vk.com:443  ·  tcp  ·  tun/tun  ·  selector → reality-out  ·  ↑ 1.0 КБ  ↓ 2.0 КБ"
        );
    }
}
