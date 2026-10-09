use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use windows::Win32::{
    Foundation::WIN32_ERROR,
    Networking::WinInet::{
        INTERNET_OPTION_REFRESH, INTERNET_OPTION_SETTINGS_CHANGED, InternetSetOptionW,
    },
    System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_VALUE_TYPE, RegCloseKey,
        RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    },
};
use windows::core::w;

const SNAPSHOT_HEADER: &str = "RPROXY1";
const LOCAL_PROXY: &str = "127.0.0.1:1080";
const BYPASS: &str = "localhost;127.*;10.*;172.16.*;172.17.*;172.18.*;172.19.*;172.20.*;172.21.*;172.22.*;172.23.*;172.24.*;172.25.*;172.26.*;172.27.*;172.28.*;172.29.*;172.30.*;172.31.*;192.168.*;<local>";
const ERROR_FILE_NOT_FOUND: u32 = 2;
const MAX_REGISTRY_VALUE: usize = 1024 * 1024;
const MAX_SNAPSHOT_STRING: usize = 2 * MAX_REGISTRY_VALUE;
const MAX_SNAPSHOT_SIZE: usize = 3 * MAX_SNAPSHOT_STRING + 128;

#[derive(Clone)]
struct ValueSnapshot {
    exists: bool,
    kind: u32,
    data: Vec<u8>,
}

struct ProxySnapshot {
    enable: ValueSnapshot,
    server: ValueSnapshot,
    bypass: ValueSnapshot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RecoveryDecision {
    AlreadyRestored,
    RestoreClientOwnedProxy,
    PreserveForeignProxy,
}

struct RegistryHandle(HKEY);

impl Drop for RegistryHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProxyValue {
    Server,
    Bypass,
    Enable,
}

trait ProxyBackend {
    fn read_snapshot(&mut self) -> Result<ProxySnapshot, String>;
    fn write_value(&mut self, name: ProxyValue, value: &ValueSnapshot) -> Result<(), String>;
    fn notify_changed(&mut self) -> Result<(), String>;
}

struct RegistryBackend(RegistryHandle);

impl RegistryBackend {
    fn open(access: windows::Win32::System::Registry::REG_SAM_FLAGS) -> Result<Self, String> {
        open_settings(access).map(Self)
    }
}

impl ProxyBackend for RegistryBackend {
    fn read_snapshot(&mut self) -> Result<ProxySnapshot, String> {
        ProxySnapshot::read_from(&self.0)
    }

    fn write_value(&mut self, name: ProxyValue, value: &ValueSnapshot) -> Result<(), String> {
        let registry_name = match name {
            ProxyValue::Server => w!("ProxyServer"),
            ProxyValue::Bypass => w!("ProxyOverride"),
            ProxyValue::Enable => w!("ProxyEnable"),
        };
        restore_value(&self.0, registry_name, value)
    }

    fn notify_changed(&mut self) -> Result<(), String> {
        notify_proxy_change()
    }
}

impl ProxySnapshot {
    fn capture() -> Result<Self, String> {
        let mut backend = RegistryBackend::open(KEY_QUERY_VALUE | KEY_SET_VALUE)?;
        let mut snapshot = backend.read_snapshot()?;
        if snapshot.is_enabled_at(LOCAL_PROXY)? {
            snapshot.enable = ValueSnapshot {
                exists: true,
                kind: 4,
                data: 0i32.to_le_bytes().to_vec(),
            };
        }
        // The shared C# recovery format can represent DWORD/QWORD and strings.
        for value in [&snapshot.enable, &snapshot.server, &snapshot.bypass] {
            encode_legacy_value(value)?;
        }
        Ok(snapshot)
    }

    fn is_enabled_at(&self, address: &str) -> Result<bool, String> {
        if !self.enable.exists || !self.server.exists {
            return Ok(false);
        }
        let enabled = read_integer(&self.enable)? == 1;
        let server = decode_registry_text(&self.server)?;
        Ok(enabled && server.eq_ignore_ascii_case(address))
    }

