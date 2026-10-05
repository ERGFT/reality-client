use std::{
    collections::VecDeque,
    ffi::{CStr, c_void},
    fs::{self, OpenOptions},
    io::Write,
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use zeroize::Zeroizing;

#[cfg(windows)]
use crate::core::recover_proxy;
use crate::{
    core::{check_config_file, default_config_path, has_proxy_recovery},
    ffi_core::FfiCore,
    security::redact_sensitive_text,
};

pub struct CoreSession {
    core: Option<FfiCore>,
    _log_queue: Arc<Mutex<VecDeque<String>>>,
    secret_file: Option<PathBuf>,
    #[cfg_attr(target_os = "android", allow(dead_code))]
    proxy_backup: Option<PathBuf>,
    verify_socks: bool,
    reload_supported: bool,
    config_path: Option<PathBuf>,
}

impl CoreSession {
    pub fn reload_supported(&self) -> bool {
        self.reload_supported && self.core.is_some()
    }

    pub fn reload_config(&self, config_path: &Path, config: &str) -> Result<String, String> {
        if !self.reload_supported() {
            return Err(
                "Горячее обновление доступно только для запущенного desktop-конфига без TUN."
                    .into(),
            );
        }
        validate_reload_config(config)?;
        let config_path = config_path
            .canonicalize()
            .map_err(|e| format!("Не удалось разрешить путь JSON-конфигурации: {e}"))?;
        if self.config_path.as_deref() != Some(config_path.as_path()) {
            return Err(
                "Для горячего обновления выберите тот JSON-файл, с которым было запущено ядро."
                    .into(),
            );
        }
        self.core
            .as_ref()
            .ok_or("Ядро уже остановлено.")?
            .reload(config)
            .map(|result| redact_sensitive_text(&result))
            .map_err(|problem| redact_sensitive_text(&problem))
    }

    pub fn runtime_api(&self, path: &str) -> Result<serde_json::Value, String> {
        let core = self.core.as_ref().ok_or("Ядро уже остановлено.")?;
        let (status, body) = core.request("GET", path, None)?;
        if !(200..300).contains(&status) {
            return Err(format!(
                "Локальный API ядра вернул HTTP {status} для {path}."
            ));
        }
        serde_json::from_str(&body)
            .map_err(|_| format!("Локальный API ядра вернул некорректный JSON для {path}."))
    }

    pub fn select_group_member(&self, group: &str, member: &str) -> Result<(), String> {
        if group.is_empty()
            || member.is_empty()
            || group
                .chars()
                .any(|c| c.is_control() || matches!(c, '/' | '?' | '#' | '%'))
            || member.chars().any(|c| c.is_control())
        {
            return Err("Имя группы или сервера содержит неподдерживаемые символы.".into());
        }
        let core = self.core.as_ref().ok_or("Ядро уже остановлено.")?;
        let path = format!("/groups/{group}");
        let body = serde_json::json!({ "member": member }).to_string();
        let (status, response) = core.request("PUT", &path, Some(&body))?;
        if !(200..300).contains(&status) {
            let message = serde_json::from_str::<serde_json::Value>(&response)
                .ok()
                .and_then(|value| {
                    value
                        .get("message")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                })
                .unwrap_or_else(|| format!("Локальный API ядра вернул HTTP {status}."));
            return Err(redact_sensitive_text(&message));
        }
        Ok(())
    }

    pub fn start(
        vless_link: &str,
        system_proxy: bool,
        logs: &Arc<Mutex<VecDeque<String>>>,
    ) -> Result<Self, String> {
        refuse_recovery()?;
        let data_dir = default_config_path()?
            .parent()
            .ok_or("Не удалось определить папку данных клиента.")?
            .to_owned();
        fs::create_dir_all(&data_dir)
            .map_err(|e| format!("Не удалось создать папку клиента: {e}"))?;
        let link_path = data_dir.join("server.txt");
        let config_path = data_dir.join("client.json");
        write_secret(&link_path, vless_link)?;
        let mut cleanup = SecretCleanup {
            path: link_path.clone(),
            retained: false,
        };
        let config = profile_config();
        fs::write(&config_path, config.as_bytes())
            .map_err(|e| format!("Не удалось записать конфигурацию профиля: {e}"))?;
        check_config_file(&config_path)?;

        let library = FfiCore::load()?;
        let mut core = FfiCore::start(library, &config, &data_dir, -1)?;
        install_log_callback(&mut core, logs)?;
        let backup = if system_proxy {
            #[cfg(windows)]
            {
                Some(crate::windows_proxy::prepare_system_proxy_backup()?)
            }
            #[cfg(not(windows))]
            {
                core.stop();
                return Err("Системный прокси пока реализован только для Windows.".into());
            }
        } else {
            None
        };
        if system_proxy {
            #[cfg(windows)]
            if let Err(problem) = crate::windows_proxy::enable_system_proxy() {
                core.stop();
                let _ = recover_proxy();
                return Err(format!(
                    "Ядро запущено, но Windows не включила системный прокси: {problem}"
                ));
            }
        }

        let mut session = Self {
            core: Some(core),
            _log_queue: logs.clone(),
            secret_file: Some(link_path.clone()),
            proxy_backup: backup,
            verify_socks: true,
            reload_supported: false,
            config_path: None,
        };
        if let Err(problem) = session.wait_for_socks(Duration::from_secs(15)) {
            let _ = session.stop();
            return Err(problem);
        }
        cleanup.retained = true;
        Ok(session)
    }

    pub fn start_config(
        config_path: &Path,
        logs: &Arc<Mutex<VecDeque<String>>>,
    ) -> Result<Self, String> {
        refuse_recovery()?;
        let config_path = config_path
            .canonicalize()
            .map_err(|e| format!("Не удалось открыть JSON-конфигурацию: {e}"))?;
        let base_dir = config_path
            .parent()
            .ok_or("Не удалось определить папку конфигурации.")?;
        let config = read_checked_config(&config_path, check_config_file)?;
        crate::core::save_advanced_config_path(&config_path)?;
        refuse_unmanaged_platform_tun(&config)?;
        let reload_supported = !config_has_tun(&config)?;
        let library = FfiCore::load()?;
        #[cfg(windows)]
        if !reload_supported {
            require_bundled_wintun()?;
            let lock_dir = crate::platform::prepare_windows_tun_lock_dir()?;
            FfiCore::set_lock_dir(&library, &lock_dir)?;
        }
        let mut core = FfiCore::start(library, &config, base_dir, -1)?;
        install_log_callback(&mut core, logs)?;
        Ok(Self {
            core: Some(core),
            _log_queue: logs.clone(),
            secret_file: None,
            proxy_backup: None,
            verify_socks: false,
            reload_supported,
            config_path: Some(config_path),
        })
    }

    pub fn stop(&mut self) -> Result<(), String> {
        if let Some(mut core) = self.core.take() {
            core.stop();
        }
        if let Some(path) = self.secret_file.take() {
            erase_file(&path);
        }
        #[cfg(windows)]
        if let Some(path) = self.proxy_backup.take() {
            crate::windows_proxy::recover_system_proxy(&path)?;
        }
        Ok(())
    }

    fn wait_for_socks(&mut self, timeout: Duration) -> Result<(), String> {
        if !self.verify_socks {
            return Ok(());
        }
        let address: SocketAddr = "127.0.0.1:1080".parse().unwrap();
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.core.is_none() {
                return Err("Ядро остановлено во время запуска.".into());
            }
            if socks5_greeting(address) {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(120));
        }
        Err("Ядро не ответило на локальный SOCKS5 handshake за 15 секунд.".into())
    }
}

