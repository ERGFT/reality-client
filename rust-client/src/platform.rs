use std::path::PathBuf;

#[cfg(windows)]
pub struct SingleInstanceGuard(windows::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        use windows::Win32::{Foundation::CloseHandle, System::Threading::ReleaseMutex};

        // SAFETY: this guard owns the handle returned by CreateMutexW and is
        // the owning thread for the mutex until it is dropped.
        unsafe {
            let _ = ReleaseMutex(self.0);
            let _ = CloseHandle(self.0);
        }
    }
}

#[cfg(windows)]
pub fn acquire_single_instance() -> Result<Option<SingleInstanceGuard>, String> {
    let name = format!("Local\\RealityClientRust_{}", current_user_sid()?);
    acquire_single_instance_named(&name)
}

#[cfg(windows)]
fn acquire_single_instance_named(name: &str) -> Result<Option<SingleInstanceGuard>, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        Win32::{
            Foundation::{ERROR_ALREADY_EXISTS, GetLastError, SetLastError, WIN32_ERROR},
            System::Threading::CreateMutexW,
        },
        core::PCWSTR,
    };

    let name: Vec<u16> = std::ffi::OsStr::new(name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // CreateMutexW reports ERROR_ALREADY_EXISTS through the calling thread's
    // last-error value even when it successfully returns an existing handle.
    unsafe { SetLastError(WIN32_ERROR(0)) };
    // SAFETY: `name` is NUL-terminated and remains alive through the call.
    let handle = unsafe { CreateMutexW(None, true, PCWSTR(name.as_ptr())) }
        .map_err(|problem| format!("Не удалось создать mutex клиента: {problem}"))?;
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        // This call did not acquire ownership when the named mutex already
        // existed, so only close the additional handle.
        unsafe { windows::Win32::Foundation::CloseHandle(handle) }
            .map_err(|problem| format!("Не удалось закрыть дублирующий handle mutex: {problem}"))?;
        return Ok(None);
    }
    Ok(Some(SingleInstanceGuard(handle)))
}

#[cfg(windows)]
pub fn notify_already_running() {
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONINFORMATION, MB_OK, MessageBoxW};
    use windows::core::w;

    // SAFETY: both wide string literals are statically NUL-terminated.
    unsafe {
        let _ = MessageBoxW(
            None,
            w!("Reality Client уже запущен."),
            w!("Reality Client"),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

#[cfg(all(test, windows))]
mod single_instance_tests {
    use super::acquire_single_instance_named;

    #[test]
    fn named_mutex_allows_only_one_live_instance_and_releases_on_drop() {
        let name = format!(
            "Local\\RealityClientRust-Test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let first = acquire_single_instance_named(&name).unwrap();
        assert!(first.is_some());
        assert!(acquire_single_instance_named(&name).unwrap().is_none());
        drop(first);
        assert!(acquire_single_instance_named(&name).unwrap().is_some());
    }
}

#[cfg(target_os = "android")]
static ANDROID_DATA_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
#[cfg(any(target_os = "android", feature = "android-bridge-check"))]
static ANDROID_ACTIVITY: std::sync::OnceLock<std::sync::Mutex<Option<AndroidActivity>>> =
    std::sync::OnceLock::new();

#[cfg(any(target_os = "android", feature = "android-bridge-check"))]
struct AndroidActivity {
    vm: jni::JavaVM,
    activity: jni::refs::Global<jni::objects::JObject<'static>>,
}

pub fn app_data_dir() -> Result<PathBuf, String> {
    #[cfg(windows)]
    {
        let root = std::env::var_os("LOCALAPPDATA").ok_or("Не найдена папка LOCALAPPDATA.")?;
        Ok(PathBuf::from(root).join("RealityClient"))
    }
    #[cfg(target_os = "linux")]
    {
        linux_data_dir(
            std::env::var_os("XDG_DATA_HOME").as_deref(),
            std::env::var_os("HOME").as_deref(),
        )
    }
    #[cfg(target_os = "android")]
    {
        ANDROID_DATA_DIR
            .get()
            .cloned()
            .ok_or_else(|| "Android-приложение не передало приватную папку данных.".into())
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME").ok_or("Не найдена домашняя папка пользователя.")?;
        Ok(PathBuf::from(home).join("Library/Application Support/RealityClient"))
    }
    #[cfg(not(any(
        windows,
        target_os = "linux",
        target_os = "android",
        target_os = "macos"
    )))]
    {
        Err("Папка данных клиента для этой ОС не настроена.".into())
    }
}