    fn save(&self, path: &Path) -> Result<(), String> {
        if path.exists() {
            return Err("Найден файл незавершённого восстановления системного прокси.".into());
        }
        let mut bytes = Vec::new();
        write_dotnet_string(&mut bytes, SNAPSHOT_HEADER)?;
        for value in [&self.enable, &self.server, &self.bypass] {
            write_legacy_value(&mut bytes, value)?;
        }
        let temp = path.with_extension("proxy-backup.tmp");
        let mut file = File::create(&temp)
            .map_err(|e| format!("Не удалось создать резервную копию прокси: {e}"))?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|e| format!("Не удалось записать резервную копию прокси: {e}"))?;
        drop(file);
        fs::rename(&temp, path)
            .map_err(|e| format!("Не удалось сохранить резервную копию прокси: {e}"))
    }

    fn load(path: &Path) -> Result<Self, String> {
        let mut bytes = Vec::new();
        File::open(path)
            .map_err(|e| format!("Не удалось прочитать резервную копию прокси: {e}"))?
            .take((MAX_SNAPSHOT_SIZE + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("Не удалось прочитать резервную копию прокси: {e}"))?;
        if bytes.len() > MAX_SNAPSHOT_SIZE {
            return Err("Файл восстановления прокси превышает допустимый размер.".into());
        }
        let mut reader = SnapshotReader {
            bytes: &bytes,
            pos: 0,
        };
        if reader.dotnet_string()?.as_bytes() != SNAPSHOT_HEADER.as_bytes() {
            return Err("Файл восстановления прокси имеет неизвестный формат.".into());
        }
        let enable = reader.legacy_value()?;
        let server = reader.legacy_value()?;
        let bypass = reader.legacy_value()?;
        if reader.pos != bytes.len() {
            return Err("В файле восстановления прокси обнаружены лишние данные.".into());
        }
        Ok(Self {
            enable,
            server,
            bypass,
        })
    }

    fn restore_with_backend<B: ProxyBackend>(&self, backend: &mut B) -> Result<bool, String> {
        let current = backend.read_snapshot()?;
        match self.recovery_decision(&current)? {
            RecoveryDecision::AlreadyRestored => {
                backend.notify_changed()?;
                return Ok(true);
            }
            RecoveryDecision::PreserveForeignProxy => return Ok(false),
            RecoveryDecision::RestoreClientOwnedProxy => {}
        }

        // Restore ProxyServer last. If either earlier write fails, the local
        // endpoint still identifies this as our incomplete recovery on retry.
        backend.write_value(ProxyValue::Bypass, &self.bypass)?;
        backend.write_value(ProxyValue::Enable, &self.enable)?;
        backend.write_value(ProxyValue::Server, &self.server)?;
        backend.notify_changed()?;
        Ok(true)
    }

    fn read_from(key: &RegistryHandle) -> Result<Self, String> {
        Ok(Self {
            enable: read_value(key, w!("ProxyEnable"))?,
            server: read_value(key, w!("ProxyServer"))?,
            bypass: read_value(key, w!("ProxyOverride"))?,
        })
    }

    fn server_is_local(&self) -> Result<bool, String> {
        Ok(self.server.exists
            && decode_registry_text(&self.server)?.eq_ignore_ascii_case(LOCAL_PROXY))
    }

    fn matches(&self, current: &Self) -> Result<bool, String> {
        Ok(values_match(&self.enable, &current.enable)?
            && values_match(&self.server, &current.server)?
            && values_match(&self.bypass, &current.bypass)?)
    }

    fn recovery_decision(&self, current: &Self) -> Result<RecoveryDecision, String> {
        if self.matches(current)? {
            return Ok(RecoveryDecision::AlreadyRestored);
        }

        if current.server_is_local()? {
            // The backup belongs to this client only while the proxy settings
            // still match either the state we installed or a partial restore
            // toward the saved state. A local endpoint by itself is not enough:
            // the user may have changed ProxyEnable or ProxyOverride while the
            // session was active, and recovery must not overwrite that choice.
            let enable_is_owned =
                current.is_enabled_at(LOCAL_PROXY)? || values_match(&self.enable, &current.enable)?;
            let bypass_is_owned = values_match(&self.bypass, &current.bypass)?
                || (current.bypass.exists
                    && decode_registry_text(&current.bypass)?.eq_ignore_ascii_case(BYPASS));
            if enable_is_owned && bypass_is_owned {
                return Ok(RecoveryDecision::RestoreClientOwnedProxy);
            }
        }

        Ok(RecoveryDecision::PreserveForeignProxy)
    }
}

pub fn snapshot_path() -> Result<PathBuf, String> {
    super::core::default_config_path().map(|p| p.with_file_name("proxy-backup.dat"))
}

pub fn prepare_system_proxy_backup() -> Result<PathBuf, String> {
    let path = snapshot_path()?;
    let snapshot = ProxySnapshot::capture()?;
    snapshot.save(&path)?;
    Ok(path)
}

pub fn enable_system_proxy() -> Result<(), String> {
    let mut backend = RegistryBackend::open(KEY_SET_VALUE)?;
    enable_with_backend(&mut backend)?;
    if !active_is_local_proxy()? {
        return Err("Windows не сохранила системный прокси Reality Client; возможно, его изменило другое приложение.".into());
    }
    Ok(())
}

fn enable_with_backend<B: ProxyBackend>(backend: &mut B) -> Result<(), String> {
    let server = string_registry_value(LOCAL_PROXY);
    let bypass = string_registry_value(BYPASS);
    let enabled = ValueSnapshot {
        exists: true,
        kind: 4,
        data: 1i32.to_le_bytes().to_vec(),
    };
    // Match the pinned core ordering: publish endpoint and bypass rules first,
    // then enable the proxy only after both values are present.
    backend.write_value(ProxyValue::Server, &server)?;
    backend.write_value(ProxyValue::Bypass, &bypass)?;
    backend.write_value(ProxyValue::Enable, &enabled)?;
    backend.notify_changed()
}

fn notify_proxy_change() -> Result<(), String> {
    unsafe { InternetSetOptionW(None, INTERNET_OPTION_SETTINGS_CHANGED, None, 0) }
        .map_err(|e| format!("Windows не подтвердила изменение прокси: {e}"))?;
    unsafe { InternetSetOptionW(None, INTERNET_OPTION_REFRESH, None, 0) }
        .map_err(|e| format!("Windows не обновила настройки прокси: {e}"))?;
    Ok(())
}

pub fn recover_system_proxy(path: &Path) -> Result<bool, String> {
    if !path.exists() {
        return Ok(false);
    }
    let mut backend = RegistryBackend::open(KEY_QUERY_VALUE | KEY_SET_VALUE)?;
    recover_with_backend(path, &mut backend)
}

fn recover_with_backend<B: ProxyBackend>(path: &Path, backend: &mut B) -> Result<bool, String> {
    if !path.exists() {
        return Ok(false);
    }
    let snapshot = ProxySnapshot::load(path)?;
    let restored = snapshot.restore_with_backend(backend)?;
    if !restored {
        // Another proxy may own the current settings. Keep the snapshot so the
        // previous configuration is not silently discarded.
        return Ok(false);
    }
    fs::remove_file(path).map_err(|e| {
        format!("Состояние прокси восстановлено, но backup не удалось удалить: {e}")
    })?;
    Ok(restored)
}

pub fn active_is_local_proxy() -> Result<bool, String> {
    let mut backend = RegistryBackend::open(KEY_QUERY_VALUE)?;
    backend.read_snapshot()?.is_enabled_at(LOCAL_PROXY)
}

fn open_settings(
    access: windows::Win32::System::Registry::REG_SAM_FLAGS,
) -> Result<RegistryHandle, String> {
    let mut key = HKEY::default();
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings"),
            None,
            access,
            &mut key,
        )
    };
    if status != WIN32_ERROR(0) {
        return Err(format!(
            "Не удалось открыть настройки прокси пользователя Windows (код {}).",
            status.0
        ));
    }
    Ok(RegistryHandle(key))
}