impl Drop for CoreSession {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn read_checked_config(
    path: &Path,
    check: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<String, String> {
    let config = fs::read_to_string(path)
        .map_err(|e| format!("Не удалось прочитать JSON-конфигурацию: {e}"))?;
    check(path)?;
    let checked_config = fs::read_to_string(path)
        .map_err(|e| format!("Не удалось повторно прочитать проверенный JSON-конфиг: {e}"))?;
    if config != checked_config {
        return Err("JSON-конфигурация изменилась во время проверки. Проверьте её ещё раз.".into());
    }
    Ok(config)
}

#[cfg(test)]
mod checked_config_tests {
    use super::read_checked_config;
    use std::{fs, path::Path};

    fn temp_config_path() -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "reality-config-race-{}-{nonce}.json",
            std::process::id()
        ))
    }

    #[test]
    fn refuses_config_modified_during_validation() {
        let path = temp_config_path();
        fs::write(&path, r#"{"route":{"final":"direct"}}"#).unwrap();
        let result = read_checked_config(&path, |path: &Path| {
            fs::write(path, r#"{"route":{"final":"proxy"}}"#).map_err(|problem| problem.to_string())
        });
        let _ = fs::remove_file(&path);
        assert!(matches!(result, Err(problem) if problem.contains("изменилась")));
    }

    #[test]
    fn returns_the_exact_config_that_passed_validation() {
        let path = temp_config_path();
        let expected = r#"{"route":{"final":"direct"}}"#;
        fs::write(&path, expected).unwrap();
        let result = read_checked_config(&path, |_| Ok(()));
        let _ = fs::remove_file(path);
        assert_eq!(result.unwrap(), expected);
    }
}

#[cfg(test)]
mod session_stop_tests {
    use super::CoreSession;
    use std::{
        collections::VecDeque,
        fs,
        path::Path,
        sync::{Arc, Mutex},
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn repeated_stop_does_not_touch_a_file_created_after_secret_cleanup() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let secret_path = std::env::temp_dir().join(format!(
            "reality-client-stop-secret-{}-{nonce}",
            std::process::id()
        ));
        fs::write(&secret_path, b"temporary secret").unwrap();
        let logs = Arc::new(Mutex::new(VecDeque::new()));
        let mut session = CoreSession {
            core: None,
            _log_queue: logs,
            secret_file: Some(secret_path.clone()),
            proxy_backup: None,
            verify_socks: false,
            reload_supported: false,
            config_path: None,
        };

        assert!(session.stop().is_ok());
        assert!(!secret_path.exists());

        fs::write(&secret_path, b"unrelated replacement").unwrap();
        assert!(session.stop().is_ok());
        assert_eq!(fs::read(&secret_path).unwrap(), b"unrelated replacement");
        fs::remove_file(secret_path).unwrap();
    }

    #[test]
    fn reload_is_refused_when_session_does_not_support_it() {
        let session = CoreSession {
            core: None,
            _log_queue: Arc::new(Mutex::new(VecDeque::new())),
            secret_file: None,
            proxy_backup: None,
            verify_socks: false,
            reload_supported: false,
            config_path: None,
        };
        let problem = session
            .reload_config(Path::new("unused.json"), "{}")
            .unwrap_err();
        assert!(problem.contains("без TUN"));
    }
}

fn install_log_callback(
    core: &mut FfiCore,
    logs: &Arc<Mutex<VecDeque<String>>>,
) -> Result<(), String> {
    core.set_log_callback(
        "warning",
        Some(receive_core_log),
        Arc::as_ptr(logs) as *mut c_void,
    )
}

unsafe extern "C" fn receive_core_log(json: *const std::ffi::c_char, user: *mut c_void) {
    if json.is_null() || user.is_null() {
        return;
    }
    // SAFETY: the FFI passes a valid NUL-terminated JSON string for this call;
    // the queue remains owned by the application until after rc_stop returns.
    let Ok(value) = serde_json::from_str::<serde_json::Value>(
        unsafe { CStr::from_ptr(json) }.to_string_lossy().as_ref(),
    ) else {
        return;
    };
    let level = value
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("warning");
    let payload = value
        .get("payload")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    if payload.is_empty() {
        return;
    }
    let safe_payload = redact_sensitive_text(payload);
    let line = format!("[{level}] {safe_payload}");
    // SAFETY: user is the stable pointer returned by Arc::as_ptr for this queue.
    let queue = unsafe { &*(user as *const Mutex<VecDeque<String>>) };
    if let Ok(mut queue) = queue.try_lock() {
        if queue.len() >= 200 {
            queue.pop_front();
        }
        queue.push_back(line);
    }
}

#[cfg(all(test, windows))]
mod ffi_tests {
    use super::*;
    use crate::ffi_core::FfiCore;
    use std::net::{Ipv4Addr, TcpListener};

    #[test]
    fn pinned_ffi_starts_reloads_and_stops_a_loopback_socks_listener() {
        let library_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("third_party/reality.dll");
        assert!(
            library_path.is_file(),
            "expected built FFI DLL at {}",
            library_path.display()
        );
        let probe = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);

        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base_dir =
            std::env::temp_dir().join(format!("reality-ffi-smoke-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&base_dir).unwrap();
        let config = format!(
            r#"{{"inbounds":[{{"type":"mixed","tag":"smoke","listen":"127.0.0.1","listen_port":{port}}}],"outbounds":[{{"type":"selector","tag":"choose","outbounds":["direct","block"]}},{{"type":"direct","tag":"direct"}},{{"type":"block","tag":"block"}}],"route":{{"final":"choose"}}}}"#
        );

        let result = (|| {
            let library = FfiCore::load_from(&library_path)?;
            let mut core = FfiCore::start(library, &config, &base_dir, -1)?;
            let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                if socks5_greeting(address) {
                    let (status, groups) = core.request("GET", "/groups", None)?;
                    assert_eq!(status, 200, "embedded group API should respond locally");
                    let groups: serde_json::Value = serde_json::from_str(&groups).unwrap();
                    assert_eq!(groups["groups"][0]["tag"], "choose");
                    assert_eq!(groups["groups"][0]["current"], "direct");
                    let (status, _) =
                        core.request("PUT", "/groups/choose", Some(r#"{"member":"block"}"#))?;
                    assert_eq!(status, 200, "embedded group selection should succeed");
                    let (status, stats) = core.request("GET", "/stats", None)?;
                    assert_eq!(status, 200, "embedded traffic stats should respond locally");
                    let stats: serde_json::Value = serde_json::from_str(&stats).unwrap();
                    assert!(
                        stats
                            .get("up")
                            .and_then(serde_json::Value::as_u64)
                            .is_some()
                    );
                    assert!(
                        stats
                            .get("down")
                            .and_then(serde_json::Value::as_u64)
                            .is_some()
                    );
                    let mut replacement: serde_json::Value = serde_json::from_str(&config).unwrap();
                    replacement["outbounds"][0]["tag"] = "renamed".into();
                    replacement["route"]["final"] = "renamed".into();
                    let reload = core.reload(&replacement.to_string()).unwrap();
                    let reload: serde_json::Value = serde_json::from_str(&reload).unwrap();
                    assert!(reload["notes"].as_array().is_some());
                    let (status, groups) = core.request("GET", "/groups", None).unwrap();
                    assert_eq!(status, 200);
                    let groups: serde_json::Value = serde_json::from_str(&groups).unwrap();
                    assert_eq!(groups["groups"][0]["tag"], "renamed");
                    assert!(
                        socks5_greeting(address),
                        "SOCKS listener should remain available after rc_reload"
                    );
                    core.stop();
                    return Ok(());
                }
                thread::sleep(Duration::from_millis(100));
            }
            core.stop();
            Err("FFI-ядро не ответило на локальный SOCKS5 handshake.".to_owned())
        })();
        let _ = fs::remove_dir_all(&base_dir);
        result.unwrap();
    }
}

#[cfg(test)]
mod log_tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn core_log_callback_redacts_vless_links_and_credentials() {
        let logs = Arc::new(Mutex::new(VecDeque::<String>::new()));
        let user = Arc::as_ptr(&logs) as *mut c_void;
        let input = CString::new(r#"{"type":"warning","payload":"bad vless://uuid-secret@example.org:443?pbk=key-secret&sid=id-secret"}"#).unwrap();
        unsafe {
            receive_core_log(input.as_ptr(), user);
        }
        let line = logs.lock().unwrap().pop_front().unwrap();
        assert!(line.contains("[warning]"));
        assert!(!line.contains("uuid-secret"));
        assert!(!line.contains("key-secret"));
        assert!(!line.contains("id-secret"));
        assert!(line.contains("vless://[скрыто]"));
    }
}

struct SecretCleanup {
    path: PathBuf,
    retained: bool,
}

impl Drop for SecretCleanup {
    fn drop(&mut self) {
        if !self.retained {
            erase_file(&self.path);
        }
    }
}

fn refuse_recovery() -> Result<(), String> {
    if has_proxy_recovery() {
        Err("Найдена незавершённая сессия системного прокси. Сначала восстановите прежние настройки.".into())
    } else {
        Ok(())
    }
}

fn profile_config() -> String {
    r#"{
  "inbounds": [{ "type": "mixed", "tag": "local", "listen": "127.0.0.1", "listen_port": 1080 }],
  "outbounds": [
    { "type": "vless", "tag": "proxy", "link_file": "server.txt" },
    { "type": "direct", "tag": "direct" },
    { "type": "block", "tag": "block" }
  ],
  "route": { "rules": [{ "action": "sniff" }], "final": "proxy" }
}"#
    .to_owned()
}

#[cfg(any(target_os = "android", feature = "android-bridge-check"))]
pub fn prepare_android_profile(vless_link: &str) -> Result<String, String> {
    let data_dir = crate::platform::app_data_dir()?;
    crate::platform::ensure_private_dir(&data_dir)?;
    let secret_path = data_dir.join("android-server.txt");
    write_secret(&secret_path, vless_link)?;
    let config = r#"{
  "inbounds": [{ "type": "tun", "tag": "tun", "address": ["172.19.0.1/30", "fdfe:dcba:9876::1/126"], "mtu": 1500 }],
  "outbounds": [
    { "type": "vless", "tag": "proxy", "link_file": "android-server.txt" },
    { "type": "direct", "tag": "direct" },
    { "type": "block", "tag": "block" }
  ],
  "route": { "rules": [{ "action": "sniff" }, { "protocol": "dns", "action": "hijack-dns" }], "final": "proxy" },
  "dns": { "servers": [{ "type": "https", "tag": "remote", "server": "1.1.1.1", "detour": "proxy" }], "final": "remote" }
}"#;
    Ok(config.to_owned())
}

#[cfg(any(target_os = "android", feature = "android-bridge-check"))]
pub fn read_android_full_config(path: &Path) -> Result<String, String> {
    let path = path
        .canonicalize()
        .map_err(|e| format!("Не удалось открыть JSON-конфигурацию: {e}"))?;
    let config = fs::read_to_string(&path)
        .map_err(|e| format!("Не удалось прочитать JSON-конфигурацию: {e}"))?;
    let has_tun = config_has_tun(&config)?;
    if !has_tun {
        return Err("Для Android VPN в полном JSON-конфиге нужен входящий тип tun.".into());
    }
    crate::core::save_advanced_config_path(&path)?;
    Ok(config)
}

#[cfg(any(target_os = "android", feature = "android-bridge-check"))]
pub fn erase_android_profile_secret() {
    if let Ok(data_dir) = crate::platform::app_data_dir() {
        erase_file(&data_dir.join("android-server.txt"));
    }
}

fn write_secret(path: &Path, link: &str) -> Result<(), String> {
    let content = Zeroizing::new(format!("{link}\n").into_bytes());
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .share_mode(0)
            .open(path)
            .map_err(|e| format!("Не удалось сохранить временную ссылку: {e}"))?;
        file.write_all(&content)
            .map_err(|e| format!("Не удалось сохранить временную ссылку: {e}"))?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| format!("Не удалось сохранить временную ссылку: {e}"))?;
        file.write_all(&content)
            .map_err(|e| format!("Не удалось сохранить временную ссылку: {e}"))?;
    }
    Ok(())
}

