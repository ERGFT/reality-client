use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    path::{Path, PathBuf},
    ptr::NonNull,
    sync::Arc,
};

use crate::security::redact_sensitive_text;
use libloading::Library;

type RcStart =
    unsafe extern "C" fn(*const c_char, *const c_char, c_int, *mut *mut c_char) -> *mut c_void;
type RcStop = unsafe extern "C" fn(*mut c_void);
type RcRequest = unsafe extern "C" fn(
    *mut c_void,
    *const c_char,
    *const c_char,
    *const c_char,
    *mut c_int,
) -> *mut c_char;
type RcReload = unsafe extern "C" fn(*mut c_void, *const c_char, *mut *mut c_char) -> *mut c_char;
type RcFreeString = unsafe extern "C" fn(*mut c_char);
#[cfg(any(target_os = "android", feature = "android-bridge-check"))]
pub type RcProtect = unsafe extern "C" fn(i64, *mut c_void) -> c_int;
#[cfg(any(target_os = "android", feature = "android-bridge-check"))]
type RcSetProtect = unsafe extern "C" fn(Option<RcProtect>, *mut c_void);
pub type RcCallback = Option<unsafe extern "C" fn(*const c_char, *mut c_void)>;
type RcSetLogCallback =
    unsafe extern "C" fn(*mut c_void, *const c_char, RcCallback, *mut c_void) -> c_int;

struct CoreFunctions {
    start: RcStart,
    stop: RcStop,
    request: RcRequest,
    reload: RcReload,
    free_string: RcFreeString,
    #[cfg(any(target_os = "android", feature = "android-bridge-check"))]
    set_protect: RcSetProtect,
    set_log_callback: RcSetLogCallback,
}

pub struct CoreLibrary {
    _library: Library,
    functions: CoreFunctions,
}

pub struct FfiCore {
    library: Arc<CoreLibrary>,
    handle: Option<NonNull<c_void>>,
}

// The ABI documents thread-safe calls. The wrapper moves a live handle between
// threads but serializes ownership and never calls the handle concurrently.
unsafe impl Send for FfiCore {}

impl FfiCore {
    pub fn load() -> Result<Arc<CoreLibrary>, String> {
        let path = find_library()?;
        Self::load_from(&path)
    }

    pub fn load_from(path: &Path) -> Result<Arc<CoreLibrary>, String> {
        // SAFETY: candidates are resolved from the app bundle or its private data directory.
        let library = unsafe { Library::new(path) }.map_err(|e| {
            format!(
                "Не удалось загрузить библиотеку Rust-ядра {}: {e}",
                path.display()
            )
        })?;
        let functions = unsafe {
            CoreFunctions {
                start: *library
                    .get(b"rc_start\0")
                    .map_err(|e| missing_symbol("rc_start", e))?,
                stop: *library
                    .get(b"rc_stop\0")
                    .map_err(|e| missing_symbol("rc_stop", e))?,
                request: *library
                    .get(b"rc_request\0")
                    .map_err(|e| missing_symbol("rc_request", e))?,
                reload: *library
                    .get(b"rc_reload\0")
                    .map_err(|e| missing_symbol("rc_reload", e))?,
                free_string: *library
                    .get(b"rc_free_string\0")
                    .map_err(|e| missing_symbol("rc_free_string", e))?,
                #[cfg(any(target_os = "android", feature = "android-bridge-check"))]
                set_protect: *library
                    .get(b"rc_set_protect\0")
                    .map_err(|e| missing_symbol("rc_set_protect", e))?,
                set_log_callback: *library
                    .get(b"rc_set_log_callback\0")
                    .map_err(|e| missing_symbol("rc_set_log_callback", e))?,
            }
        };
        Ok(Arc::new(CoreLibrary {
            _library: library,
            functions,
        }))
    }

    pub fn start(
        library: Arc<CoreLibrary>,
        config: &str,
        base_dir: &Path,
        tun_fd: i32,
    ) -> Result<Self, String> {
        let config = c_string(config, "конфигурация")?;
        let base = c_string(&base_dir.to_string_lossy(), "путь к конфигурации")?;
        let mut error = std::ptr::null_mut();
        // SAFETY: strings are NUL-terminated and the returned handle is wrapped exactly once.
        let handle = unsafe {
            (library.functions.start)(config.as_ptr(), base.as_ptr(), tun_fd, &mut error)
        };
        let Some(handle) = NonNull::new(handle) else {
            return Err(take_error(&library, error, "Ядро не смогло запуститься."));
        };
        Ok(Self {
            library,
            handle: Some(handle),
        })
    }