fn read_value(key: &RegistryHandle, name: windows::core::PCWSTR) -> Result<ValueSnapshot, String> {
    let mut kind = REG_VALUE_TYPE(0);
    let mut size = 0u32;
    let status =
        unsafe { RegQueryValueExW(key.0, name, None, Some(&mut kind), None, Some(&mut size)) };
    if status.0 == ERROR_FILE_NOT_FOUND {
        return Ok(ValueSnapshot {
            exists: false,
            kind: 0,
            data: Vec::new(),
        });
    }
    if status != WIN32_ERROR(0) || size as usize > MAX_REGISTRY_VALUE {
        return Err(format!(
            "Не удалось прочитать значение настроек прокси (код {}).",
            status.0
        ));
    }
    let mut data = vec![0u8; size as usize];
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            name,
            None,
            Some(&mut kind),
            Some(data.as_mut_ptr()),
            Some(&mut size),
        )
    };
    if status != WIN32_ERROR(0) {
        return Err(format!(
            "Не удалось прочитать настройки прокси (код {}).",
            status.0
        ));
    }
    data.truncate(size as usize);
    Ok(ValueSnapshot {
        exists: true,
        kind: kind.0,
        data,
    })
}

fn restore_value(
    key: &RegistryHandle,
    name: windows::core::PCWSTR,
    value: &ValueSnapshot,
) -> Result<(), String> {
    let status = if value.exists {
        unsafe {
            RegSetValueExW(
                key.0,
                name,
                None,
                REG_VALUE_TYPE(value.kind),
                Some(&value.data),
            )
        }
    } else {
        unsafe { RegDeleteValueW(key.0, name) }
    };
    if status.0 != 0 && !(status.0 == ERROR_FILE_NOT_FOUND && !value.exists) {
        return Err(format!(
            "Не удалось восстановить значение прокси (код {}).",
            status.0
        ));
    }
    Ok(())
}