fn erase_file(path: &Path) {
    if let Ok(metadata) = fs::metadata(path)
        && let Ok(mut file) = OpenOptions::new().write(true).open(path)
    {
        let zeros = [0u8; 4096];
        let mut remaining = metadata.len();
        while remaining > 0 {
            let count = remaining.min(zeros.len() as u64) as usize;
            if file.write_all(&zeros[..count]).is_err() {
                break;
            }
            remaining -= count as u64;
        }
        let _ = file.sync_all();
    }
    let _ = fs::remove_file(path);
}

fn socks5_greeting(address: SocketAddr) -> bool {
    use std::io::{Read, Write};
    let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_millis(150)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(250)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(250)));
    if stream.write_all(&[5, 1, 0]).is_err() {
        return false;
    }
    let mut reply = [0u8; 2];
    stream.read_exact(&mut reply).is_ok() && reply == [5, 0]
}

fn config_has_tun(config: &str) -> Result<bool, String> {
    let value: serde_json::Value =
        serde_json::from_str(config).map_err(|e| format!("Некорректный JSON: {e}"))?;
    Ok(value
        .get("inbounds")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|inbound| {
                ["type", "protocol"]
                    .iter()
                    .any(|key| inbound.get(*key).and_then(serde_json::Value::as_str) == Some("tun"))
            })
        }))
}

