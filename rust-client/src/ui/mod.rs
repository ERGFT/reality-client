//! Окно приложения: общее состояние и подключение обработчиков по темам.
//!
//! Каждый модуль устанавливает обработчики своей области на `MainWindow`:
//! тема, диагностика IP, редактор конфигурации, профили, подключение,
//! группы серверов, таймеры. Общее состояние лежит в `UiState`.

mod appearance;
mod config_editor;
mod connection;
mod diagnostics;
mod groups;
mod profile_flow;
mod profiles;
mod runtime;
mod subscriptions;

use std::{
    collections::VecDeque,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex, atomic::AtomicBool},
};

#[cfg(any(windows, target_os = "linux"))]
use std::sync::atomic::AtomicU8;

use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use zeroize::Zeroizing;

use crate::{
    MainWindow,
    core::{self, default_config_path, load_advanced_config_path},
    ffi_session::CoreSession,
    platform,
    profiles::ProfileStore,
    runtime_stats::SelectableGroup,
};

const STARTER_CONFIG: &str = r#"{
  "inbounds": [{ "type": "mixed", "tag": "local", "listen": "127.0.0.1", "listen_port": 1080 }],
  "outbounds": [
    { "type": "vless", "tag": "proxy", "link_file": "server.txt" },
    { "type": "direct", "tag": "direct" },
    { "type": "block", "tag": "block" }
  ],
  "route": { "final": "proxy" }
}"#;

/// Состояние, общее для обработчиков окна.
#[derive(Clone)]
pub(crate) struct UiState {
    pub profile_store: Arc<Mutex<Option<ProfileStore>>>,
    pub selected_profile_link: profile_flow::SelectedProfileLink,
    pub core_session: Arc<Mutex<Option<CoreSession>>>,
    pub public_ip_check_busy: Arc<AtomicBool>,
    pub core_logs: Arc<Mutex<VecDeque<String>>>,
    pub is_starting: Arc<AtomicBool>,
    pub selectable_groups: Arc<Mutex<Vec<SelectableGroup>>>,
    pub updating_group_controls: Arc<AtomicBool>,
    #[cfg(any(windows, target_os = "linux"))]
    pub close_state: Arc<AtomicU8>,
    pub theme_path: Option<PathBuf>,
    #[cfg(target_os = "android")]
    pub system_palette: Arc<String>,
    pub android_package_ids: Arc<Vec<String>>,
}

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

    // Идентификатор приложения связывает окно с .desktop-файлом (иконка, группировка).
    #[cfg(target_os = "linux")]
    {
        slint::BackendSelector::new().select()?;
        slint::set_xdg_app_id("reality-client")?;
    }
    let window = MainWindow::new()?;
    window.set_app_version(env!("CARGO_PKG_VERSION").into());
    #[cfg(target_os = "android")]
    window.set_mobile_layout(true);
    // Палитра читается один раз; смена светлой/тёмной темы пересчитывает токены.
    #[cfg(target_os = "android")]
    let system_palette: Arc<String> =
        Arc::new(platform::read_android_system_palette().unwrap_or_default());
    let theme_path = platform::app_data_dir()
        .ok()
        .map(|dir| dir.join("theme.txt"));
    if let Some(path) = &theme_path
        && std::fs::read_to_string(path).is_ok_and(|saved| saved.trim() == "light")
    {
        window.set_theme_index(1);
        window.set_dark_theme(false);
    }
    #[cfg(target_os = "android")]
    appearance::apply_material_theme(&window, &system_palette, window.get_dark_theme());
    let profile_store = Arc::new(Mutex::new(ProfileStore::open_default().ok()));
    let selected_profile_link = Arc::new(Mutex::new(None::<(usize, Zeroizing<String>)>));
    let core_session: Arc<Mutex<Option<CoreSession>>> = Arc::new(Mutex::new(None));
    let public_ip_check_busy = Arc::new(AtomicBool::new(false));
    let core_logs = Arc::new(Mutex::new(VecDeque::<String>::new()));
    let is_starting = Arc::new(AtomicBool::new(false));
    #[cfg(any(windows, target_os = "linux"))]
    let close_state = Arc::new(AtomicU8::new(0));

    window.set_recovery_visible(core::has_proxy_recovery());
    window.set_tun_cleanup_supported(cfg!(windows));
    window.set_system_proxy_supported(cfg!(windows));
    window.set_clipboard_paste_supported(cfg!(any(
        windows,
        target_os = "android",
        target_os = "linux"
    )));
    window.set_file_dialog_supported(cfg!(any(windows, target_os = "linux")));
    let platform_guidance = if cfg!(windows) {
        "Windows TUN требует запуска клиента от имени администратора и wintun.dll из официального пакета. При аварийном завершении доступна ручная очистка маршрутов Reality Core."
    } else if cfg!(target_os = "linux") {
        "Linux TUN требует root или CAP_NET_ADMIN; после аварии с strict_route используйте reality-client --tun-cleanup."
    } else if cfg!(target_os = "android") {
        "Android VPN пока экспериментальный и не проверен на устройстве. Фильтр приложений работает через Android VPN API, а доменные/IP правила применяет ядро. Нативные поля маршрутов TUN и strict_route пока не поддерживаются."
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
    #[cfg(any(windows, target_os = "linux", target_os = "android"))]
    window.set_tun_settings_supported(true);
    #[cfg(target_os = "android")]
    window.set_android_app_filter_supported(true);
    #[cfg(target_os = "android")]
    let android_apps = match platform::list_android_launchable_apps() {
        Ok(apps) => apps,
        Err(problem) => {
            window.set_detail_text(problem.into());
            Vec::new()
        }
    };
    #[cfg(not(target_os = "android"))]
    let android_apps = Vec::<(String, String)>::new();
    let android_package_ids = android_apps
        .iter()
        .map(|(package, _)| package.clone())
        .collect::<Vec<_>>();
    window.set_android_app_model(ModelRc::from(Rc::new(VecModel::from(
        android_apps
            .iter()
            .map(|(package, label)| SharedString::from(format!("{label} · {package}")))
            .collect::<Vec<_>>(),
    ))));
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
                    if let Err(problem) = config_editor::sync_config_options(&window) {
                        window.set_detail_text(format!("Конфигурация загружена, но параметры не удалось прочитать: {problem}").into());
                    }
                }
            });
        });
    } else {
        window.set_config_editor_text(STARTER_CONFIG.into());
        if let Err(problem) = config_editor::sync_config_options(&window) {
            window.set_detail_text(problem.into());
        }
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

    let state = UiState {
        profile_store,
        selected_profile_link,
        core_session,
        public_ip_check_busy,
        core_logs,
        is_starting,
        selectable_groups,
        updating_group_controls,
        #[cfg(any(windows, target_os = "linux"))]
        close_state,
        theme_path,
        #[cfg(target_os = "android")]
        system_palette,
        android_package_ids: Arc::new(android_package_ids),
    };

    appearance::install(&window, &state);
    diagnostics::install(&window, &state);
    config_editor::install(&window, &state);
    profiles::install(&window, &state);
    subscriptions::install(&window, &state);
    if let Some(index) = profile_flow::initial_profile_index(window.get_profile_model().row_count())
    {
        let index = index as i32;
        window.set_selected_profile_index(index);
        window.invoke_profile_selected(index);
    }
    connection::install(&window, &state);
    groups::install(&window, &state);
    let _timers = runtime::install(&window, &state);

    #[cfg(all(target_os = "android", feature = "subscription-device-check"))]
    subscriptions::device_check::start(&window, &state);

    window.run()
}
