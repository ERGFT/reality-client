#[cfg(any(target_os = "android", feature = "android-bridge-check"))]
mod android_bridge;
mod core;
mod ffi_core;
mod ffi_session;
mod platform;
mod profiles;
mod security;
#[cfg(windows)]
mod windows_proxy;

use std::{
    collections::VecDeque,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

#[cfg(any(windows, target_os = "linux"))]
use std::sync::atomic::AtomicU8;

use core::{check_config_file, default_config_path, load_advanced_config_path};
use ffi_session::CoreSession;
use profiles::{ProfileStore, storage_backend_description};
use security::redact_sensitive_text;
use slint::{
    CloseRequestResponse, ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel,
};
use std::path::PathBuf;
use std::time::Duration;
use zeroize::Zeroizing;

slint::include_modules!();

#[derive(Clone, Debug, PartialEq, Eq)]
struct SelectableGroup {
    tag: String,
    members: Vec<String>,
    current: Option<String>,
}

struct RuntimeSnapshot {
    groups: Vec<SelectableGroup>,
    uploaded: u64,
    downloaded: u64,
    connections: u64,
}

struct SavedProfile {
    index: usize,
    names: Vec<String>,
    name: String,
    link: Zeroizing<String>,
}

type SelectedProfileLink = Arc<Mutex<Option<(usize, Zeroizing<String>)>>>;

fn save_profile_for_connection(
    profile_store: &Arc<Mutex<Option<ProfileStore>>>,
    name: &str,
    link: &Zeroizing<String>,
    selected: Option<usize>,
) -> Result<SavedProfile, String> {
    let mut guard = profile_store
        .lock()
        .map_err(|_| "Хранилище профилей недоступно.".to_owned())?;
    let store = guard.as_mut().ok_or_else(|| {
        "Защищённое хранилище профилей недоступно; ссылка не сохранена.".to_owned()
    })?;
    let name = if name.trim().is_empty() {
        "Текущий профиль"
    } else {
        name.trim()
    };
    let index = store.save(name, link, selected)?;
    Ok(SavedProfile {
        index,
        names: store.names(),
        name: name.to_owned(),
        link: Zeroizing::new(link.to_string()),
    })
}

fn apply_saved_profile(
    window: &MainWindow,
    profile: &SavedProfile,
    selected_profile_link: &SelectedProfileLink,
) {
    window.set_profile_model(ModelRc::from(Rc::new(VecModel::from(
        profile
            .names
            .iter()
            .map(|name| SharedString::from(name.as_str()))
            .collect::<Vec<_>>(),
    ))));
    window.set_selected_profile_index(profile.index as i32);
    window.set_profile_count(format!("Профили: {}", profile.names.len()).into());
    window.set_profile_name(profile.name.as_str().into());
    window.set_vless_link(profile.link.as_str().into());
    if let Ok(mut cache) = selected_profile_link.lock() {
        *cache = Some((profile.index, Zeroizing::new(profile.link.to_string())));
    }
}

const STARTER_CONFIG: &str = r#"{
  "inbounds": [{ "type": "mixed", "tag": "local", "listen": "127.0.0.1", "listen_port": 1080 }],
  "outbounds": [
    { "type": "vless", "tag": "proxy", "link_file": "server.txt" },
    { "type": "direct", "tag": "direct" },
    { "type": "block", "tag": "block" }
  ],
  "route": { "final": "proxy" }
}"#;