#[cfg(any(target_os = "linux", test))]
fn linux_data_dir(
    xdg_data_home: Option<&std::ffi::OsStr>,
    home: Option<&std::ffi::OsStr>,
) -> Result<PathBuf, String> {
    if let Some(root) = xdg_data_home
        .map(PathBuf::from)
        .filter(|path| is_linux_absolute(path))
    {
        return Ok(root.join("reality-client"));
    }
    let home = home
        .map(PathBuf::from)
        .filter(|path| is_linux_absolute(path))
        .ok_or("Не найдена абсолютная домашняя папка пользователя.")?;
    Ok(home.join(".local/share/reality-client"))
}

#[cfg(any(target_os = "linux", test))]
fn is_linux_absolute(path: &std::path::Path) -> bool {
    path.as_os_str().to_string_lossy().starts_with('/')
}

#[cfg(test)]
mod linux_data_dir_tests {
    use super::linux_data_dir;
    use std::ffi::OsStr;
    use std::path::Path;

    #[test]
    fn prefers_absolute_xdg_data_home() {
        assert_eq!(
            linux_data_dir(
                Some(OsStr::new("/data/user")),
                Some(OsStr::new("/home/user"))
            )
            .unwrap(),
            Path::new("/data/user/reality-client")
        );
    }

    #[test]
    fn falls_back_to_absolute_home_when_xdg_path_is_relative() {
        assert_eq!(
            linux_data_dir(
                Some(OsStr::new("relative/data")),
                Some(OsStr::new("/home/user"))
            )
            .unwrap(),
            Path::new("/home/user/.local/share/reality-client")
        );
    }

    #[test]
    fn rejects_missing_empty_and_relative_home_paths() {
        for home in [
            None,
            Some(OsStr::new("")),
            Some(OsStr::new("relative/home")),
        ] {
            assert!(linux_data_dir(None, home).is_err());
        }
    }

    #[test]
    fn absolute_xdg_path_does_not_require_home() {
        assert_eq!(
            linux_data_dir(Some(OsStr::new("/data/user")), None).unwrap(),
            Path::new("/data/user/reality-client")
        );
    }
}

#[cfg(target_os = "android")]
pub fn initialize_android_data_dir(app: &slint::android::AndroidApp) -> Result<(), String> {
    let (path, android_activity) =
        initialize_android_context(app.vm_as_ptr(), app.activity_as_ptr())?;
    match ANDROID_DATA_DIR.set(path.clone()) {
        Ok(()) => {}
        Err(path) => {
            ensure_same_android_data_dir(ANDROID_DATA_DIR.get().map(PathBuf::as_path), &path)?
        }
    }
    replace_android_activity(android_activity);
    Ok(())
}

#[cfg(any(target_os = "android", feature = "android-bridge-check"))]
fn ensure_same_android_data_dir(
    existing: Option<&std::path::Path>,
    requested: &std::path::Path,
) -> Result<(), String> {
    if existing.is_some_and(|path| path != requested) {
        return Err("Android-приложение сменило приватную папку данных во время работы.".into());
    }
    Ok(())
}

#[cfg(any(target_os = "android", feature = "android-bridge-check"))]
fn replace_android_activity(activity: AndroidActivity) {
    let slot = ANDROID_ACTIVITY.get_or_init(|| std::sync::Mutex::new(None));
    replace_slot(slot, activity);
}

#[cfg(any(target_os = "android", feature = "android-bridge-check"))]
fn replace_slot<T>(slot: &std::sync::Mutex<Option<T>>, value: T) {
    *lock_recover(slot) = Some(value);
}

#[cfg(any(target_os = "android", feature = "android-bridge-check"))]
fn clear_slot_if<T>(slot: &std::sync::Mutex<Option<T>>, matches: impl FnOnce(&T) -> bool) -> bool {
    let mut current = lock_recover(slot);
    if current.as_ref().is_some_and(matches) {
        *current = None;
        true
    } else {
        false
    }
}

#[cfg(any(target_os = "android", feature = "android-bridge-check"))]
fn lock_recover<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            let guard = poisoned.into_inner();
            mutex.clear_poison();
            guard
        }
    }
}

