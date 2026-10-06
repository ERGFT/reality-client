use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const MAX_CONFIG_DIAGNOSTIC_BYTES: usize = 16 * 1024;

pub fn default_config_path() -> Result<PathBuf, String> {
    let root = crate::platform::app_data_dir()?;
    crate::platform::ensure_private_dir(&root)?;
    Ok(root.join("advanced-config.json"))
}

pub fn load_advanced_config_path() -> Result<Option<PathBuf>, String> {
    load_advanced_config_path_from(&crate::platform::app_data_dir()?)
}

fn load_advanced_config_path_from(root: &Path) -> Result<Option<PathBuf>, String> {
    let path = root.join("settings.txt");
    match std::fs::read_to_string(path) {
        Ok(value) => {
            let value = value.trim();
            Ok((!value.is_empty()).then(|| PathBuf::from(value)))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("Не удалось прочитать настройки клиента: {error}")),
    }
}

pub fn save_advanced_config_path(path: &Path) -> Result<(), String> {
    let root = crate::platform::app_data_dir()?;
    crate::platform::ensure_private_dir(&root)?;
    save_advanced_config_path_to(&root, path)
}

fn save_advanced_config_path_to(root: &Path, path: &Path) -> Result<(), String> {
    std::fs::write(root.join("settings.txt"), path.to_string_lossy().as_bytes())
        .map_err(|error| format!("Не удалось сохранить путь JSON-конфигурации: {error}"))
}

pub fn has_proxy_recovery() -> bool {
    #[cfg(windows)]
    {
        crate::windows_proxy::snapshot_path().is_ok_and(|path| path.exists())
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub fn recover_proxy() -> Result<bool, String> {
    #[cfg(windows)]
    {
        let path = crate::windows_proxy::snapshot_path()?;
        if !path.exists() {
            return Ok(false);
        }
        crate::windows_proxy::recover_system_proxy(&path)
    }
    #[cfg(not(windows))]
    {
        Err("Восстановление системного прокси пока реализовано только для Windows.".into())
    }
}

pub fn discard_proxy_recovery() -> Result<bool, String> {
    #[cfg(windows)]
    {
        let path = crate::windows_proxy::snapshot_path()?;
        if !path.exists() {
            return Ok(false);
        }
        if crate::windows_proxy::active_is_local_proxy()? {
            return Err(
                "Системный прокси всё ещё указывает на Reality Client. Сначала восстановите настройки.".into(),
            );
        }
        std::fs::remove_file(&path)
            .map_err(|e| format!("Не удалось удалить резервную копию прокси: {e}"))?;
        Ok(true)
    }
    #[cfg(not(windows))]
    {
        Err("Резервная копия системного прокси доступна только в Windows.".into())
    }
}

pub fn check_config_file(path: &std::path::Path) -> Result<(), String> {
    if !path.is_file() {
        return Err("Файл конфигурации не найден. Сначала сохраните JSON.".into());
    }
    let core = find_core()?;
    let path = path
        .canonicalize()
        .map_err(|e| format!("Не удалось открыть конфигурацию: {e}"))?;
    let base = path
        .parent()
        .ok_or("Не удалось определить папку конфигурации.")?;
    let mut process = Command::new(core)
        .args([
            "--config",
            path.to_str()
                .ok_or("Путь конфигурации не является корректным Unicode.")?,
            "--check",
        ])
        .current_dir(base)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Не удалось запустить проверку ядра: {e}"))?;
    let stdout = process
        .stdout
        .take()
        .ok_or("Не удалось прочитать вывод проверки конфигурации.")?;
    let stderr = process
        .stderr
        .take()
        .ok_or("Не удалось прочитать ошибки проверки конфигурации.")?;
    let stdout_reader = thread::spawn(move || read_capped_output(stdout));
    let stderr_reader = thread::spawn(move || read_capped_output(stderr));
    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        match process.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                let _ = process.kill();
                let _ = process.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(format!("Не удалось получить результат проверки: {error}"));
            }
        }
        if Instant::now() >= deadline {
            let _ = process.kill();
            let _ = process.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(
                "Проверка конфигурации превысила 20 секунд; процесс проверки остановлен.".into(),
            );
        }
        thread::sleep(Duration::from_millis(50));
    };
    let stdout = stdout_reader
        .join()
        .map_err(|_| "Поток чтения вывода проверки конфигурации завершился аварийно.".to_owned())?
        .map_err(|error| format!("Не удалось прочитать вывод проверки конфигурации: {error}"))?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| "Поток чтения ошибок проверки конфигурации завершился аварийно.".to_owned())?
        .map_err(|error| format!("Не удалось прочитать ошибки проверки конфигурации: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(config_check_failure_message(&stdout, &stderr))
    }
}

fn read_capped_output(mut reader: impl Read) -> std::io::Result<Vec<u8>> {
    let mut captured = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok(captured);
        }
        let remaining = MAX_CONFIG_DIAGNOSTIC_BYTES.saturating_sub(captured.len());
        captured.extend_from_slice(&buffer[..read.min(remaining)]);
    }
}