fn validate_reload_config(config: &str) -> Result<(), String> {
    if config_has_tun(config)? {
        return Err(
            "Горячее обновление конфигураций с TUN отключено: маршруты требуют отдельной проверки."
                .into(),
        );
    }
    Ok(())
}

fn validate_tun_policy(
    config: &str,
    core_manages_routes: bool,
    unsupported_message: &str,
) -> Result<(), String> {
    if config_has_tun(config)? && !core_manages_routes {
        return Err(unsupported_message.to_owned());
    }
    Ok(())
}

#[cfg(windows)]
fn refuse_unmanaged_platform_tun(config: &str) -> Result<(), String> {
    validate_tun_policy(config, true, "")
}

#[cfg(windows)]
fn require_bundled_wintun() -> Result<PathBuf, String> {
    let executable = std::env::current_exe()
        .map_err(|problem| format!("Не удалось определить путь клиента Windows: {problem}"))?;
    let dll = executable
        .parent()
        .ok_or("Не удалось определить папку Windows-клиента.")?
        .join("wintun.dll");
    if !dll.is_file() {
        return Err(format!(
            "Для Windows TUN нужен официальный wintun.dll рядом с приложением: {}. Переустановите полный пакет клиента.",
            dll.display()
        ));
    }
    Ok(dll)
}