#[cfg(any(target_os = "android", feature = "android-bridge-check"))]
fn initialize_android_context(
    vm_ptr: *mut std::ffi::c_void,
    activity_ptr: *mut std::ffi::c_void,
) -> Result<(PathBuf, AndroidActivity), String> {
    use jni::{
        JavaVM,
        objects::{JObject, JString},
        refs::Global,
    };

    // SAFETY: AndroidApp owns the process JVM reference for its full lifetime.
    let vm = unsafe { JavaVM::from_raw(vm_ptr.cast()) };
    let (path, activity) = vm
        .attach_current_thread(|env| -> jni::errors::Result<_> {
            let raw_activity_global: jni::sys::jobject = activity_ptr.cast();
            // SAFETY: AndroidApp documents this as a valid, unowned global reference.
            // Cast it as borrowed so JNI does not try to delete AndroidApp's reference.
            let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw_activity_global)? };
            let files_dir = env
                .call_method(
                    activity.as_ref(),
                    jni::jni_str!("getFilesDir"),
                    jni::jni_sig!("()Ljava/io/File;"),
                    &[],
                )?
                .l()?;
            let path_object = env
                .call_method(
                    &files_dir,
                    jni::jni_str!("getAbsolutePath"),
                    jni::jni_sig!("()Ljava/lang/String;"),
                    &[],
                )?
                .l()?;
            let path = env.cast_local::<JString>(path_object)?.try_to_string(env)?;
            let activity = env.new_global_ref(activity.as_ref())?;
            Ok((PathBuf::from(path), activity))
        })
        .map_err(|e| format!("Не удалось прочитать контекст Android: {e}"))?;
    Ok((path, AndroidActivity { vm, activity }))
}

#[cfg(any(target_os = "android", feature = "android-bridge-check"))]
pub fn request_android_vpn(
    config: &str,
    base_dir: &std::path::Path,
    remove_profile_secret: bool,
) -> Result<(), String> {
    use jni::objects::{JObject, JValue};

    let slot = ANDROID_ACTIVITY
        .get()
        .ok_or("Android Activity ещё не инициализирована.")?;
    let state_guard = lock_recover(slot);
    let state = state_guard
        .as_ref()
        .ok_or("Android Activity ещё не инициализирована.")?;
    state
        .vm
        .attach_current_thread(|env| -> jni::errors::Result<()> {
            let config = env.new_string(config)?;
            let config_object: JObject = config.into();
            let base_dir = env.new_string(base_dir.to_string_lossy())?;
            let base_dir_object: JObject = base_dir.into();
            env.call_method(
                state.activity.as_ref(),
                jni::jni_str!("requestVpn"),
                jni::jni_sig!("(Ljava/lang/String;Ljava/lang/String;Z)V"),
                &[
                    JValue::Object(&config_object),
                    JValue::Object(&base_dir_object),
                    JValue::Bool(remove_profile_secret),
                ],
            )?;
            Ok(())
        })
        .map_err(|e| format!("Android не смог запросить разрешение VPN: {e}"))?;
    crate::android_bridge::set_pending();
    Ok(())
}

#[cfg(any(target_os = "android", feature = "android-bridge-check"))]
pub fn stop_android_vpn() -> Result<(), String> {
    let slot = ANDROID_ACTIVITY
        .get()
        .ok_or("Android Activity ещё не инициализирована.")?;
    let state_guard = lock_recover(slot);
    let state = state_guard
        .as_ref()
        .ok_or("Android Activity ещё не инициализирована.")?;
    state
        .vm
        .attach_current_thread(|env| -> jni::errors::Result<()> {
            env.call_method(
                state.activity.as_ref(),
                jni::jni_str!("stopVpn"),
                jni::jni_sig!("()V"),
                &[],
            )?;
            Ok(())
        })
        .map_err(|e| format!("Не удалось остановить Android VPN: {e}"))?;
    Ok(())
}

#[cfg(target_os = "android")]
pub fn read_android_clipboard_text() -> Result<String, String> {
    use jni::objects::JString;

    let slot = ANDROID_ACTIVITY
        .get()
        .ok_or("Android Activity ещё не инициализирована.")?;
    let state_guard = lock_recover(slot);
    let state = state_guard
        .as_ref()
        .ok_or("Android Activity ещё не инициализирована.")?;
    state
        .vm
        .attach_current_thread(|env| -> jni::errors::Result<String> {
            let value = env
                .call_method(
                    state.activity.as_ref(),
                    jni::jni_str!("readClipboardText"),
                    jni::jni_sig!("()Ljava/lang/String;"),
                    &[],
                )?
                .l()?;
            env.cast_local::<JString>(value)?.try_to_string(env)
        })
        .map_err(|e| format!("Не удалось прочитать буфер обмена Android: {e}"))
}