fn read_integer(value: &ValueSnapshot) -> Result<i64, String> {
    match (value.kind, value.data.as_slice()) {
        (4, bytes) if bytes.len() == 4 => Ok(i32::from_le_bytes(bytes.try_into().unwrap()) as i64),
        (11, bytes) if bytes.len() == 8 => Ok(i64::from_le_bytes(bytes.try_into().unwrap())),
        _ => Err("Значение ProxyEnable имеет неожиданный тип реестра.".into()),
    }
}

fn values_match(expected: &ValueSnapshot, actual: &ValueSnapshot) -> Result<bool, String> {
    if expected.exists != actual.exists {
        return Ok(false);
    }
    if !expected.exists {
        return Ok(true);
    }
    if expected.kind != actual.kind {
        return Ok(false);
    }
    match expected.kind {
        4 | 11 => Ok(read_integer(expected)? == read_integer(actual)?),
        1 | 2 => Ok(decode_registry_text(expected)? == decode_registry_text(actual)?),
        _ => Ok(expected.data == actual.data),
    }
}

fn decode_registry_text(value: &ValueSnapshot) -> Result<String, String> {
    if value.kind != 1 && value.kind != 2 {
        return Err("Настройка ProxyServer имеет неподдерживаемый тип реестра.".into());
    }
    if !value.data.len().is_multiple_of(2) {
        return Err("Строковое значение прокси повреждено.".into());
    }
    let (pairs, _) = value.data.as_chunks::<2>();
    let words = pairs
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .take_while(|&c| c != 0)
        .collect::<Vec<_>>();
    String::from_utf16(&words)
        .map_err(|_| "Строковое значение прокси содержит некорректный UTF-16.".into())
}

fn string_registry_value(value: &str) -> ValueSnapshot {
    let mut data = value
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    data.extend_from_slice(&[0, 0]);
    ValueSnapshot {
        exists: true,
        kind: 1,
        data,
    }
}

fn encode_legacy_value(value: &ValueSnapshot) -> Result<(), String> {
    if !value.exists {
        return Ok(());
    }
    match value.kind {
        4 => {
            read_integer(value)?;
            Ok(())
        }
        11 => {
            read_integer(value)?;
            Ok(())
        }
        1 | 2 => {
            decode_registry_text(value)?;
            Ok(())
        }
        _ => Err(format!(
            "Тип одного из значений прокси ({}) нельзя безопасно сохранить.",
            value.kind
        )),
    }
}

fn write_legacy_value(out: &mut Vec<u8>, value: &ValueSnapshot) -> Result<(), String> {
    out.push(u8::from(value.exists));
    if !value.exists {
        return Ok(());
    }
    out.extend_from_slice(&(value.kind as i32).to_le_bytes());
    match value.kind {
        4 => out.extend_from_slice(&(read_integer(value)? as i32).to_le_bytes()),
        11 => out.extend_from_slice(&read_integer(value)?.to_le_bytes()),
        1 | 2 => write_dotnet_string(out, &decode_registry_text(value)?)?,
        _ => return Err("Тип значения прокси нельзя сериализовать.".into()),
    }
    Ok(())
}