#[cfg(target_os = "linux")]
fn refuse_unmanaged_platform_tun(config: &str) -> Result<(), String> {
    // The pinned core creates Linux TUN devices and owns marked auto_route
    // rules through a guard that restores them when the FFI session stops.
    validate_tun_policy(config, true, "")
}

#[cfg(target_os = "android")]
fn refuse_unmanaged_platform_tun(config: &str) -> Result<(), String> {
    validate_tun_policy(
        config,
        false,
        "Android VpnService ещё не подключён; TUN-конфигурация не запущена.",
    )
}

#[cfg(not(any(windows, target_os = "linux", target_os = "android")))]
fn refuse_unmanaged_platform_tun(config: &str) -> Result<(), String> {
    validate_tun_policy(
        config,
        false,
        "TUN на этой платформе пока не поддерживается клиентом.",
    )
}

#[cfg(test)]
mod platform_config_tests {
    use super::*;

    #[test]
    fn detects_tun_in_full_config_without_starting_it() {
        let tun_config = r#"{"inbounds":[{"type":"tun"}]}"#;
        let xray_tun_config = r#"{"inbounds":[{"protocol":"tun"}]}"#;
        let proxy_config = r#"{"inbounds":[{"type":"mixed"}]}"#;
        assert!(config_has_tun(tun_config).unwrap());
        assert!(config_has_tun(xray_tun_config).unwrap());
        assert!(!config_has_tun(proxy_config).unwrap());
        #[cfg(target_os = "linux")]
        assert!(refuse_unmanaged_platform_tun(tun_config).is_ok());
        #[cfg(not(any(target_os = "linux", windows)))]
        assert!(refuse_unmanaged_platform_tun(tun_config).is_err());
        #[cfg(any(target_os = "linux", windows))]
        assert!(refuse_unmanaged_platform_tun(tun_config).is_ok());
        assert!(refuse_unmanaged_platform_tun(proxy_config).is_ok());
        #[cfg(not(any(target_os = "linux", windows)))]
        assert!(refuse_unmanaged_platform_tun(xray_tun_config).is_err());
        #[cfg(any(target_os = "linux", windows))]
        assert!(refuse_unmanaged_platform_tun(xray_tun_config).is_ok());
        assert!(validate_tun_policy(tun_config, true, "unsupported").is_ok());
        assert!(validate_tun_policy(tun_config, false, "unsupported").is_err());
        assert!(validate_reload_config(proxy_config).is_ok());
        assert!(validate_reload_config(tun_config).is_err());
        assert!(validate_reload_config(xray_tun_config).is_err());
        assert!(validate_tun_policy("{", true, "unsupported").is_err());
    }
}