#[cfg(target_os = "android")]
pub fn list_android_launchable_apps() -> Result<Vec<(String, String)>, String> {
    use jni::objects::JString;

    let slot = ANDROID_ACTIVITY
        .get()
        .ok_or("Android Activity ещё не инициализирована.")?;
    let state_guard = lock_recover(slot);
    let state = state_guard
        .as_ref()
        .ok_or("Android Activity ещё не инициализирована.")?;
    let encoded = state
        .vm
        .attach_current_thread(|env| -> jni::errors::Result<String> {
            let value = env
                .call_method(
                    state.activity.as_ref(),
                    jni::jni_str!("listLaunchableApps"),
                    jni::jni_sig!("()Ljava/lang/String;"),
                    &[],
                )?
                .l()?;
            env.cast_local::<JString>(value)?.try_to_string(env)
        })
        .map_err(|problem| format!("Не удалось получить список приложений Android: {problem}"))?;
    let apps: Vec<serde_json::Value> = serde_json::from_str(&encoded)
        .map_err(|problem| format!("Android вернул неверный список приложений: {problem}"))?;
    Ok(apps
        .into_iter()
        .filter_map(|app| {
            Some((
                app.get("package")?.as_str()?.to_owned(),
                app.get("label")?.as_str()?.to_owned(),
            ))
        })
        .collect())
}

#[cfg(any(target_os = "android", feature = "android-bridge-check"))]
fn clear_android_activity(
    env: &jni::Env<'_>,
    activity: &jni::objects::JObject<'_>,
) -> Result<(), String> {
    let Some(slot) = ANDROID_ACTIVITY.get() else {
        return Ok(());
    };
    let mut comparison_error = None;
    clear_slot_if(slot, |state| {
        match env.is_same_object(state.activity.as_ref(), activity) {
            Ok(is_current) => is_current,
            Err(problem) => {
                comparison_error = Some(problem);
                false
            }
        }
    });
    if let Some(problem) = comparison_error {
        return Err(format!("Не удалось проверить Android Activity: {problem}"));
    }
    Ok(())
}

#[cfg(all(test, feature = "android-bridge-check"))]
mod android_lifecycle_tests {
    use super::{clear_slot_if, ensure_same_android_data_dir, replace_slot};
    use std::{path::Path, sync::Mutex};

    #[test]
    fn android_activity_registry_replaces_and_clears_only_the_matching_activity() {
        let slot = Mutex::new(Some("old"));
        replace_slot(&slot, "current");
        assert!(!clear_slot_if(&slot, |activity| *activity == "old"));
        assert_eq!(*slot.lock().unwrap(), Some("current"));
        assert!(clear_slot_if(&slot, |activity| *activity == "current"));
        assert_eq!(*slot.lock().unwrap(), None);
    }

    #[test]
    fn android_activity_recreation_keeps_the_same_private_data_directory() {
        assert!(
            ensure_same_android_data_dir(
                Some(Path::new("/data/user/0/com.example/files")),
                Path::new("/data/user/0/com.example/files")
            )
            .is_ok()
        );
        assert!(
            ensure_same_android_data_dir(
                Some(Path::new("/data/user/0/com.example/files")),
                Path::new("/data/user/10/com.example/files")
            )
            .is_err()
        );
    }
}

#[cfg(any(target_os = "android", feature = "android-bridge-check"))]
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_ergft_realityclient_MainActivity_nativeActivityDestroyed<'local>(
    mut unowned_env: jni::EnvUnowned<'local>,
    activity: jni::objects::JObject<'local>,
) {
    unowned_env
        .with_env(|env| -> jni::errors::Result<()> {
            if let Err(problem) = clear_android_activity(env, &activity) {
                eprintln!("Не удалось освободить Android Activity: {problem}");
            }
            Ok(())
        })
        .resolve::<jni::errors::ThrowRuntimeExAndDefault>();
}

#[cfg(any(target_os = "android", feature = "android-bridge-check"))]
pub fn android_vpn_state() -> u8 {
    crate::android_bridge::state()
}

#[cfg(any(target_os = "android", feature = "android-bridge-check"))]
pub fn take_android_vpn_error() -> String {
    crate::android_bridge::take_error()
}

