use std::{
    ffi::{CStr, CString, c_char, c_void},
    path::Path,
    ptr::NonNull,
};

use crate::security::redact_sensitive_text;
// Библиотека пакета reality-ffi называется `reality` (libreality.so, reality.dll).
use reality as reality_ffi;
use reality_ffi::RcCore;

#[cfg(any(target_os = "android", feature = "android-bridge-check"))]
pub type RcProtect = unsafe extern "C" fn(i64, *mut c_void) -> std::ffi::c_int;
pub type RcCallback = Option<unsafe extern "C" fn(*const c_char, *mut c_void)>;

/// Запущенное ядро, слинкованное в само приложение (`reality-ffi`).
pub struct FfiCore {
    handle: Option<NonNull<RcCore>>,
}

// The ABI documents thread-safe calls. The wrapper moves a live handle between
// threads but serializes ownership and never calls the handle concurrently.
unsafe impl Send for FfiCore {}

impl FfiCore {
    #[cfg(windows)]
    pub fn set_lock_dir(path: &Path) -> Result<(), String> {
        let path = c_string(&path.to_string_lossy(), "каталог блокировки Windows TUN")?;
        // SAFETY: the path is a valid NUL-terminated string and the core FFI
        // stores an owned PathBuf before returning.
        let status = unsafe { reality_ffi::rc_set_lock_dir(path.as_ptr()) };
        if status == 0 {
            Ok(())
        } else {
            Err("Ядро не приняло защищённый каталог блокировки TUN.".into())
        }
    }

    pub fn start(config: &str, base_dir: &Path, tun_fd: i32) -> Result<Self, String> {
        let config = c_string(config, "конфигурация")?;
        let base = c_string(&base_dir.to_string_lossy(), "путь к конфигурации")?;
        let mut error = std::ptr::null_mut();
        // SAFETY: strings are NUL-terminated and the returned handle is wrapped exactly once.
        let handle =
            unsafe { reality_ffi::rc_start(config.as_ptr(), base.as_ptr(), tun_fd, &mut error) };
        let Some(handle) = NonNull::new(handle) else {
            return Err(take_error(error, "Ядро не смогло запуститься."));
        };
        Ok(Self {
            handle: Some(handle),
        })
    }

    #[cfg(any(target_os = "android", feature = "android-bridge-check"))]
    pub fn set_protect(callback: Option<RcProtect>, user: *mut c_void) {
        // SAFETY: callback and user pointer remain valid until reset to None.
        unsafe { reality_ffi::rc_set_protect(callback, user) };
    }

    pub fn set_log_callback(
        &mut self,
        level: &str,
        callback: RcCallback,
        user: *mut c_void,
    ) -> Result<(), String> {
        let handle = self.handle_ptr()?;
        let level = c_string(level, "уровень журнала")?;
        // SAFETY: the caller owns callback context through rc_stop.
        let status =
            unsafe { reality_ffi::rc_set_log_callback(handle, level.as_ptr(), callback, user) };
        if status == 0 {
            Ok(())
        } else {
            Err("Не удалось подключить журнал ядра.".into())
        }
    }

    pub fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> Result<(i32, String), String> {
        let handle = self.handle_ptr()?;
        let method = c_string(method, "HTTP-метод")?;
        let path = c_string(path, "путь API")?;
        let body = body.map(|b| c_string(b, "тело API-запроса")).transpose()?;
        let mut status = 0;
        // SAFETY: all arguments remain live for the duration of the ABI call.
        let response = unsafe {
            reality_ffi::rc_request(
                handle,
                method.as_ptr(),
                path.as_ptr(),
                body.as_ref().map_or(std::ptr::null(), |b| b.as_ptr()),
                &mut status,
            )
        };
        Ok((status, take_string(response)?))
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

        let path = format!("/groups/{group}");
        let body = serde_json::json!({ "member": member }).to_string();
        let (status, response) = self.request("PUT", &path, Some(&body))?;
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

    pub fn reload(&self, config: &str) -> Result<String, String> {
        let handle = self.handle_ptr()?;
        let config = c_string(config, "конфигурация")?;
        let mut error = std::ptr::null_mut();
        // SAFETY: config remains live during the call.
        let response = unsafe { reality_ffi::rc_reload(handle, config.as_ptr(), &mut error) };
        if response.is_null() {
            return Err(take_error(
                error,
                "Ядро не смогло перезагрузить конфигурацию.",
            ));
        }
        take_string(response)
    }

    fn handle_ptr(&self) -> Result<*mut RcCore, String> {
        self.handle
            .map(NonNull::as_ptr)
            .ok_or_else(|| "Ядро уже остановлено.".into())
    }

    pub fn stop(&mut self) {
        if let Some(handle) = self.handle.take() {
            // SAFETY: take() guarantees this handle is stopped exactly once.
            unsafe { reality_ffi::rc_stop(handle.as_ptr()) };
        }
    }
}

impl Drop for FfiCore {
    fn drop(&mut self) {
        self.stop();
    }
}

fn take_string(value: *mut c_char) -> Result<String, String> {
    let Some(value) = NonNull::new(value) else {
        return Err("Ядро вернуло пустой ответ.".into());
    };
    // SAFETY: string ownership is transferred by the ABI and released with rc_free_string.
    unsafe {
        let result = CStr::from_ptr(value.as_ptr())
            .to_string_lossy()
            .into_owned();
        reality_ffi::rc_free_string(value.as_ptr());
        Ok(result)
    }
}

fn take_error(error: *mut c_char, fallback: &str) -> String {
    let Some(error) = NonNull::new(error) else {
        return fallback.to_owned();
    };
    // SAFETY: error strings returned by the ABI use rc_free_string.
    unsafe {
        let message = CStr::from_ptr(error.as_ptr())
            .to_string_lossy()
            .into_owned();
        reality_ffi::rc_free_string(error.as_ptr());
        redact_sensitive_text(&message)
    }
}

fn c_string(value: &str, field: &str) -> Result<CString, String> {
    CString::new(value).map_err(|_| format!("В поле «{field}» обнаружен нулевой байт."))
}