    #[cfg(any(target_os = "android", feature = "android-bridge-check"))]
    pub fn library(&self) -> &CoreLibrary {
        &self.library
    }

    #[cfg(any(target_os = "android", feature = "android-bridge-check"))]
    pub fn set_protect(library: &CoreLibrary, callback: Option<RcProtect>, user: *mut c_void) {
        // SAFETY: callback and user pointer remain valid until reset to None.
        unsafe { (library.functions.set_protect)(callback, user) };
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
        let status = unsafe {
            (self.library.functions.set_log_callback)(handle, level.as_ptr(), callback, user)
        };
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
            (self.library.functions.request)(
                handle,
                method.as_ptr(),
                path.as_ptr(),
                body.as_ref().map_or(std::ptr::null(), |b| b.as_ptr()),
                &mut status,
            )
        };
        Ok((status, take_string(&self.library, response)?))
    }

    pub fn reload(&self, config: &str) -> Result<String, String> {
        let handle = self.handle_ptr()?;
        let config = c_string(config, "конфигурация")?;
        let mut error = std::ptr::null_mut();
        // SAFETY: config remains live during the call; strings are freed via the same library.
        let response =
            unsafe { (self.library.functions.reload)(handle, config.as_ptr(), &mut error) };
        if response.is_null() {
            return Err(take_error(
                &self.library,
                error,
                "Ядро не смогло перезагрузить конфигурацию.",
            ));
        }
        take_string(&self.library, response)
    }

    fn handle_ptr(&self) -> Result<*mut c_void, String> {
        self.handle
            .map(NonNull::as_ptr)
            .ok_or_else(|| "Ядро уже остановлено.".into())
    }

    pub fn stop(&mut self) {
        if let Some(handle) = self.handle.take() {
            // SAFETY: take() guarantees this handle is stopped exactly once.
            unsafe { (self.library.functions.stop)(handle.as_ptr()) };
        }
    }
}

impl Drop for FfiCore {
    fn drop(&mut self) {
        self.stop();
    }
}

fn take_string(library: &CoreLibrary, value: *mut c_char) -> Result<String, String> {
    let Some(value) = NonNull::new(value) else {
        return Err("Ядро вернуло пустой ответ.".into());
    };
    // SAFETY: string ownership is transferred by the ABI and released with rc_free_string.
    unsafe {
        let result = CStr::from_ptr(value.as_ptr())
            .to_string_lossy()
            .into_owned();
        (library.functions.free_string)(value.as_ptr());
        Ok(result)
    }
}

fn take_error(library: &CoreLibrary, error: *mut c_char, fallback: &str) -> String {
    let Some(error) = NonNull::new(error) else {
        return fallback.to_owned();
    };
    // SAFETY: error strings returned by the ABI use rc_free_string.
    unsafe {
        let message = CStr::from_ptr(error.as_ptr())
            .to_string_lossy()
            .into_owned();
        (library.functions.free_string)(error.as_ptr());
        redact_sensitive_text(&message)
    }
}

fn c_string(value: &str, field: &str) -> Result<CString, String> {
    CString::new(value).map_err(|_| format!("В поле «{field}» обнаружен нулевой байт."))
}

fn missing_symbol(name: &str, error: libloading::Error) -> String {
    format!("Библиотека ядра не содержит функцию {name}: {error}")
}

fn find_library() -> Result<PathBuf, String> {
    let file_name = if cfg!(windows) {
        "reality.dll"
    } else if cfg!(target_os = "macos") {
        "libreality.dylib"
    } else {
        "libreality.so"
    };
    let mut candidates = Vec::new();
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
    candidates.into_iter().find(|path| path.is_file()).ok_or_else(|| {
        format!("Не найдена библиотека Rust-ядра {file_name}; соберите reality-ffi и положите библиотеку рядом с приложением.")
    })
}