fn write_dotnet_string(out: &mut Vec<u8>, text: &str) -> Result<(), String> {
    let bytes = text.as_bytes();
    let mut length = bytes.len() as u32;
    while length >= 0x80 {
        out.push((length as u8) | 0x80);
        length >>= 7;
    }
    out.push(length as u8);
    out.extend_from_slice(bytes);
    Ok(())
}

struct SnapshotReader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl SnapshotReader<'_> {
    fn take(&mut self, count: usize) -> Result<&[u8], String> {
        let end = self
            .pos
            .checked_add(count)
            .ok_or("Файл восстановления прокси повреждён.")?;
        let value = self
            .bytes
            .get(self.pos..end)
            .ok_or("Файл восстановления прокси повреждён.")?;
        self.pos = end;
        Ok(value)
    }

    fn dotnet_string(&mut self) -> Result<String, String> {
        let mut length = 0usize;
        let mut shift = 0;
        loop {
            let byte = *self.take(1)?.first().unwrap();
            if shift >= usize::BITS || ((byte & 0x7f) as usize).checked_shl(shift).is_none() {
                return Err("Длина строки в файле восстановления повреждена.".into());
            }
            length |= ((byte & 0x7f) as usize) << shift;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift > 28 {
                return Err("Длина строки в файле восстановления слишком велика.".into());
            }
        }
        if length > MAX_SNAPSHOT_STRING {
            return Err("Строка в файле восстановления слишком велика.".into());
        }
        String::from_utf8(self.take(length)?.to_vec())
            .map_err(|_| "Файл восстановления содержит некорректный UTF-8.".into())
    }

    fn legacy_value(&mut self) -> Result<ValueSnapshot, String> {
        let exists = self.take(1)?[0] != 0;
        if !exists {
            return Ok(ValueSnapshot {
                exists,
                kind: 0,
                data: Vec::new(),
            });
        }
        let kind = i32::from_le_bytes(self.take(4)?.try_into().unwrap()) as u32;
        let data = match kind {
            4 => self.take(4)?.to_vec(),
            11 => self.take(8)?.to_vec(),
            1 | 2 => {
                let value = self.dotnet_string()?;
                let mut bytes = value
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>();
                bytes.extend_from_slice(&[0, 0]);
                bytes
            }
            _ => {
                return Err(format!(
                    "Тип в backup прокси ({kind}) нельзя безопасно восстановить."
                ));
            }
        };
        Ok(ValueSnapshot { exists, kind, data })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MemoryBackend {
        current: ProxySnapshot,
        writes: Vec<ProxyValue>,
        fail_on: Option<ProxyValue>,
        notifications: usize,
        fail_notification: bool,
    }

    impl ProxyBackend for MemoryBackend {
        fn read_snapshot(&mut self) -> Result<ProxySnapshot, String> {
            Ok(ProxySnapshot {
                enable: self.current.enable.clone(),
                server: self.current.server.clone(),
                bypass: self.current.bypass.clone(),
            })
        }

        fn write_value(&mut self, name: ProxyValue, value: &ValueSnapshot) -> Result<(), String> {
            self.writes.push(name);
            if self.fail_on == Some(name) {
                return Err("simulated write failure".into());
            }
            match name {
                ProxyValue::Enable => self.current.enable = value.clone(),
                ProxyValue::Server => self.current.server = value.clone(),
                ProxyValue::Bypass => self.current.bypass = value.clone(),
            }
            Ok(())
        }

        fn notify_changed(&mut self) -> Result<(), String> {
            self.notifications += 1;
            if self.fail_notification {
                Err("simulated notification failure".into())
            } else {
                Ok(())
            }
        }
    }

    fn memory_backend(server: &str, enabled: i32) -> MemoryBackend {
        MemoryBackend {
            current: ProxySnapshot {
                enable: ValueSnapshot {
                    exists: true,
                    kind: 4,
                    data: enabled.to_le_bytes().to_vec(),
                },
                server: string_registry_value(server),
                bypass: string_registry_value("localhost;<local>"),
            },
            writes: Vec::new(),
            fail_on: None,
            notifications: 0,
            fail_notification: false,
        }
    }

    fn temporary_backup_path() -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "reality-proxy-backup-test-{}-{nonce}.dat",
            std::process::id()
        ))
    }

    #[test]
    fn recovery_snapshot_roundtrips_dotnet_value_encoding() {
        let original = ProxySnapshot {
            enable: ValueSnapshot {
                exists: true,
                kind: 4,
                data: 1i32.to_le_bytes().to_vec(),
            },
            server: string_registry_value("127.0.0.1:1080"),
            bypass: ValueSnapshot {
                exists: false,
                kind: 0,
                data: Vec::new(),
            },
        };
        let mut bytes = Vec::new();
        write_dotnet_string(&mut bytes, SNAPSHOT_HEADER).unwrap();
        for value in [&original.enable, &original.server, &original.bypass] {
            write_legacy_value(&mut bytes, value).unwrap();
        }

        let mut reader = SnapshotReader {
            bytes: &bytes,
            pos: 0,
        };
        assert_eq!(reader.dotnet_string().unwrap(), SNAPSHOT_HEADER);
        let enable = reader.legacy_value().unwrap();
        let server = reader.legacy_value().unwrap();
        let bypass = reader.legacy_value().unwrap();
        assert_eq!(reader.pos, bytes.len());
        assert_eq!(read_integer(&enable).unwrap(), 1);
        assert_eq!(decode_registry_text(&server).unwrap(), LOCAL_PROXY);
        assert!(!bypass.exists);
    }

    #[test]
    fn malformed_snapshot_values_are_rejected() {
        let mut reader = SnapshotReader {
            bytes: &[1, 99, 0, 0, 0],
            pos: 0,
        };
        assert!(reader.legacy_value().is_err());

        let mut reader = SnapshotReader {
            bytes: &[1, 4, 0, 0, 0, 1],
            pos: 0,
        };
        assert!(reader.legacy_value().is_err());
    }

    #[test]
    fn oversized_recovery_snapshot_is_rejected_before_unbounded_read() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "reality-proxy-backup-limit-{}-{nonce}.dat",
            std::process::id()
        ));
        fs::write(&path, vec![0; MAX_SNAPSHOT_SIZE + 1]).unwrap();
        let result = ProxySnapshot::load(&path);
        fs::remove_file(&path).unwrap();
        assert!(matches!(result, Err(problem) if problem.contains("превышает допустимый размер")));
    }

    #[test]
    fn recovery_recognizes_completed_restore_and_refuses_foreign_proxy() {
        let original = ProxySnapshot {
            enable: ValueSnapshot {
                exists: true,
                kind: 4,
                data: 0i32.to_le_bytes().to_vec(),
            },
            server: string_registry_value("proxy.example:3128"),
            bypass: string_registry_value("localhost;<local>"),
        };
        assert_eq!(
            original.recovery_decision(&original).unwrap(),
            RecoveryDecision::AlreadyRestored
        );

        let local_client = ProxySnapshot {
            enable: ValueSnapshot {
                exists: true,
                kind: 4,
                data: 1i32.to_le_bytes().to_vec(),
            },
            server: string_registry_value(LOCAL_PROXY),
            bypass: string_registry_value(BYPASS),
        };
        assert_eq!(
            original.recovery_decision(&local_client).unwrap(),
            RecoveryDecision::RestoreClientOwnedProxy
        );
        assert!(!original.matches(&local_client).unwrap());

        let foreign = ProxySnapshot {
            enable: local_client.enable.clone(),
            server: string_registry_value("127.0.0.1:7890"),
            bypass: local_client.bypass,
        };
        assert_eq!(
            original.recovery_decision(&foreign).unwrap(),
            RecoveryDecision::PreserveForeignProxy
        );
    }

    #[test]
    fn enabling_proxy_publishes_endpoint_before_turning_it_on() {
        let mut backend = memory_backend("proxy.example:3128", 0);
        enable_with_backend(&mut backend).unwrap();

        assert_eq!(
            backend.writes,
            [ProxyValue::Server, ProxyValue::Bypass, ProxyValue::Enable]
        );
        assert!(backend.current.is_enabled_at(LOCAL_PROXY).unwrap());
        assert_eq!(backend.notifications, 1);
    }

    #[test]
    fn recovery_restores_proxy_endpoint_last() {
        let original = memory_backend("proxy.example:3128", 0).current;
        let backup = ProxySnapshot {
            enable: original.enable.clone(),
            server: original.server.clone(),
            bypass: original.bypass.clone(),
        };
        let mut backend = memory_backend(LOCAL_PROXY, 1);

        assert!(backup.restore_with_backend(&mut backend).unwrap());
        assert_eq!(
            backend.writes,
            [ProxyValue::Bypass, ProxyValue::Enable, ProxyValue::Server]
        );
        assert!(backup.matches(&backend.current).unwrap());
        assert_eq!(backend.notifications, 1);
    }

    #[test]
    fn interrupted_recovery_keeps_local_endpoint_for_a_safe_retry() {
        let original = memory_backend("proxy.example:3128", 0).current;
        let backup = ProxySnapshot {
            enable: original.enable,
            server: original.server,
            bypass: original.bypass,
        };
        let mut backend = memory_backend(LOCAL_PROXY, 1);
        backend.fail_on = Some(ProxyValue::Server);

        assert!(backup.restore_with_backend(&mut backend).is_err());
        assert_eq!(
            backend.writes,
            [ProxyValue::Bypass, ProxyValue::Enable, ProxyValue::Server]
        );
        assert!(backend.current.server_is_local().unwrap());
        assert_eq!(
            backup.recovery_decision(&backend.current).unwrap(),
            RecoveryDecision::RestoreClientOwnedProxy
        );
        assert_eq!(backend.notifications, 0);
    }

    #[test]
    fn recovery_never_writes_when_a_foreign_proxy_owns_the_settings() {
        let original = memory_backend("proxy.example:3128", 0).current;
        let backup = ProxySnapshot {
            enable: original.enable,
            server: original.server,
            bypass: original.bypass,
        };
        let mut backend = memory_backend("127.0.0.1:7890", 1);

        assert!(!backup.restore_with_backend(&mut backend).unwrap());
        assert!(backend.writes.is_empty());
        assert_eq!(backend.notifications, 0);
    }

    #[test]
    fn recovery_preserves_user_changes_to_proxy_enable_or_bypass() {
        let original_enabled = memory_backend("proxy.example:3128", 1).current;
        let backup = ProxySnapshot {
            enable: original_enabled.enable.clone(),
            server: original_enabled.server.clone(),
            bypass: original_enabled.bypass.clone(),
        };

        let mut changed_bypass = memory_backend(LOCAL_PROXY, 1);
        changed_bypass.current.bypass = string_registry_value("localhost;*.example.org");
        assert!(!backup.restore_with_backend(&mut changed_bypass).unwrap());
        assert!(changed_bypass.writes.is_empty());
        assert_eq!(changed_bypass.notifications, 0);

        let mut disabled_by_user = memory_backend(LOCAL_PROXY, 0);
        assert!(!backup.restore_with_backend(&mut disabled_by_user).unwrap());
        assert!(disabled_by_user.writes.is_empty());
        assert_eq!(disabled_by_user.notifications, 0);
    }

    #[test]
    fn backup_stays_when_a_foreign_proxy_owns_the_settings() {
        let original = memory_backend("proxy.example:3128", 0).current;
        let backup = ProxySnapshot {
            enable: original.enable,
            server: original.server,
            bypass: original.bypass,
        };
        let path = temporary_backup_path();
        backup.save(&path).unwrap();
        let mut backend = memory_backend("127.0.0.1:7890", 1);

        assert!(!recover_with_backend(&path, &mut backend).unwrap());
        assert!(path.exists());
        assert!(backend.writes.is_empty());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn backup_stays_after_notification_failure_and_retry_removes_it() {
        let original = memory_backend("proxy.example:3128", 0).current;
        let backup = ProxySnapshot {
            enable: original.enable,
            server: original.server,
            bypass: original.bypass,
        };
        let path = temporary_backup_path();
        backup.save(&path).unwrap();
        let mut backend = memory_backend(LOCAL_PROXY, 1);
        backend.fail_notification = true;

        assert!(recover_with_backend(&path, &mut backend).is_err());
        assert!(path.exists());
        assert!(backup.matches(&backend.current).unwrap());

        backend.fail_notification = false;
        assert!(recover_with_backend(&path, &mut backend).unwrap());
        assert!(!path.exists());
        assert_eq!(backend.notifications, 2);
    }
}