fn config_check_failure_message(stdout: &[u8], stderr: &[u8]) -> String {
    const SUMMARY: &str =
        "Ядро отклонило JSON-конфигурацию. Проверьте обязательные поля и пути связанных файлов.";
    let output = [stdout, stderr]
        .into_iter()
        .filter(|stream| !stream.is_empty())
        .map(String::from_utf8_lossy)
        .collect::<Vec<_>>()
        .join("\n");
    if output.trim().is_empty() {
        return SUMMARY.to_owned();
    }
    let safe = crate::security::redact_sensitive_text(output.trim());
    const MAX_DISPLAY_CHARS: usize = 4000;
    let mut diagnostic = safe.chars().take(MAX_DISPLAY_CHARS).collect::<String>();
    if safe.chars().count() > MAX_DISPLAY_CHARS {
        diagnostic.push_str("… (вывод обрезан)");
    }
    format!("{SUMMARY}\n\n{diagnostic}")
}

fn find_core() -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    #[cfg(windows)]
    let file_name = "reality-client.exe";
    #[cfg(not(windows))]
    let file_name = "reality-client";
    if let Ok(root) = std::env::var("LOCALAPPDATA") {
        candidates.push(PathBuf::from(root).join("RealityClient").join(file_name));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(root) = exe.parent()
    {
        candidates.push(root.join(file_name));
        candidates.push(root.join("third_party").join(file_name));
        candidates.push(root.join("../third_party").join(file_name));
    }
    // Тесты берут ядро, собранное из закреплённой версии (build_rust_core.ps1,
    // build-linux.sh) в rust-client/third_party.
    #[cfg(test)]
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("third_party")
            .join(file_name),
    );
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            format!(
                "Не найден исполняемый файл ядра ({file_name}). Положите его рядом с приложением."
            )
        })
}

#[cfg(windows)]
pub fn cleanup_tun_routes() -> Result<String, String> {
    let executable = find_core()?;
    let output = Command::new(&executable)
        .arg("--tun-cleanup")
        .current_dir(executable.parent().unwrap_or_else(|| Path::new(".")))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|problem| format!("Не удалось запустить восстановление Windows TUN: {problem}"))?;
    let diagnostic = [output.stdout.as_slice(), output.stderr.as_slice()]
        .into_iter()
        .filter(|stream| !stream.is_empty())
        .map(String::from_utf8_lossy)
        .collect::<Vec<_>>()
        .join("\n");
    let safe = crate::security::redact_sensitive_text(diagnostic.trim());
    if output.status.success() {
        Ok(if safe.is_empty() {
            "Ядро выполнило очистку собственных остатков TUN.".to_owned()
        } else {
            safe
        })
    } else if safe.is_empty() {
        Err("Очистка TUN завершилась ошибкой. Запустите Reality Client от имени администратора и повторите.".into())
    } else {
        Err(safe)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn config_check_diagnostics_are_redacted_and_bounded_for_display() {
        let message = config_check_failure_message(
            b"invalid vless://uuid-secret@example.org:443?pbk=key-secret",
            b"missing field token=token-secret",
        );
        assert!(message.contains("missing field"));
        assert!(message.contains("vless://[скрыто]"));
        for secret in ["uuid-secret", "key-secret", "token-secret"] {
            assert!(!message.contains(secret), "diagnostic leaked {secret}");
        }
        let oversized =
            config_check_failure_message(&vec![b'x'; MAX_CONFIG_DIAGNOSTIC_BYTES + 1000], b"");
        assert!(oversized.ends_with("… (вывод обрезан)"));
        assert!(oversized.chars().count() < 5000);
    }

    #[test]
    fn config_check_output_is_capped_while_remaining_bytes_are_drained() {
        let input = vec![b'x'; MAX_CONFIG_DIAGNOSTIC_BYTES + 8192];
        let output = read_capped_output(std::io::Cursor::new(input)).unwrap();
        assert_eq!(output.len(), MAX_CONFIG_DIAGNOSTIC_BYTES);
    }

    #[test]
    fn advanced_config_path_settings_roundtrip_and_missing_file() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("reality-settings-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        assert_eq!(load_advanced_config_path_from(&root).unwrap(), None);
        let selected = root.join("configs with spaces").join("client.json");
        save_advanced_config_path_to(&root, &selected).unwrap();
        assert_eq!(
            load_advanced_config_path_from(&root).unwrap(),
            Some(selected)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn bundled_core_accepts_a_direct_json_config() {
        let core =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../third_party/reality-client.exe");
        assert!(
            core.is_file(),
            "expected pinned config checker at {}",
            core.display()
        );
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "reality-config-check-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("config.json");
        fs::write(&config_path, r#"{"inbounds":[{"type":"mixed","tag":"local","listen":"127.0.0.1","listen_port":1080}],"outbounds":[{"type":"direct","tag":"direct"}],"route":{"final":"direct"}}"#).unwrap();
        let result = check_config_file(&config_path);
        let _ = fs::remove_dir_all(&dir);
        result.unwrap();
    }
}