pub fn ensure_private_dir(path: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(path)
        .map_err(|e| format!("Не удалось создать папку данных клиента: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("Не удалось ограничить доступ к папке данных клиента: {e}"))?;
    }
    #[cfg(windows)]
    harden_windows_directory(path)?;
    Ok(())
}

#[cfg(windows)]
pub fn prepare_windows_tun_lock_dir() -> Result<PathBuf, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        Win32::{
            Foundation::{ERROR_ALREADY_EXISTS, HLOCAL, LocalFree},
            Security::Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, GetNamedSecurityInfoW,
                SDDL_REVISION_1, SE_FILE_OBJECT, SetNamedSecurityInfoW,
            },
            Security::{
                DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, IsWellKnownSid,
                OBJECT_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
                PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
                SECURITY_ATTRIBUTES, WinBuiltinAdministratorsSid, WinLocalSystemSid,
            },
            Storage::FileSystem::{CreateDirectoryW, FILE_ATTRIBUTE_REPARSE_POINT},
        },
        core::PCWSTR,
    };

    const LOCK_DIR_SDDL: &str = "O:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";
    let program_data = std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or("Не удалось определить защищённый каталог Windows ProgramData.")?;
    let path = program_data.join("RealityClient");
    let path_wide = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let sddl_wide = LOCK_DIR_SDDL
        .encode_utf16()
        .chain(Some(0))
        .collect::<Vec<_>>();

    struct LocalDescriptor(PSECURITY_DESCRIPTOR);
    impl Drop for LocalDescriptor {
        fn drop(&mut self) {
            if !self.0.0.is_null() {
                unsafe {
                    let _ = LocalFree(Some(HLOCAL(self.0.0)));
                }
            }
        }
    }

    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl_wide.as_ptr()),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )
    }
    .map_err(|problem| format!("Не удалось создать защищённые права Windows TUN: {problem}"))?;
    let descriptor = LocalDescriptor(descriptor);
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0.0,
        bInheritHandle: false.into(),
    };

    let existing = match unsafe { CreateDirectoryW(PCWSTR(path_wide.as_ptr()), Some(&attributes)) }
    {
        Ok(()) => false,
        Err(problem) if problem.code().0 as u32 == ERROR_ALREADY_EXISTS.0 => true,
        Err(problem) => {
            return Err(format!(
                "Не удалось создать защищённую папку {}. Запустите клиент от имени администратора для работы TUN: {problem}",
                path.display()
            ));
        }
    };

    if existing {
        let metadata = std::fs::symlink_metadata(&path).map_err(|problem| {
            format!(
                "Не удалось проверить защищённую папку {}: {problem}",
                path.display()
            )
        })?;
        use std::os::windows::fs::MetadataExt;
        if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
            return Err(format!(
                "Папка блокировки TUN {} является ссылкой или не является каталогом.",
                path.display()
            ));
        }

        let mut owner = PSID::default();
        let mut existing_descriptor = PSECURITY_DESCRIPTOR::default();
        let status = unsafe {
            GetNamedSecurityInfoW(
                PCWSTR(path_wide.as_ptr()),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION,
                Some(&mut owner),
                None,
                None,
                None,
                &mut existing_descriptor,
            )
        };
        if status.0 != 0 {
            return Err(format!(
                "Не удалось проверить владельца папки TUN {} (код Windows {}).",
                path.display(),
                status.0
            ));
        }
        let existing_descriptor = LocalDescriptor(existing_descriptor);
        let trusted_owner = unsafe {
            IsWellKnownSid(owner, WinBuiltinAdministratorsSid).as_bool()
                || IsWellKnownSid(owner, WinLocalSystemSid).as_bool()
        };
        if !trusted_owner {
            return Err(format!(
                "Папка TUN {} создана не администратором или SYSTEM; автоматический запуск TUN отменён.",
                path.display()
            ));
        }

        let mut dacl_present = false.into();
        let mut dacl_defaulted = false.into();
        let mut dacl = std::ptr::null_mut();
        unsafe {
            GetSecurityDescriptorDacl(
                descriptor.0,
                &mut dacl_present,
                &mut dacl,
                &mut dacl_defaulted,
            )
        }
        .map_err(|problem| format!("Не удалось прочитать защищённые права TUN: {problem}"))?;
        if !dacl_present.as_bool() || dacl.is_null() {
            return Err("Windows не создала закрытый ACL каталога TUN.".into());
        }
        let status = unsafe {
            SetNamedSecurityInfoW(
                PCWSTR(path_wide.as_ptr()),
                SE_FILE_OBJECT,
                OBJECT_SECURITY_INFORMATION(
                    DACL_SECURITY_INFORMATION.0 | PROTECTED_DACL_SECURITY_INFORMATION.0,
                ),
                None,
                None,
                Some(dacl),
                None,
            )
        };
        drop(existing_descriptor);
        if status.0 != 0 {
            return Err(format!(
                "Не удалось ограничить доступ к папке TUN {} (код Windows {}). Запустите клиент от имени администратора.",
                path.display(),
                status.0
            ));
        }
    }

    Ok(path)
}

