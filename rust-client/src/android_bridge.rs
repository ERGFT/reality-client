use std::{
    ffi::c_void,
    path::Path,
    sync::{
        Mutex, MutexGuard, OnceLock,
        atomic::{AtomicU8, Ordering},
    },
};

use jni::{
    Env, EnvUnowned, JavaVM,
    errors::ThrowRuntimeExAndDefault,
    objects::{JObject, JString, JValue},
    refs::Global,
    sys::{jint, jstring},
};

use crate::ffi_core::{FfiCore, RcProtect};
use crate::security::redact_sensitive_text;

struct ProtectContext {
    vm: JavaVM,
    service: Global<JObject<'static>>,
}

struct AndroidCore {
    core: FfiCore,
    protect: Box<ProtectContext>,
    remove_profile_secret: bool,
}

struct AndroidSecretCleanup {
    enabled: bool,
    retained: bool,
}

struct IncomingTunFd(Option<i32>);

impl IncomingTunFd {
    fn new(fd: i32) -> Self {
        Self(Some(fd))
    }

    fn transfer_to_core(&mut self) -> i32 {
        self.0
            .take()
            .expect("TUN descriptor was already transferred")
    }
}

impl Drop for IncomingTunFd {
    fn drop(&mut self) {
        if let Some(fd) = self.0.take() {
            #[cfg(target_os = "android")]
            {
                // SAFETY: the guard exclusively owns this detached descriptor
                // until `transfer_to_core` hands it to rc_start.
                unsafe extern "C" {
                    fn close(fd: i32) -> i32;
                }
                // SAFETY: this is a live descriptor detached from a ParcelFileDescriptor.
                unsafe {
                    let _ = close(fd);
                }
            }
            #[cfg(not(target_os = "android"))]
            let _ = fd;
        }
    }
}

impl Drop for AndroidSecretCleanup {
    fn drop(&mut self) {
        if self.enabled && !self.retained {
            crate::ffi_session::erase_android_profile_secret();
        }
    }
}

static ANDROID_CORE: OnceLock<Mutex<Option<AndroidCore>>> = OnceLock::new();
static VPN_STATE: AtomicU8 = AtomicU8::new(0);
static VPN_ERROR: OnceLock<Mutex<String>> = OnceLock::new();

pub fn set_pending() {
    VPN_STATE.store(1, Ordering::Release);
}

pub fn state() -> u8 {
    VPN_STATE.load(Ordering::Acquire)
}

pub fn cleanup_stale_profile_secret_if_inactive() {
    if state() == 0 && lock_recover(core_slot()).is_none() {
        crate::ffi_session::erase_android_profile_secret();
    }
}

pub fn take_error() -> String {
    let mut error = lock_recover(VPN_ERROR.get_or_init(|| Mutex::new(String::new())));
    std::mem::take(&mut *error)
}

fn record_error(message: String) {
    *lock_recover(VPN_ERROR.get_or_init(|| Mutex::new(String::new()))) =
        redact_sensitive_text(&message);
    VPN_STATE.store(3, Ordering::Release);
}

fn core_slot() -> &'static Mutex<Option<AndroidCore>> {
    ANDROID_CORE.get_or_init(|| Mutex::new(None))
}

fn lock_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            let guard = poisoned.into_inner();
            mutex.clear_poison();
            guard
        }
    }
}