pub fn run_ui() -> Result<(), slint::PlatformError> {
    #[cfg(windows)]
    let _single_instance = match platform::acquire_single_instance() {
        Ok(Some(guard)) => guard,
        Ok(None) => {
            platform::notify_already_running();
            return Ok(());
        }
        Err(problem) => return Err(slint::PlatformError::Other(problem)),
    };

    let window = MainWindow::new()?;
    let profile_store = Arc::new(Mutex::new(ProfileStore::open_default().ok()));
    let selected_profile_link = Arc::new(Mutex::new(None::<(usize, Zeroizing<String>)>));
    let core_session: Arc<Mutex<Option<CoreSession>>> = Arc::new(Mutex::new(None));
    let core_logs = Arc::new(Mutex::new(VecDeque::<String>::new()));
    let is_starting = Arc::new(AtomicBool::new(false));
    #[cfg(any(windows, target_os = "linux"))]
    let close_state = Arc::new(AtomicU8::new(0));

    #[cfg(any(windows, target_os = "linux"))]
    window.window().on_close_requested({
        let close_state = close_state.clone();
        let is_starting = is_starting.clone();
        let weak_window = window.as_weak();
        move || {
            if close_state.load(Ordering::Acquire) == 0 {
                let was_busy = is_starting.swap(true, Ordering::AcqRel);
                close_state.store(if was_busy { 1 } else { 2 }, Ordering::Release);
                if let Some(window) = weak_window.upgrade() {
                    window.set_status_text("Завершение…".into());
                    window.set_detail_text(
                        "Дожидаюсь текущей операции, останавливаю ядро и проверяю восстановление сети…".into(),
                    );
                }
            }
            CloseRequestResponse::KeepWindowShown
        }
    });
    window.set_recovery_visible(core::has_proxy_recovery());
    window.set_system_proxy_supported(cfg!(windows));
    window.set_clipboard_paste_supported(cfg!(windows));
    window.set_file_dialog_supported(cfg!(any(windows, target_os = "linux")));
    let platform_guidance = if cfg!(windows) {
        "Windows Rust-клиент пока принимает только desktop-конфиги без TUN; TUN-конфиги будут отклонены."
    } else if cfg!(target_os = "linux") {
        "Linux TUN требует root или CAP_NET_ADMIN; после аварии с strict_route используйте reality-client --tun-cleanup."
    } else if cfg!(target_os = "android") {
        "Android VPN пока экспериментальный и не проверен на устройстве; нужны DNS и TUN address. Пользовательские маршруты, per-app и strict_route пока не поддержаны."
    } else {
        "TUN на этой платформе пока не поддерживается клиентом."
    };
    window.set_config_platform_guidance(platform_guidance.into());
    let profile_model = Rc::new(VecModel::from(
        profile_store
            .lock()
            .expect("profile-store mutex poisoned")
            .as_ref()
            .map(ProfileStore::names)
            .unwrap_or_default()
            .into_iter()
            .map(SharedString::from)
            .collect::<Vec<_>>(),
    ));
    window.set_profile_model(ModelRc::from(profile_model.clone()));
    let selectable_groups = Arc::new(Mutex::new(Vec::<SelectableGroup>::new()));
    let updating_group_controls = Arc::new(AtomicBool::new(false));
    window.set_log_text("Предупреждения и ошибки ядра появятся здесь.".into());
    window.set_profile_count(format!("Профили: {}", profile_model.row_count()).into());
    if let Ok(default_path) = default_config_path() {
        let path = load_advanced_config_path()
            .ok()
            .flatten()
            .filter(|path| path.is_file())
            .unwrap_or(default_path);
        window.set_config_editor_path(path.to_string_lossy().into_owned().into());
        window.set_config_editor_text(STARTER_CONFIG.into());
        let weak_window = window.as_weak();
        std::thread::spawn(move || {
            let result = std::fs::read_to_string(&path);
            let _ = slint::invoke_from_event_loop(move || {
                let Some(window) = weak_window.upgrade() else {
                    return;
                };
                if let Ok(text) = result {
                    window.set_config_editor_text(text.into());
                }
            });
        });
    } else {
        window.set_config_editor_text(STARTER_CONFIG.into());
    }

    if profile_store
        .lock()
        .expect("profile-store mutex poisoned")
        .is_none()
    {
        window.set_detail_text(
            "Не удалось открыть защищённое хранилище профилей. Существующие данные не изменены."
                .into(),
        );
    }

    window.on_save_profile({
        let window = window.as_weak();
        let profile_store = profile_store.clone();
        let selected_profile_link = selected_profile_link.clone();
        move || {
            let Some(window) = window.upgrade() else {
                return;
            };
            let name = window.get_profile_name().trim().to_owned();
            let link = Zeroizing::new(window.get_vless_link().trim().to_owned());
            let selected = window.get_selected_profile_index();
            let visible_count = window.get_profile_model().row_count();
            let selected =
                (selected >= 0 && (selected as usize) < visible_count).then_some(selected as usize);
            let auto_name = format!("Профиль {}", visible_count + 1);
            window.set_detail_text("Сохраняю профиль в защищённое хранилище…".into());
            let weak_window = window.as_weak();
            let worker_store = profile_store.clone();
            let worker_link_cache = selected_profile_link.clone();
            std::thread::spawn(move || {
                let result = {
                    let mut guard = worker_store.lock();
                    match guard {
                        Ok(ref mut store) => match store.as_mut() {
                            Some(store) => {
                                let profile_name = if name.is_empty() { auto_name } else { name };
                                store.save(&profile_name, &link, selected).map(|index| {
                                    (index, store.names(), store.len(), profile_name, link)
                                })
                            }
                            None => {
                                Err("Хранилище профилей недоступно; ссылка не сохранена.".into())
                            }
                        },
                        Err(_) => Err("Хранилище профилей недоступно.".into()),
                    }
                };
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(window) = weak_window.upgrade() else {
                        return;
                    };
                    match result {
                        Ok((index, names, count, profile_name, link)) => {
                            window.set_profile_model(ModelRc::from(Rc::new(VecModel::from(
                                names
                                    .iter()
                                    .map(|name| SharedString::from(name.as_str()))
                                    .collect::<Vec<_>>(),
                            ))));
                            window.set_selected_profile_index(index as i32);
                            window.set_profile_count(format!("Профили: {count}").into());
                            if let Ok(mut cache) = worker_link_cache.lock() {
                                *cache = Some((index, Zeroizing::new(link.to_string())));
                            }
                            window.set_vless_link(link.as_str().into());
                            window.set_detail_text(
                                format!(
                                    "Профиль «{profile_name}» сохранён в {}.",
                                    storage_backend_description()
                                )
                                .into(),
                            );
                        }
                        Err(problem) => window.set_detail_text(problem.into()),
                    }
                });
            });
        }
    });

    #[cfg(windows)]
    window.on_paste_profile_link({
        let weak_window = window.as_weak();
        move || {
            let weak_window = weak_window.clone();
            std::thread::spawn(move || {
                let result = read_clipboard_profile_link();
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(window) = weak_window.upgrade() else {
                        return;
                    };
                    match result {
                        Ok(link) => {
                            window.set_vless_link(link.as_str().into());
                            window.set_detail_text(
                                "Ссылка вставлена из буфера обмена и скрыта в поле. Сохраните профиль."
                                    .into(),
                            );
                        }
                        Err(problem) => window.set_detail_text(problem.into()),
                    }
                });
            });
        }
    });

    window.on_profile_selected({
        let window = window.as_weak();
        let profile_store = profile_store.clone();
        let selected_profile_link = selected_profile_link.clone();
        move |index| {
            let Some(window) = window.upgrade() else {
                return;
            };
            if index < 0 {
                return;
            }
            if let Ok(mut cache) = selected_profile_link.lock() {
                *cache = None;
            }
            // Don't let Connect mistake the previously selected profile's
            // still-visible link for an edit to the newly selected profile.
            window.set_profile_name("".into());
            window.set_vless_link("".into());
            window.set_detail_text("Читаю профиль из защищённого хранилища…".into());
            let weak_window = window.as_weak();
            let profile_store = profile_store.clone();
            let selected_profile_link = selected_profile_link.clone();
            std::thread::spawn(move || {
                let result = match profile_store.lock() {
                    Ok(store) => match store.as_ref() {
                        Some(store) => store
                            .read_link(index as usize)
                            .map(|link| (link, store.names())),
                        None => Err("Хранилище профилей недоступно.".into()),
                    },
                    Err(_) => Err("Хранилище профилей недоступно.".into()),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(window) = weak_window.upgrade() else {
                        return;
                    };
                    if window.get_selected_profile_index() != index {
                        return;
                    }
                    match result {
                        Ok((link, names)) => {
                            let Some(name) = names.get(index as usize) else {
                                return;
                            };
                            if let Ok(mut cache) = selected_profile_link.lock() {
                                *cache = Some((index as usize, Zeroizing::new(link.to_string())));
                            }
                            window.set_profile_name(name.as_str().into());
                            window.set_vless_link(link.as_str().into());
                            window.set_detail_text(
                                format!("Профиль получен из {}.", storage_backend_description())
                                    .into(),
                            );
                        }
                        Err(problem) => {
                            window.set_vless_link("".into());
                            window.set_detail_text(problem.into());
                        }
                    }
                });
            });
        }
    });

    window.on_config_load_requested({
        let window = window.as_weak();
        let is_starting = is_starting.clone();
        move || {
            if is_starting.swap(true, Ordering::AcqRel) {
                return;
            }
            let Some(window) = window.upgrade() else {
                is_starting.store(false, Ordering::Release);
                return;
            };
            let path = PathBuf::from(window.get_config_editor_path().as_str());
            window.set_detail_text("Загружаю JSON…".into());
            let weak_window = window.as_weak();
            let is_starting = is_starting.clone();
            std::thread::spawn(move || {
                let result = std::fs::read_to_string(&path);
                let _ = slint::invoke_from_event_loop(move || {
                    is_starting.store(false, Ordering::Release);
                    let Some(window) = weak_window.upgrade() else {
                        return;
                    };
                    match result {
                        Ok(text) => {
                            window.set_config_editor_text(text.into());
                            window.set_detail_text(
                                format!("Загружен JSON: {}", path.display()).into(),
                            );
                        }
                        Err(problem) => window.set_detail_text(
                            format!("Не удалось загрузить JSON: {problem}").into(),
                        ),
                    }
                });
            });
        }
    });

    #[cfg(any(windows, target_os = "linux"))]
    window.on_config_open_requested({
        let window = window.as_weak();
        let is_starting = is_starting.clone();
        move || {
            if is_starting.swap(true, Ordering::AcqRel) {
                return;
            }
            let Some(window) = window.upgrade() else {
                is_starting.store(false, Ordering::Release);
                return;
            };
            let current = PathBuf::from(window.get_config_editor_path().as_str());
            window.set_detail_text("Выберите JSON-конфигурацию…".into());
            let weak_window = window.as_weak();
            let is_starting = is_starting.clone();
            std::thread::spawn(move || {
                let dialog = rfd::FileDialog::new();
                let dialog = if let Some(directory) =
                    current.parent().filter(|directory| directory.is_dir())
                {
                    dialog.set_directory(directory)
                } else {
                    dialog
                };
                let selected = dialog
                    .set_title("Открыть JSON-конфигурацию")
                    .add_filter("Конфигурация JSON", &["json", "jsonc"])
                    .pick_file();
                let loaded =
                    selected.map(|path| std::fs::read_to_string(&path).map(|text| (path, text)));
                let _ = slint::invoke_from_event_loop(move || {
                    is_starting.store(false, Ordering::Release);
                    let Some(window) = weak_window.upgrade() else {
                        return;
                    };
                    match loaded {
                        None => window.set_detail_text("Открытие JSON отменено.".into()),
                        Some(Ok((path, text))) => {
                            window
                                .set_config_editor_path(path.to_string_lossy().into_owned().into());
                            window.set_config_editor_text(text.into());
                            window.set_detail_text(
                                format!("Загружен JSON: {}", path.display()).into(),
                            );
                        }
                        Some(Err(problem)) => window.set_detail_text(
                            format!("Не удалось загрузить JSON: {problem}").into(),
                        ),
                    }
                });
            });
        }
    });

    window.on_config_save_requested({
        let window = window.as_weak();
        let is_starting = is_starting.clone();
        move || {
            if is_starting.swap(true, Ordering::AcqRel) {
                return;
            }
            let Some(window) = window.upgrade() else {
                is_starting.store(false, Ordering::Release);
                return;
            };
            let path = PathBuf::from(window.get_config_editor_path().as_str());
            let Some(parent) = path.parent().map(PathBuf::from) else {
                is_starting.store(false, Ordering::Release);
                window.set_detail_text("Укажите корректный путь к JSON-файлу.".into());
                return;
            };
            let text = Zeroizing::new(window.get_config_editor_text().to_string());
            window.set_detail_text("Сохраняю JSON…".into());
            let weak_window = window.as_weak();
            let is_starting = is_starting.clone();
            std::thread::spawn(move || {
                let saved = std::fs::create_dir_all(&parent)
                    .and_then(|()| std::fs::write(&path, text.as_bytes()));
                let _ = slint::invoke_from_event_loop(move || {
                    is_starting.store(false, Ordering::Release);
                    let Some(window) = weak_window.upgrade() else {
                        return;
                    };
                    match saved {
                        Ok(()) => window.set_detail_text(
                            format!(
                                "JSON сохранён: {}. Файл может содержать незашифрованные секреты.",
                                path.display()
                            )
                            .into(),
                        ),
                        Err(problem) => window.set_detail_text(
                            format!("Не удалось сохранить JSON: {problem}").into(),
                        ),
                    }
                });
            });
        }
    });

    #[cfg(any(windows, target_os = "linux"))]
    window.on_config_save_as_requested({
        let window = window.as_weak();
        let is_starting = is_starting.clone();
        move || {
            if is_starting.swap(true, Ordering::AcqRel) {
                return;
            }
            let Some(window) = window.upgrade() else {
                is_starting.store(false, Ordering::Release);
                return;
            };
            let current = PathBuf::from(window.get_config_editor_path().as_str());
            let text = Zeroizing::new(window.get_config_editor_text().to_string());
            window.set_detail_text("Выберите, куда сохранить JSON…".into());
            let weak_window = window.as_weak();
            let is_starting = is_starting.clone();
            std::thread::spawn(move || {
                let dialog = rfd::FileDialog::new();
                let dialog = if let Some(directory) =
                    current.parent().filter(|directory| directory.is_dir())
                {
                    dialog.set_directory(directory)
                } else {
                    dialog
                };
                let file_name = current
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "client.json".into());
                let saved = dialog
                    .set_title("Сохранить JSON-конфигурацию как")
                    .add_filter("Конфигурация JSON", &["json"])
                    .set_file_name(file_name)
                    .save_file()
                    .map(|mut path| {
                    if path.extension().is_none() {
                        path.set_extension("json");
                    }
                    let result = path
                        .parent()
                        .ok_or_else(|| "У выбранного файла нет папки назначения.".to_owned())
                        .and_then(|parent| {
                            std::fs::create_dir_all(parent)
                                .map_err(|problem| problem.to_string())
                        })
                        .and_then(|()| {
                            std::fs::write(&path, text.as_bytes())
                                .map_err(|problem| problem.to_string())
                        });
                        result.map(|()| path)
                    });
                let _ = slint::invoke_from_event_loop(move || {
                    is_starting.store(false, Ordering::Release);
                    let Some(window) = weak_window.upgrade() else {
                        return;
                    };
                    match saved {
                        None => window.set_detail_text("Сохранение JSON отменено.".into()),
                        Some(Ok(path)) => {
                            window.set_config_editor_path(path.to_string_lossy().into_owned().into());
                            window.set_detail_text(
                                format!(
                                    "JSON сохранён: {}. Файл может содержать незашифрованные секреты.",
                                    path.display()
                                )
                                .into(),
                            );
                        }
                        Some(Err(problem)) => window.set_detail_text(
                            format!("Не удалось сохранить JSON как новый файл: {problem}").into(),
                        ),
                    }
                });
            });
        }
    });

    window.on_config_check_requested({
        let window = window.as_weak();
        let is_starting = is_starting.clone();
        move || {
            #[cfg(target_os = "android")]
            {
                if let Some(window) = window.upgrade() {
                    window.set_status_text("Проверка при запуске VPN".into());
                    window.set_detail_text(
                        "Android проверит полный JSON встроенным ядром при запуске VPN; отдельный CLI-проверяющий файл в APK не используется.".into(),
                    );
                }
                return;
            }
            if is_starting.swap(true, Ordering::AcqRel) { return; }
            let Some(window) = window.upgrade() else {
                is_starting.store(false, Ordering::Release);
                return;
            };
            let path = PathBuf::from(window.get_config_editor_path().as_str());
            let Some(parent) = path.parent().map(PathBuf::from) else {
                is_starting.store(false, Ordering::Release);
                window.set_detail_text("Укажите корректный путь к JSON-файлу.".into());
                return;
            };
            let text = Zeroizing::new(window.get_config_editor_text().to_string());
            let weak_window = window.as_weak();
            let is_starting = is_starting.clone();
            window.set_status_text("Проверка JSON…".into());
            window.set_detail_text("Сохраняю файл и передаю его штатному валидатору ядра…".into());
            std::thread::spawn(move || {
                let result = std::fs::create_dir_all(&parent)
                    .and_then(|()| std::fs::write(&path, text.as_bytes()))
                    .map_err(|problem| format!("Сначала не удалось сохранить JSON: {problem}"))
                    .and_then(|()| check_config_file(&path));
                let _ = slint::invoke_from_event_loop(move || {
                    is_starting.store(false, Ordering::Release);
                    let Some(window) = weak_window.upgrade() else { return; };
                    match result {
                        Ok(()) => {
                            window.set_status_text("JSON принят ядром".into());
                            window.set_detail_text("Проверка конфигурации пройдена. Это не проверяет подключение к удалённому серверу.".into());
                        }
                        Err(problem) => {
                            window.set_status_text("Проверка JSON не пройдена".into());
                            window.set_detail_text(problem.into());
                        }
                    }
                });
            });
        }
    });

    window.on_config_reload_requested({
        let window = window.as_weak();
        let is_starting = is_starting.clone();
        let core_session = core_session.clone();
        move || {
            if is_starting.swap(true, Ordering::AcqRel) {
                return;
            }
            let Some(window) = window.upgrade() else {
                is_starting.store(false, Ordering::Release);
                return;
            };
            let path = PathBuf::from(window.get_config_editor_path().as_str());
            let config = Zeroizing::new(window.get_config_editor_text().to_string());
            window.set_status_text("Применение JSON…".into());
            window.set_detail_text(
                "Сверяю сохранённый и проверенный файл, затем применяю изменения без перезапуска ядра…".into(),
            );
            let weak_window = window.as_weak();
            let is_starting = is_starting.clone();
            let core_session = core_session.clone();
            std::thread::spawn(move || {
                let result = (|| {
                    let saved = std::fs::read_to_string(&path)
                        .map_err(|e| format!("Сначала сохраните JSON: {e}"))?;
                    let editor_value: serde_json::Value = serde_json::from_str(&config)
                        .map_err(|e| format!("В редакторе некорректный JSON: {e}"))?;
                    let saved_value: serde_json::Value = serde_json::from_str(&saved)
                        .map_err(|e| format!("Сохранённый JSON некорректен: {e}"))?;
                    if editor_value != saved_value {
                        return Err("Сначала сохраните текущий текст редактора. Конфигурация ещё не применена.".to_owned());
                    }
                    check_config_file(&path)?;
                    let sessions = core_session
                        .lock()
                        .map_err(|_| "Состояние ядра недоступно.".to_owned())?;
                    sessions
                        .as_ref()
                        .ok_or_else(|| "Сначала запустите полный JSON-конфиг без TUN.".to_owned())?
                        .reload_config(&path, &config)
                })();
                let _ = slint::invoke_from_event_loop(move || {
                    is_starting.store(false, Ordering::Release);
                    let Some(window) = weak_window.upgrade() else {
                        return;
                    };
                    match result {
                        Ok(response) => {
                            let notes = serde_json::from_str::<serde_json::Value>(&response)
                                .ok()
                                .and_then(|value| value.get("notes").and_then(serde_json::Value::as_array).cloned())
                                .unwrap_or_default()
                                .into_iter()
                                .filter_map(|note| note.as_str().map(str::to_owned))
                                .collect::<Vec<_>>();
                            window.set_status_text("Конфигурация применена".into());
                            window.set_detail_text(if notes.is_empty() {
                                "Изменения применены к работающему ядру. Чтобы сохранить их для следующего запуска, сохраните JSON отдельно.".into()
                            } else {
                                format!("Изменения применены. После перезапуска вступят в силу: {}", notes.join("; ")).into()
                            });
                        }
                        Err(problem) => {
                            window.set_status_text("Конфигурация не применена".into());
                            window.set_detail_text(redact_sensitive_text(&problem).into());
                        }
                    }
                });
            });
        }
    });

    window.on_recover_proxy_requested({
        let window = window.as_weak();
        let is_starting = is_starting.clone();
        move || {
            if is_starting.swap(true, Ordering::AcqRel) { return; }
            let Some(window) = window.upgrade() else {
                is_starting.store(false, Ordering::Release);
                return;
            };
            window.set_status_text("Восстановление прокси…".into());
            window.set_detail_text("Проверяю резервную копию и возвращаю прежние настройки Windows…".into());
            let weak_window = window.as_weak();
            let is_starting = is_starting.clone();
            std::thread::spawn(move || {
                let result = core::recover_proxy();
                let recovery_left = core::has_proxy_recovery();
                let _ = slint::invoke_from_event_loop(move || {
                    is_starting.store(false, Ordering::Release);
                    let Some(window) = weak_window.upgrade() else { return; };
                    window.set_recovery_visible(recovery_left);
                    match result {
                        Ok(true) => {
                            window.set_status_text("Отключено".into());
                            window.set_detail_text("Прежние настройки системного прокси восстановлены.".into());
                        }
                        Ok(false) => {
                            window.set_status_text("Отключено".into());
                            window.set_detail_text("Текущие настройки системного прокси не принадлежат Reality Client; они не менялись. Резервная копия сохранена для ручной проверки.".into());
                        }
                        Err(problem) => {
                            window.set_status_text("Восстановление не завершено".into());
                            window.set_detail_text(problem.into());
                        }
                    }
                });
            });
        }
    });

    window.on_discard_proxy_recovery_requested({
        let window = window.as_weak();
        let is_starting = is_starting.clone();
        move || {
            if is_starting.swap(true, Ordering::AcqRel) {
                return;
            }
            let Some(window) = window.upgrade() else {
                is_starting.store(false, Ordering::Release);
                return;
            };
            window.set_status_text("Удаление копии…".into());
            window.set_detail_text("Проверяю, что прокси Reality Client уже не активен.".into());
            let weak_window = window.as_weak();
            let is_starting = is_starting.clone();
            std::thread::spawn(move || {
                let result = core::discard_proxy_recovery();
                let recovery_left = core::has_proxy_recovery();
                let _ = slint::invoke_from_event_loop(move || {
                    is_starting.store(false, Ordering::Release);
                    let Some(window) = weak_window.upgrade() else {
                        return;
                    };
                    window.set_recovery_visible(recovery_left);
                    match result {
                        Ok(true) => {
                            window.set_status_text("Отключено".into());
                            window.set_detail_text(
                                "Резервная копия удалена. Настройки системного прокси не менялись."
                                    .into(),
                            );
                        }
                        Ok(false) => {
                            window.set_status_text("Отключено".into());
                            window.set_detail_text("Резервная копия уже отсутствует.".into());
                        }
                        Err(problem) => {
                            window.set_status_text("Копия сохранена".into());
                            window.set_detail_text(problem.into());
                        }
                    }
                });
            });
        }
    });

    window.on_confirm_delete_profile({
        let window = window.as_weak();
        let profile_store = profile_store.clone();
        let selected_profile_link = selected_profile_link.clone();
        move || {
            let Some(window) = window.upgrade() else {
                return;
            };
            let index = window.get_selected_profile_index();
            if index < 0 {
                window.set_detail_text("Сначала выберите профиль для удаления.".into());
                return;
            }
            window.set_detail_text("Удаляю профиль из защищённого хранилища…".into());
            let weak_window = window.as_weak();
            let worker_store = profile_store.clone();
            let worker_link_cache = selected_profile_link.clone();
            std::thread::spawn(move || {
                let result = match worker_store.lock() {
                    Ok(mut guard) => match guard.as_mut() {
                        Some(store) => store.delete(index as usize).map(|()| {
                            let names = store.names();
                            let next = if names.is_empty() {
                                None
                            } else {
                                Some((index as usize).min(names.len() - 1))
                            };
                            let link = next.and_then(|next| store.read_link(next).ok());
                            (names, next, link)
                        }),
                        None => Err("Хранилище профилей недоступно.".into()),
                    },
                    Err(_) => Err("Хранилище профилей недоступно.".into()),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(window) = weak_window.upgrade() else {
                        return;
                    };
                    match result {
                        Ok((names, next_index, link)) => {
                            let next = next_index.map_or(-1, |index| index as i32);
                            window.set_profile_model(ModelRc::from(Rc::new(VecModel::from(
                                names
                                    .iter()
                                    .map(|name| SharedString::from(name.as_str()))
                                    .collect::<Vec<_>>(),
                            ))));
                            window.set_selected_profile_index(next);
                            window.set_profile_count(format!("Профили: {}", names.len()).into());
                            if let Ok(mut cache) = worker_link_cache.lock() {
                                *cache = next_index.zip(
                                    link.as_ref().map(|link| Zeroizing::new(link.to_string())),
                                );
                            }
                            if let Some(link) = link {
                                window.set_profile_name(names[next as usize].as_str().into());
                                window.set_vless_link(link.as_str().into());
                                window.set_detail_text(
                                    "Профиль удалён; следующий профиль выбран.".into(),
                                );
                            } else if next < 0 {
                                window.set_profile_name("".into());
                                window.set_vless_link("".into());
                                window.set_detail_text("Профиль удалён.".into());
                            } else {
                                window.set_vless_link("".into());
                                window.set_detail_text(
                                    "Профиль удалён, но следующий профиль не удалось прочитать."
                                        .into(),
                                );
                            }
                        }
                        Err(problem) => window.set_detail_text(problem.into()),
                    }
                });
            });
        }
    });

    window.on_connect_requested({
        let window = window.as_weak();
        let selected_profile_link = selected_profile_link.clone();
        let profile_store = profile_store.clone();
        let core_session = core_session.clone();
        let core_logs = core_logs.clone();
        let is_starting = is_starting.clone();
        move || {
            let Some(window) = window.upgrade() else { return; };
            if is_starting.load(Ordering::Acquire) { return; }
            #[cfg(target_os = "android")]
            if window.get_connect_button_text() == "Отключить"
                || window.get_connect_button_text() == "Отмена"
            {
                match platform::stop_android_vpn() {
                    Ok(()) => {
                        window.set_status_text("Отключение Android VPN…".into());
                        window.set_connect_button_text("Отключается…".into());
                    }
                    Err(problem) => window.set_detail_text(problem.into()),
                }
                return;
            }
            let mut active = core_session.lock().expect("core-session mutex poisoned");
            if let Some(mut session) = active.take() {
                drop(active);
                is_starting.store(true, Ordering::Release);
                window.set_status_text("Отключение…".into());
                window.set_connect_button_text("Отключение…".into());
                window.set_detail_text("Останавливаю ядро и восстанавливаю настройки клиента…".into());
                let weak_window = window.as_weak();
                let is_starting = is_starting.clone();
                std::thread::spawn(move || {
                    let result = session.stop();
                    let recovery_visible = core::has_proxy_recovery();
                    let _ = slint::invoke_from_event_loop(move || {
                        is_starting.store(false, Ordering::Release);
                        let Some(window) = weak_window.upgrade() else { return; };
                        window.set_recovery_visible(recovery_visible);
                        window.set_config_reload_supported(false);
                        match result {
                            Ok(()) => {
                                window.set_status_text("Отключено".into());
                                window.set_connect_button_text("Подключить".into());
                                window.set_detail_text("Ядро остановлено; локальный временный файл ссылки удалён.".into());
                            }
                            Err(problem) => {
                                window.set_status_text("Нужно восстановление".into());
                                window.set_connect_button_text("Подключить".into());
                                window.set_detail_text(problem.into());
                            }
                        }
                    });
                });
                return;
            }

            let use_full_config = window.get_use_full_config();
            let profile_index = window.get_selected_profile_index();
            let selected = (profile_index >= 0
                && (profile_index as usize) < window.get_profile_model().row_count())
                .then_some(profile_index as usize);
            let profile_name = window.get_profile_name().trim().to_owned();
            let link = if use_full_config {
                None
            } else {
                let editor_link = Zeroizing::new(window.get_vless_link().trim().to_owned());
                if !editor_link.is_empty() {
                    Some(editor_link)
                } else if let Some(index) = selected {
                    match selected_profile_link.lock() {
                        Ok(cache) => match cache.as_ref() {
                            Some((cached_index, link)) if *cached_index == index => {
                                Some(Zeroizing::new(link.to_string()))
                            }
                            _ => {
                                window.set_detail_text("Профиль ещё загружается из защищённого хранилища. Повторите подключение через секунду.".into());
                                return;
                            }
                        },
                        Err(_) => {
                            window.set_detail_text("Кэш выбранного профиля недоступен.".into());
                            return;
                        }
                    }
                } else {
                    window.set_detail_text("Вставьте или выберите VLESS-ссылку для подключения.".into());
                    return;
                }
            };
            let enable_system_proxy = cfg!(windows)
                && window.get_enable_system_proxy()
                && !use_full_config;
            let config_path = PathBuf::from(window.get_config_editor_path().as_str());
            #[cfg(target_os = "android")]
            {
                let remove_profile_secret = link.is_some();
                window.set_status_text("Подготовка подключения…".into());
                window.set_connect_button_text("Запускаю…".into());
                window.set_detail_text("Сохраняю профиль и подготавливаю запрос Android VPN…".into());
                is_starting.store(true, Ordering::Release);
                let weak_window = window.as_weak();
                let is_starting = is_starting.clone();
                let profile_store = profile_store.clone();
                let selected_profile_link = selected_profile_link.clone();
                std::thread::spawn(move || {
                    let saved_profile = if let Some(link) = link.as_ref() {
                        match save_profile_for_connection(&profile_store, &profile_name, link, selected) {
                            Ok(profile) => Some(profile),
                            Err(problem) => {
                                let _ = slint::invoke_from_event_loop(move || {
                                    is_starting.store(false, Ordering::Release);
                                    if let Some(window) = weak_window.upgrade() {
                                        window.set_status_text("Ошибка сохранения профиля".into());
                                        window.set_connect_button_text("Подключить".into());
                                        window.set_detail_text(problem.into());
                                    }
                                });
                                return;
                            }
                        }
                    } else { None };
                    let prepared = if let Some(link) = link.as_ref() {
                        ffi_session::prepare_android_profile(link)
                            .map(|config| (config, platform::app_data_dir()))
                    } else {
                        ffi_session::read_android_full_config(&config_path).map(|config| {
                            let base = config_path.parent().map(PathBuf::from)
                                .ok_or_else(|| "Не удалось определить папку JSON-конфигурации.".to_owned());
                            (config, base)
                        })
                    };
                    let result = match prepared {
                        Ok((config, Ok(base_dir))) => platform::request_android_vpn(&config, &base_dir, remove_profile_secret),
                        Ok((_, Err(problem))) | Err(problem) => Err(problem),
                    };
                    if result.is_err() && remove_profile_secret {
                        ffi_session::erase_android_profile_secret();
                    }
                    let _ = slint::invoke_from_event_loop(move || {
                        is_starting.store(false, Ordering::Release);
                        let Some(window) = weak_window.upgrade() else { return; };
                        if let Some(profile) = saved_profile.as_ref() {
                            apply_saved_profile(&window, profile, &selected_profile_link);
                        }
                        match result {
                            Ok(()) => {
                                window.set_status_text("Ожидание разрешения Android VPN…".into());
                                window.set_connect_button_text("Отмена".into());
                                window.set_detail_text("Подтвердите системный запрос Android. При первом подключении система попросит разрешение VPN.".into());
                            }
                            Err(problem) => {
                                window.set_status_text("Ошибка запуска".into());
                                window.set_connect_button_text("Подключить".into());
                                window.set_detail_text(problem.into());
                            }
                        }
                    });
                });
                return;
            }
            core_logs.lock().expect("core-log mutex poisoned").clear();
            window.set_log_text("".into());
            window.set_status_text("Подключение…".into());
            window.set_detail_text(if use_full_config {
                "Проверяю полный JSON-конфиг и запускаю ядро. Полный конфиг может менять системные маршруты и DNS."
            } else {
                "Проверяю профиль и запускаю локальный SOCKS5-прокси…"
            }.into());
            is_starting.store(true, Ordering::Release);
            window.set_connect_button_text("Запускаю…".into());
            let weak_window = window.as_weak();
            let core_session = core_session.clone();
            let core_logs = core_logs.clone();
            let is_starting = is_starting.clone();
            let profile_store = profile_store.clone();
            let selected_profile_link = selected_profile_link.clone();
            std::thread::spawn(move || {
                let saved_profile = if let Some(link) = link.as_ref() {
                    match save_profile_for_connection(&profile_store, &profile_name, link, selected) {
                        Ok(profile) => Some(profile),
                        Err(problem) => {
                            let _ = slint::invoke_from_event_loop(move || {
                                is_starting.store(false, Ordering::Release);
                                if let Some(window) = weak_window.upgrade() {
                                    window.set_status_text("Ошибка сохранения профиля".into());
                                    window.set_connect_button_text("Подключить".into());
                                    window.set_detail_text(problem.into());
                                }
                            });
                            return;
                        }
                    }
                } else {
                    None
                };
                let result = if let Some(link) = link.as_ref() {
                    CoreSession::start(link, enable_system_proxy, &core_logs)
                        .map(|session| (session, false, enable_system_proxy))
                } else {
                    CoreSession::start_config(&config_path, &core_logs).map(|session| (session, true, false))
                };
                let _ = slint::invoke_from_event_loop(move || {
                    is_starting.store(false, Ordering::Release);
                    let Some(window) = weak_window.upgrade() else { return; };
                    if let Some(profile) = saved_profile.as_ref() {
                        apply_saved_profile(&window, profile, &selected_profile_link);
                    }
                    match result {
                        Ok((session, full_config, system_proxy)) => {
                            window.set_config_reload_supported(session.reload_supported());
                            *core_session.lock().expect("core-session mutex poisoned") = Some(session);
                            window.set_recovery_visible(false);
                            window.set_status_text(if full_config { "Полный конфиг запущен" } else { "Прокси запущен" }.into());
                            window.set_connect_button_text("Отключить".into());
                            let detail = if full_config {
                                if cfg!(target_os = "linux") {
                                    "Полный конфиг работает в закреплённом ядре. Для Linux TUN/auto_route нужны CAP_NET_ADMIN; штатный Stop возвращает маршруты ядра. После аварии с strict_route выполните reality-client --tun-cleanup. Удалённое соединение этим запуском не проверялось."
                                } else {
                                    "Ядро оставалось запущенным после старта. Адреса, маршруты и удалённое соединение по пользовательскому JSON отдельно не проверялись."
                                }
                            } else if system_proxy {
                                "Локальная проверка SOCKS5 прошла; системный прокси Windows включён. Удалённое соединение нужно проверить реальным запросом."
                            } else {
                                "Локальная проверка SOCKS5 прошла на 127.0.0.1:1080. Это подтверждает запуск ядра, но не доступность удалённого сервера."
                            };
                            window.set_detail_text(detail.into());
                        }
                        Err(problem) => {
                            window.set_config_reload_supported(false);
                            window.set_recovery_visible(core::has_proxy_recovery());
                            window.set_status_text("Ошибка запуска".into());
                            window.set_connect_button_text("Подключить".into());
                            window.set_detail_text(problem.into());
                        }
                    }
                });
            });
        }
    });

    window.on_group_selected({
        let window = window.as_weak();
        let selectable_groups = selectable_groups.clone();
        let updating_group_controls = updating_group_controls.clone();
        move |index| {
            if updating_group_controls.load(Ordering::Acquire) {
                return;
            }
            let Some(window) = window.upgrade() else {
                return;
            };
            let groups = selectable_groups
                .lock()
                .expect("selector group mutex poisoned");
            let Some(group) = groups.get(index as usize) else {
                return;
            };
            updating_group_controls.store(true, Ordering::Release);
            window.set_member_model(ModelRc::from(Rc::new(VecModel::from(
                group
                    .members
                    .iter()
                    .cloned()
                    .map(SharedString::from)
                    .collect::<Vec<_>>(),
            ))));
            let member_index = group
                .current
                .as_ref()
                .and_then(|current| group.members.iter().position(|member| member == current))
                .unwrap_or(0);
            window.set_selected_member_index(if group.members.is_empty() {
                -1
            } else {
                member_index as i32
            });
            updating_group_controls.store(false, Ordering::Release);
        }
    });

    window.on_member_selected({
        let window = window.as_weak();
        let selectable_groups = selectable_groups.clone();
        let core_session = core_session.clone();
        let updating_group_controls = updating_group_controls.clone();
        let is_starting = is_starting.clone();
        move |member_index| {
            if updating_group_controls.load(Ordering::Acquire)
                || is_starting.load(Ordering::Acquire)
            {
                return;
            }
            let Some(window) = window.upgrade() else {
                return;
            };
            let group_index = window.get_selected_group_index();
            let (group_tag, member_tag) = {
                let groups = selectable_groups
                    .lock()
                    .expect("selector group mutex poisoned");
                let Some(group) = groups.get(group_index as usize) else {
                    return;
                };
                let Some(member) = group.members.get(member_index as usize) else {
                    return;
                };
                (group.tag.clone(), member.clone())
            };
            if is_starting.swap(true, Ordering::AcqRel) {
                return;
            }
            window.set_detail_text(
                format!("Переключаю группу «{group_tag}» на «{member_tag}»…").into(),
            );
            let weak_window = window.as_weak();
            let core_session = core_session.clone();
            let selectable_groups = selectable_groups.clone();
            let is_starting = is_starting.clone();
            std::thread::spawn(move || {
                let result = core_session
                    .lock()
                    .map_err(|_| "Сессия ядра недоступна.".to_owned())
                    .and_then(|session| {
                        session
                            .as_ref()
                            .ok_or_else(|| "Ядро остановлено.".to_owned())?
                            .select_group_member(&group_tag, &member_tag)
                    });
                let _ = slint::invoke_from_event_loop(move || {
                    is_starting.store(false, Ordering::Release);
                    let Some(window) = weak_window.upgrade() else {
                        return;
                    };
                    match result {
                        Ok(()) => {
                            if let Some(group) = selectable_groups
                                .lock()
                                .expect("selector group mutex poisoned")
                                .get_mut(group_index as usize)
                            {
                                group.current = Some(member_tag.clone());
                            }
                            window.set_detail_text(
                                format!("В группе «{group_tag}» выбран сервер «{member_tag}».")
                                    .into(),
                            );
                        }
                        Err(problem) => window.set_detail_text(
                            format!("Не удалось выбрать сервер: {problem}").into(),
                        ),
                    }
                });
            });
        }
    });

    let runtime_poll_pending = Arc::new(AtomicBool::new(false));
    let runtime_timer = Timer::default();
    runtime_timer.start(TimerMode::Repeated, Duration::from_secs(1), {
        let window = window.as_weak();
        let core_session = core_session.clone();
        let selectable_groups = selectable_groups.clone();
        let updating_group_controls = updating_group_controls.clone();
        let runtime_poll_pending = runtime_poll_pending.clone();
        let is_starting = is_starting.clone();
        move || {
            if is_starting.load(Ordering::Acquire)
                || runtime_poll_pending.swap(true, Ordering::AcqRel)
            {
                return;
            }
            let weak_window = window.clone();
            let core_session = core_session.clone();
            let selectable_groups = selectable_groups.clone();
            let updating_group_controls = updating_group_controls.clone();
            let runtime_poll_pending = runtime_poll_pending.clone();
            let is_starting = is_starting.clone();
            std::thread::spawn(move || {
                let result = match core_session.lock() {
                    Ok(session) => match session.as_ref() {
                        Some(session) => fetch_runtime_snapshot(session).map(Some),
                        None => Ok(None),
                    },
                    Err(_) => Err("Сессия ядра недоступна.".to_owned()),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    runtime_poll_pending.store(false, Ordering::Release);
                    if is_starting.load(Ordering::Acquire) {
                        return;
                    }
                    let Some(window) = weak_window.upgrade() else {
                        return;
                    };
                    let snapshot = match result {
                        Ok(Some(snapshot)) => snapshot,
                        Ok(None) => {
                            if !selectable_groups
                                .lock()
                                .expect("selector group mutex poisoned")
                                .is_empty()
                            {
                                selectable_groups
                                    .lock()
                                    .expect("selector group mutex poisoned")
                                    .clear();
                                updating_group_controls.store(true, Ordering::Release);
                                window.set_group_controls_visible(false);
                                window.set_group_model(ModelRc::from(Rc::new(VecModel::from(
                                    Vec::<SharedString>::new(),
                                ))));
                                window.set_member_model(ModelRc::from(Rc::new(VecModel::from(
                                    Vec::<SharedString>::new(),
                                ))));
                                window.set_selected_group_index(-1);
                                window.set_selected_member_index(-1);
                                updating_group_controls.store(false, Ordering::Release);
                            }
                            window.set_traffic_text("Трафик: —".into());
                            return;
                        }
                        Err(_) => return,
                    };
                    let mut cached = selectable_groups
                        .lock()
                        .expect("selector group mutex poisoned");
                    let selected_tag = cached
                        .get(window.get_selected_group_index().max(0) as usize)
                        .map(|group| group.tag.clone());
                    let selected = selected_tag
                        .as_ref()
                        .and_then(|tag| snapshot.groups.iter().position(|group| &group.tag == tag))
                        .unwrap_or(0);
                    let changed = *cached != snapshot.groups;
                    *cached = snapshot.groups;
                    if changed {
                        updating_group_controls.store(true, Ordering::Release);
                        let model = cached
                            .iter()
                            .map(|group| SharedString::from(group.tag.as_str()))
                            .collect::<Vec<_>>();
                        window.set_group_model(ModelRc::from(Rc::new(VecModel::from(model))));
                        window.set_group_controls_visible(!cached.is_empty());
                        if cached.is_empty() {
                            window.set_selected_group_index(-1);
                            window.set_member_model(ModelRc::from(Rc::new(VecModel::from(Vec::<
                                SharedString,
                            >::new(
                            )))));
                            window.set_selected_member_index(-1);
                        } else {
                            window.set_selected_group_index(selected as i32);
                            let group = &cached[selected];
                            window.set_member_model(ModelRc::from(Rc::new(VecModel::from(
                                group
                                    .members
                                    .iter()
                                    .cloned()
                                    .map(SharedString::from)
                                    .collect::<Vec<_>>(),
                            ))));
                            let member_index = group
                                .current
                                .as_ref()
                                .and_then(|tag| {
                                    group.members.iter().position(|member| member == tag)
                                })
                                .unwrap_or(0);
                            window.set_selected_member_index(if group.members.is_empty() {
                                -1
                            } else {
                                member_index as i32
                            });
                        }
                        updating_group_controls.store(false, Ordering::Release);
                    }
                    drop(cached);
                    window.set_traffic_text(
                        format!(
                            "Передано: ↑ {}  ↓ {}  ·  Соединений: {}",
                            format_bytes(snapshot.uploaded),
                            format_bytes(snapshot.downloaded),
                            snapshot.connections,
                        )
                        .into(),
                    );
                });
            });
        }
    });

    let log_timer = Timer::default();
    log_timer.start(TimerMode::Repeated, Duration::from_millis(250), {
        let window = window.as_weak();
        let core_logs = core_logs.clone();
        move || {
            #[cfg(target_os = "android")]
            if let Some(window) = window.upgrade() {
                match platform::android_vpn_state() {
                    1 => {
                        window.set_status_text("Ожидание разрешения Android VPN…".into());
                        window.set_connect_button_text("Отмена".into());
                    }
                    2 => {
                        window.set_status_text("VPN подключён".into());
                        window.set_connect_button_text("Отключить".into());
                        window.set_detail_text(
                            "Android создал TUN и ядро запущено. Доступность удалённого REALITY-сервера отдельно не проверена.".into(),
                        );
                    }
                    3 if window.get_connect_button_text() != "Подключить" => {
                        window.set_status_text("Ошибка Android VPN".into());
                        window.set_connect_button_text("Подключить".into());
                        window.set_detail_text(platform::take_android_vpn_error().into());
                    }
                    _ if window.get_connect_button_text() == "Отключается…" => {
                        window.set_status_text("Отключено".into());
                        window.set_connect_button_text("Подключить".into());
                        window.set_detail_text("Android VPN остановлен.".into());
                    }
                    _ if window.get_connect_button_text() == "Отмена" => {
                        window.set_status_text("Отключено".into());
                        window.set_connect_button_text("Подключить".into());
                        window.set_detail_text("Запрос Android VPN отменён или отклонён.".into());
                    }
                    _ => {}
                }
            }
            let lines = {
                let Ok(mut queue) = core_logs.lock() else {
                    return;
                };
                queue.drain(..).collect::<Vec<_>>()
            };
            if lines.is_empty() {
                return;
            }
            let Some(window) = window.upgrade() else {
                return;
            };
            let mut visible = window
                .get_log_text()
                .to_string()
                .lines()
                .filter(|line| !line.starts_with("Предупреждения и ошибки ядра появятся здесь."))
                .map(str::to_owned)
                .chain(lines)
                .collect::<VecDeque<_>>();
            while visible.len() > 60 {
                visible.pop_front();
            }
            window.set_log_text(visible.into_iter().collect::<Vec<_>>().join("\n").into());
        }
    });

    #[cfg(any(windows, target_os = "linux"))]
    let _close_timer = {
        let close_timer = Timer::default();
        close_timer.start(TimerMode::Repeated, Duration::from_millis(50), {
            let weak_window = window.as_weak();
            let close_state = close_state.clone();
            let core_session = core_session.clone();
            let is_starting = is_starting.clone();
            move || {
                if close_state.load(Ordering::Acquire) == 1 {
                    if is_starting.load(Ordering::Acquire) {
                        return;
                    }
                    close_state.store(2, Ordering::Release);
                }
                if close_state
                    .compare_exchange(2, 3, Ordering::AcqRel, Ordering::Acquire)
                    .is_err()
                {
                    return;
                }

                let mut slot = match core_session.try_lock() {
                    Ok(slot) => slot,
                    Err(std::sync::TryLockError::WouldBlock) => {
                        close_state.store(2, Ordering::Release);
                        return;
                    }
                    Err(std::sync::TryLockError::Poisoned(poisoned)) => {
                        let slot = poisoned.into_inner();
                        core_session.clear_poison();
                        slot
                    }
                };
                let session = slot.take();
                drop(slot);

                if let Some(mut session) = session {
                    let weak_window = weak_window.clone();
                    let close_state = close_state.clone();
                    let is_starting = is_starting.clone();
                    std::thread::spawn(move || {
                        let result = session.stop();
                        drop(session);
                        let recovery_left = core::has_proxy_recovery();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(window) = weak_window.upgrade() {
                                window.set_recovery_visible(recovery_left);
                                if result.is_ok() && !recovery_left {
                                    let _ = window.hide();
                                    return;
                                }
                                close_state.store(0, Ordering::Release);
                                is_starting.store(false, Ordering::Release);
                                window.set_status_text("Нужно восстановление".into());
                                window.set_detail_text(match result {
                                    Err(problem) => format!(
                                        "Не удалось безопасно завершить клиент: {problem}. Проверьте состояние восстановления прокси."
                                    ).into(),
                                    Ok(()) => "В резервной копии осталось незавершённое восстановление. Завершите его кнопкой «Восстановить прокси», затем закройте приложение.".into(),
                                });
                            }
                        });
                    });
                    return;
                }

                if core::has_proxy_recovery() {
                    if let Some(window) = weak_window.upgrade() {
                        window.set_recovery_visible(true);
                        window.set_status_text("Нужно восстановление".into());
                        window.set_detail_text(
                            "Перед закрытием восстановите системный прокси кнопкой «Восстановить прокси».".into(),
                        );
                    }
                    close_state.store(0, Ordering::Release);
                    is_starting.store(false, Ordering::Release);
                } else if let Some(window) = weak_window.upgrade() {
                    let _ = window.hide();
                }
            }
        });
        close_timer
    };

    window.run()
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["Б", "КБ", "МБ", "ГБ", "ТБ"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

#[cfg(windows)]
fn read_clipboard_profile_link() -> Result<Zeroizing<String>, String> {
    use windows::Win32::{
        Foundation::HGLOBAL,
        System::{
            DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard},
            Memory::{GlobalLock, GlobalSize, GlobalUnlock},
        },
    };

    struct ClipboardGuard;
    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseClipboard();
            }
        }
    }

    struct ClipboardMemoryGuard(HGLOBAL);
    impl Drop for ClipboardMemoryGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = GlobalUnlock(self.0);
            }
        }
    }

    let mut opened = false;
    for _ in 0..8 {
        if unsafe { OpenClipboard(None) }.is_ok() {
            opened = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if !opened {
        return Err("Буфер обмена занят другим приложением. Попробуйте ещё раз.".into());
    }
    let _clipboard_guard = ClipboardGuard;

    const CF_UNICODETEXT: u32 = 13;
    const MAX_CLIPBOARD_BYTES: usize = 1024 * 1024;
    let handle = unsafe { GetClipboardData(CF_UNICODETEXT) }
        .map_err(|_| "В буфере обмена нет текста со ссылкой.".to_owned())?;
    let memory = HGLOBAL(handle.0);
    let size = unsafe { GlobalSize(memory) };
    if !(2..=MAX_CLIPBOARD_BYTES).contains(&size) || size % 2 != 0 {
        return Err("Текст в буфере обмена имеет недопустимый размер.".into());
    }
    let pointer = unsafe { GlobalLock(memory) }.cast::<u16>();
    if pointer.is_null() {
        return Err("Не удалось прочитать текст из буфера обмена.".into());
    }
    let _memory_guard = ClipboardMemoryGuard(memory);
    // SAFETY: the clipboard stays open and the global memory is locked until
    // both guards leave scope; `size` comes from GlobalSize for this handle.
    let utf16 = unsafe { std::slice::from_raw_parts(pointer, size / 2) };
    let Some(end) = utf16.iter().position(|unit| *unit == 0) else {
        return Err("Текст в буфере обмена не завершён корректно.".into());
    };
    let text = String::from_utf16(&utf16[..end])
        .map_err(|_| "Текст в буфере обмена содержит некорректный Unicode.".to_owned())?;
    sanitize_clipboard_profile_link(text)
}

#[cfg(windows)]
fn sanitize_clipboard_profile_link(text: String) -> Result<Zeroizing<String>, String> {
    let text = Zeroizing::new(text);
    if text.contains(['\r', '\n', '\0']) {
        return Err("В буфере несколько строк. Скопируйте только одну ссылку VLESS.".into());
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("В буфере нет текста со ссылкой VLESS.".into());
    }
    Ok(Zeroizing::new(trimmed.to_owned()))
}

#[cfg(all(test, windows))]
mod clipboard_tests {
    use super::sanitize_clipboard_profile_link;

    #[test]
    fn clipboard_paste_trims_outer_whitespace_and_preserves_link() {
        let pasted =
            sanitize_clipboard_profile_link("  vless://secret@example.com:443  ".into()).unwrap();
        assert_eq!(pasted.as_str(), "vless://secret@example.com:443");
    }

    #[test]
    fn clipboard_paste_rejects_multiline_content() {
        let error = sanitize_clipboard_profile_link("vless://one\nvless://two".into()).unwrap_err();
        assert!(error.contains("несколько строк"));
        assert!(!error.contains("vless://"));
    }

    #[test]
    fn clipboard_paste_rejects_empty_text() {
        assert!(sanitize_clipboard_profile_link(" \t ".into()).is_err());
    }
}

#[cfg(all(test, windows))]
mod connect_profile_tests {
    use super::{ProfileStore, Zeroizing, save_profile_for_connection};
    use std::sync::{Arc, Mutex};

    #[test]
    fn connect_profile_save_creates_then_replaces_protected_profile() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "reality-connect-profile-test-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("profiles.dat");
        let store = ProfileStore::open_at(path.clone()).unwrap();
        let shared_store = Arc::new(Mutex::new(Some(store)));

        let initial_link = Zeroizing::new(
            "vless://00000000-0000-4000-8000-000000000000@edge.example.org:443?encryption=none"
                .to_owned(),
        );
        let created = save_profile_for_connection(&shared_store, " ", &initial_link, None).unwrap();
        assert_eq!(created.index, 0);
        assert_eq!(created.name, "Текущий профиль");
        assert_eq!(created.names, ["Текущий профиль"]);

        let edited_link = Zeroizing::new(
            "vless://11111111-1111-4111-8111-111111111111@edge.example.org:8443?encryption=none"
                .to_owned(),
        );
        let updated = save_profile_for_connection(
            &shared_store,
            "Обновлённый",
            &edited_link,
            Some(created.index),
        )
        .unwrap();
        assert_eq!(updated.index, 0);
        assert_eq!(updated.names, ["Обновлённый"]);
        assert_eq!(updated.link.as_str(), edited_link.as_str());

        let reopened = ProfileStore::open_at(path).unwrap();
        assert_eq!(reopened.names(), ["Обновлённый"]);
        assert_eq!(
            reopened.read_link(0).unwrap().as_str(),
            edited_link.as_str()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}

fn fetch_runtime_snapshot(session: &CoreSession) -> Result<RuntimeSnapshot, String> {
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
    })
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub fn android_main(app: slint::android::AndroidApp) {
    platform::initialize_android_data_dir(&app)
        .expect("failed to locate private Android data directory");
    android_bridge::cleanup_stale_profile_secret_if_inactive();
    slint::android::init(app).expect("failed to initialize Slint Android backend");
    run_ui().expect("Reality Client UI failed on Android");
}