#[cfg(windows)]
fn harden_windows_directory(path: &std::path::Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        Win32::{
            Foundation::{HLOCAL, LocalFree},
            Security::Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
            },
            Security::{
                DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, SetFileSecurityW,
            },
        },
        core::{Error, PCWSTR},
    };

    struct LocalBuffer(*mut std::ffi::c_void);
    impl Drop for LocalBuffer {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    let _ = LocalFree(Some(HLOCAL(self.0)));
                }
            }
        }
    }

    let user_sid = current_user_sid()?;

    let sddl = format!("D:P(A;OICI;FA;;;{user_sid})(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)");
    let sddl_wide = sddl.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let mut descriptor = windows::Win32::Security::PSECURITY_DESCRIPTOR::default();
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl_wide.as_ptr()),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )
    }
    .map_err(|e| format!("Не удалось подготовить закрытый ACL каталога: {e}"))?;
    let descriptor_buffer = LocalBuffer(descriptor.0);

    let path_wide = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let applied = unsafe {
        SetFileSecurityW(
            PCWSTR(path_wide.as_ptr()),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor,
        )
    };
    if !applied.as_bool() {
        let error = Error::from_thread();
        drop(descriptor_buffer);
        return Err(format!(
            "Не удалось ограничить ACL каталога данных Windows: {error}"
        ));
    }
    drop(descriptor_buffer);
    Ok(())
}

#[cfg(windows)]
fn current_user_sid() -> Result<String, String> {
    use windows::{
        Win32::{
            Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree},
            Security::Authorization::ConvertSidToStringSidW,
            Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser},
            System::Threading::{GetCurrentProcess, OpenProcessToken},
        },
        core::PWSTR,
    };

    struct Token(HANDLE);
    impl Drop for Token {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
    struct LocalBuffer(*mut std::ffi::c_void);
    impl Drop for LocalBuffer {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    let _ = LocalFree(Some(HLOCAL(self.0)));
                }
            }
        }
    }

    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }
        .map_err(|e| format!("Не удалось открыть токен Windows: {e}"))?;
    let token = Token(token);

    let mut required = 0u32;
    let _ = unsafe { GetTokenInformation(token.0, TokenUser, None, 0, &mut required) };
    if required < std::mem::size_of::<TOKEN_USER>() as u32 {
        return Err("Windows не вернула SID текущего пользователя.".into());
    }
    let mut token_data = vec![0u64; required.div_ceil(std::mem::size_of::<u64>() as u32) as usize];
    unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            Some(token_data.as_mut_ptr().cast()),
            required,
            &mut required,
        )
    }
    .map_err(|e| format!("Не удалось получить SID текущего пользователя: {e}"))?;
    let user = unsafe { &*(token_data.as_ptr().cast::<TOKEN_USER>()) };

    let mut sid_text = PWSTR::null();
    unsafe { ConvertSidToStringSidW(user.User.Sid, &mut sid_text) }
        .map_err(|e| format!("Не удалось преобразовать SID пользователя: {e}"))?;
    let sid_buffer = LocalBuffer(sid_text.0.cast());
    let mut sid_len = 0usize;
    unsafe {
        while *sid_text.0.add(sid_len) != 0 {
            sid_len += 1;
        }
    }
    let user_sid =
        String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(sid_text.0, sid_len) });
    drop(sid_buffer);
    Ok(user_sid)
}

#[cfg(all(test, windows))]
mod windows_acl_tests {
    use super::*;

    #[test]
    fn private_data_directory_acl_keeps_owner_access() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("reality-client-acl-{}-{nonce}", std::process::id()));
        ensure_private_dir(&path).unwrap();
        let probe = path.join("owner-access.txt");
        std::fs::write(&probe, b"ok").unwrap();
        assert_eq!(std::fs::read(&probe).unwrap(), b"ok");
        std::fs::remove_dir_all(path).unwrap();
    }
}