/// Called by the Android VpnService after it has established a TUN descriptor.
/// The descriptor is detached by Kotlin and ownership is passed to `rc_start`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_ergft_realityclient_RealityVpnService_nativeStart<'local>(
    mut unowned_env: EnvUnowned<'local>,
    service: JObject<'local>,
    config: JString<'local>,
    native_library_dir: JString<'local>,
    base_dir: JString<'local>,
    tun_fd: jint,
    remove_profile_secret: bool,
) -> jstring {
    unowned_env
        .with_env(|env| -> jni::errors::Result<_> {
            let result = android_start(
                env,
                &service,
                &config,
                &native_library_dir,
                &base_dir,
                tun_fd,
                remove_profile_secret,
            );
            let message = match result {
                Ok(()) => String::new(),
                Err(problem) => {
                    let safe_problem = redact_sensitive_text(&problem);
                    record_error(safe_problem.clone());
                    safe_problem
                }
            };
            Ok(env.new_string(message)?.into_raw())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_ergft_realityclient_RealityVpnService_nativeStop<'local>(
    mut unowned_env: EnvUnowned<'local>,
    _service: JObject<'local>,
) -> jstring {
    unowned_env
        .with_env(|env| -> jni::errors::Result<_> {
            let message = match android_stop() {
                Ok(()) => String::new(),
                Err(problem) => {
                    let safe_problem = redact_sensitive_text(&problem);
                    record_error(safe_problem.clone());
                    safe_problem
                }
            };
            Ok(env.new_string(message)?.into_raw())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_ergft_realityclient_MainActivity_nativeVpnPermissionDenied<
    'local,
>(
    mut unowned_env: EnvUnowned<'local>,
    _activity: JObject<'local>,
) {
    unowned_env
        .with_env(|_| -> jni::errors::Result<()> {
            VPN_STATE.store(0, Ordering::Release);
            crate::ffi_session::erase_android_profile_secret();
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_ergft_realityclient_RealityVpnService_nativeVpnStartFailed<
    'local,
>(
    mut unowned_env: EnvUnowned<'local>,
    _service: JObject<'local>,
    remove_profile_secret: bool,
    error: JString<'local>,
) {
    unowned_env
        .with_env(|env| -> jni::errors::Result<()> {
            let message = error.try_to_string(env)?;
            record_error(message);
            if remove_profile_secret {
                crate::ffi_session::erase_android_profile_secret();
            }
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>();
}

fn android_start(
    env: &mut Env<'_>,
    service: &JObject<'_>,
    config: &JString<'_>,
    native_library_dir: &JString<'_>,
    base_dir: &JString<'_>,
    tun_fd: jint,
    remove_profile_secret: bool,
) -> Result<(), String> {
    let mut secret_cleanup = AndroidSecretCleanup {
        enabled: remove_profile_secret,
        retained: false,
    };
    if tun_fd < 0 {
        return Err("Android VpnService не передал действительный TUN-дескриптор.".into());
    }
    let mut tun_fd = IncomingTunFd::new(tun_fd);
    let mut active = lock_recover(core_slot());
    if active.is_some() {
        return Err("Android-ядро уже запущено.".into());
    }

    let config = config
        .try_to_string(env)
        .map_err(|e| format!("Не удалось прочитать JSON Android-клиента: {e}"))?;
    let library_dir = native_library_dir
        .try_to_string(env)
        .map_err(|e| format!("Не удалось получить папку нативных библиотек: {e}"))?;
    let value: serde_json::Value = serde_json::from_str(&config)
        .map_err(|e| format!("Некорректный JSON Android-клиента: {e}"))?;
    let has_tun = value
        .get("inbounds")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|item| {
                item.get("type").and_then(serde_json::Value::as_str) == Some("tun")
                    || item.get("protocol").and_then(serde_json::Value::as_str) == Some("tun")
            })
        });
    if !has_tun {
        return Err("Для Android VPN-конфигурации требуется входящий тип tun.".into());
    }

    let vm = env
        .get_java_vm()
        .map_err(|e| format!("Не удалось получить JVM Android: {e}"))?;
    let protect = Box::new(ProtectContext {
        vm,
        service: env
            .new_global_ref(service)
            .map_err(|e| format!("Не удалось удержать Android VpnService: {e}"))?,
    });
    let library_path = Path::new(&library_dir).join("libreality.so");
    let library = FfiCore::load_from(&library_path)?;
    let base_dir = base_dir
        .try_to_string(env)
        .map_err(|e| format!("Не удалось получить папку конфигурации Android: {e}"))?;
    let base_dir = Path::new(&base_dir);
    crate::platform::ensure_private_dir(base_dir)?;

    FfiCore::set_protect(
        &library,
        Some(protect_socket as RcProtect),
        (&*protect as *const ProtectContext)
            .cast_mut()
            .cast::<c_void>(),
    );
    if config.contains('\0') || base_dir.to_string_lossy().contains('\0') {
        FfiCore::set_protect(&library, None, std::ptr::null_mut());
        return Err("Конфигурация или папка Android содержит недопустимый нулевой байт.".into());
    }
    // The FFI contract transfers ownership when rc_start is called. Until this
    // point, every early-return path closes the descriptor through the guard.
    let tun_fd = tun_fd.transfer_to_core();
    let core = match FfiCore::start(library.clone(), &config, base_dir, tun_fd) {
        Ok(core) => core,
        Err(problem) => {
            FfiCore::set_protect(&library, None, std::ptr::null_mut());
            return Err(problem);
        }
    };
    *active = Some(AndroidCore {
        core,
        protect,
        remove_profile_secret,
    });
    secret_cleanup.retained = true;
    VPN_STATE.store(2, Ordering::Release);
    Ok(())
}

fn android_stop() -> Result<(), String> {
    let mut active = lock_recover(core_slot());
    if let Some(mut session) = active.take() {
        // rc_stop stops workers before the callback context is released.
        session.core.stop();
        FfiCore::set_protect(session.core.library(), None, std::ptr::null_mut());
        if session.remove_profile_secret {
            crate::ffi_session::erase_android_profile_secret();
        }
        drop(session.protect);
    }
    VPN_STATE.store(0, Ordering::Release);
    Ok(())
}

unsafe extern "C" fn protect_socket(fd: i64, user: *mut c_void) -> jint {
    if user.is_null() || fd < 0 || fd > jint::MAX as i64 {
        return 0;
    }
    // SAFETY: `user` points to the Box retained in ANDROID_CORE until rc_stop
    // has joined the core workers and the callback has been unset.
    let context = unsafe { &*(user.cast::<ProtectContext>()) };
    context
        .vm
        .attach_current_thread(|env| {
            env.call_method(
                context.service.as_ref(),
                jni::jni_str!("protect"),
                jni::jni_sig!("(I)Z"),
                &[JValue::Int(fd as jint)],
            )?
            .z()
        })
        .map_or(0, |protected| if protected { 1 } else { 0 })
}

#[cfg(test)]
mod tests {
    use super::lock_recover;
    use std::sync::{Arc, Mutex};

    #[test]
    fn poisoned_android_state_lock_can_be_recovered() {
        let state = Arc::new(Mutex::new(0));
        let worker_state = state.clone();
        let _ = std::thread::spawn(move || {
            let _guard = worker_state.lock().unwrap();
            panic!("simulate a panic while holding Android state");
        })
        .join();

        assert!(state.is_poisoned());
        *lock_recover(&state) = 2;
        assert!(!state.is_poisoned());
        assert_eq!(*state.lock().unwrap(), 2);
    }
}
