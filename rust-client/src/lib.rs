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
#[cfg(any(windows, target_os = "linux"))]
use slint::CloseRequestResponse;
use slint::{ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel};
use std::net::ToSocketAddrs;
use std::path::PathBuf;
use std::time::{Duration, Instant};
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
    connection_rows: Vec<String>,
}

struct SavedProfile {
    index: usize,
    names: Vec<String>,
    name: String,
    link: Zeroizing<String>,
}

type SelectedProfileLink = Arc<Mutex<Option<(usize, Zeroizing<String>)>>>;

fn initial_profile_index(profile_count: usize) -> Option<usize> {
    (profile_count > 0).then_some(0)
}

fn profile_mutation_blocked(operation_busy: bool, session_active: bool) -> bool {
    operation_busy || session_active
}

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
    window.set_server_endpoint_text(server_endpoint(profile.link.as_str()).into());
    window.set_server_ip_text(server_ip_initial(profile.link.as_str()).into());
    if let Ok(mut cache) = selected_profile_link.lock() {
        *cache = Some((profile.index, Zeroizing::new(profile.link.to_string())));
    }
}

fn server_endpoint(link: &str) -> String {
    let Ok(url) = url::Url::parse(link) else {
        return "Некорректная ссылка".to_owned();
    };
    let Some(host) = url.host_str() else {
        return "Адрес не указан".to_owned();
    };
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    }
}

fn server_ip_initial(link: &str) -> String {
    let Ok(url) = url::Url::parse(link) else {
        return "Определите IP узла".to_owned();
    };
    url.host_str()
        .and_then(|host| {
            host.trim_start_matches('[')
                .trim_end_matches(']')
                .parse::<std::net::IpAddr>()
                .ok()
        })
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "Определите IP узла".to_owned())
}

fn server_socket_target(link: &str) -> Result<(String, u16), String> {
    let url = url::Url::parse(link)
        .map_err(|_| "В ссылке VLESS не удалось прочитать адрес узла.".to_owned())?;
    let host = url
        .host_str()
        .ok_or_else(|| "В ссылке VLESS не указан адрес узла.".to_owned())?;
    let port = url
        .port()
        .ok_or_else(|| "В ссылке VLESS не указан порт узла.".to_owned())?;
    Ok((
        host.trim_start_matches('[')
            .trim_end_matches(']')
            .to_owned(),
        port,
    ))
}

fn resolve_server_ips(host: &str, port: u16) -> Result<String, String> {
    let addresses = (host, port)
        .to_socket_addrs()
        .map_err(|problem| format!("Системный DNS не смог разрешить адрес узла: {problem}"))?;
    let mut ips = Vec::new();
    for address in addresses {
        let ip = address.ip().to_string();
        if !ips.contains(&ip) {
            ips.push(ip);
        }
        if ips.len() == 4 {
            break;
        }
    }
    if ips.is_empty() {
        return Err("Системный DNS не вернул IP-адрес для узла.".to_owned());
    }
    Ok(ips.join(" · "))
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
    window.set_app_version(env!("CARGO_PKG_VERSION").into());
    #[cfg(target_os = "android")]
    window.set_mobile_layout(true);
    let theme_path = platform::app_data_dir()
        .ok()
        .map(|dir| dir.join("theme.txt"));
    if let Some(path) = &theme_path
        && std::fs::read_to_string(path).is_ok_and(|saved| saved.trim() == "light")
    {
        window.set_theme_index(1);
        window.set_dark_theme(false);
    }
    let profile_store = Arc::new(Mutex::new(ProfileStore::open_default().ok()));
    let selected_profile_link = Arc::new(Mutex::new(None::<(usize, Zeroizing<String>)>));
    let core_session: Arc<Mutex<Option<CoreSession>>> = Arc::new(Mutex::new(None));
    let public_ip_check_busy = Arc::new(AtomicBool::new(false));
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
    window.set_tun_cleanup_supported(cfg!(windows));
    window.set_system_proxy_supported(cfg!(windows));
    window.set_clipboard_paste_supported(cfg!(any(windows, target_os = "android")));
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
                    if let Err(problem) = sync_config_options(&window) {
                        window.set_detail_text(format!("Конфигурация загружена, но параметры не удалось прочитать: {problem}").into());
                    }
                }
            });
        });
    } else {
        window.set_config_editor_text(STARTER_CONFIG.into());
        if let Err(problem) = sync_config_options(&window) {
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

    window.on_theme_selected({
        let window = window.as_weak();
        let theme_path = theme_path.clone();
        move |index| {
            let Some(window) = window.upgrade() else {
                return;
            };
            let index = index.clamp(0, 1);
            window.set_theme_index(index);
            window.set_dark_theme(index == 0);
            if let Some(path) = &theme_path
                && let Some(parent) = path.parent()
                && let Err(problem) = platform::ensure_private_dir(parent).and_then(|()| {
                    std::fs::write(path, if index == 0 { "dark\n" } else { "light\n" })
                        .map_err(|error| error.to_string())
                })
            {
                window
                    .set_detail_text(format!("Тема применена, но не сохранена: {problem}").into());
            }
        }
    });

    window.on_public_ip_check_requested({
        let window = window.as_weak();
        #[cfg(not(target_os = "android"))]
        let core_session = core_session.clone();
        let public_ip_check_busy = public_ip_check_busy.clone();
        move || {
            let Some(window) = window.upgrade() else { return; };
            #[cfg(target_os = "android")]
            let connected = platform::android_vpn_state() == 2;
            #[cfg(not(target_os = "android"))]
            let connected = core_session
                .lock()
                .map(|session| session.is_some())
                .unwrap_or(false);
            if !connected {
                window.set_public_ip_text("Не подключено".into());
                window.set_detail_text("Сначала подключитесь: проверка отправляет запрос к api.ipify.org через локальный прокси ядра.".into());
                return;
            }
            let proxy_uri = if window.get_use_full_config() {
                match local_socks_proxy_uri(window.get_config_editor_text().as_str()) {
                    Ok(Some(proxy)) => proxy,
                    Ok(None) => {
                        window.set_detail_text("В полном конфиге нет локального SOCKS/mixed-inbound. Для проверки IP добавьте такой входящий узел.".into());
                        return;
                    }
                    Err(problem) => {
                        window.set_detail_text(problem.into());
                        return;
                    }
                }
            } else {
                "socks5h://127.0.0.1:1080".to_owned()
            };
            if public_ip_check_busy.swap(true, Ordering::AcqRel) {
                return;
            }
            window.set_public_ip_text("Проверяю…".into());
            window.set_detail_text("Отправляю HTTPS-запрос к api.ipify.org через прокси ядра. Сервис увидит адрес, с которого пришёл запрос.".into());
            let weak_window = window.as_weak();
            let public_ip_check_busy = public_ip_check_busy.clone();
            std::thread::spawn(move || {
                let result = fetch_public_ip_via_proxy(&proxy_uri);
                let _ = slint::invoke_from_event_loop(move || {
                    public_ip_check_busy.store(false, Ordering::Release);
                    let Some(window) = weak_window.upgrade() else { return; };
                    match result {
                        Ok(address) => {
                            window.set_public_ip_text(address.into());
                            window.set_detail_text("Получен внешний IP для HTTPS-запроса, отправленного через локальный SOCKS/mixed-inbound ядра. Маршрутизация полного конфига может направлять этот домен напрямую.".into());
                        }
                        Err(problem) => {
                            window.set_public_ip_text("Не удалось проверить".into());
                            window.set_detail_text(problem.into());
                        }
                    }
                });
            });
        }
    });

    window.on_direct_ip_check_requested({
        let window = window.as_weak();
        let public_ip_check_busy = public_ip_check_busy.clone();
        move || {
            let Some(window) = window.upgrade() else { return; };
            if public_ip_check_busy.swap(true, Ordering::AcqRel) {
                return;
            }
            window.set_direct_ip_text("Проверяю…".into());
            window.set_detail_text("Отправляю HTTPS-запрос к api.ipify.org по обычному сетевому маршруту устройства. Если VPN уже подключён, ОС может направить запрос через него; api.ipify.org увидит адрес запроса.".into());
            let weak_window = window.as_weak();
            let public_ip_check_busy = public_ip_check_busy.clone();
            std::thread::spawn(move || {
                let result = fetch_public_ip_direct();
                let _ = slint::invoke_from_event_loop(move || {
                    public_ip_check_busy.store(false, Ordering::Release);
                    let Some(window) = weak_window.upgrade() else { return; };
                    match result {
                        Ok(address) => {
                            window.set_direct_ip_text(address.into());
                            window.set_detail_text("Получен внешний IP для запроса по обычному маршруту устройства. При активном VPN фактический маршрут зависит от настроек ОС.".into());
                        }
                        Err(problem) => {
                            window.set_direct_ip_text("Не удалось проверить".into());
                            window.set_detail_text(problem.into());
                        }
                    }
                });
            });
        }
    });

    window.on_server_ip_check_requested({
        let window = window.as_weak();
        let public_ip_check_busy = public_ip_check_busy.clone();
        move || {
            let Some(window) = window.upgrade() else { return; };
            let (host, port) = match server_socket_target(window.get_vless_link().as_str()) {
                Ok(target) => target,
                Err(problem) => {
                    window.set_server_ip_text("Узел не выбран".into());
                    window.set_detail_text(problem.into());
                    return;
                }
            };
            if public_ip_check_busy.swap(true, Ordering::AcqRel) {
                return;
            }
            window.set_server_ip_text("Определяю…".into());
            window.set_detail_text("Запрашиваю адрес узла через системный DNS. DNS-провайдер может увидеть домен сервера; это не геолокационная проверка.".into());
            let weak_window = window.as_weak();
            let public_ip_check_busy = public_ip_check_busy.clone();
            std::thread::spawn(move || {
                let result = resolve_server_ips(&host, port);
                let _ = slint::invoke_from_event_loop(move || {
                    public_ip_check_busy.store(false, Ordering::Release);
                    let Some(window) = weak_window.upgrade() else { return; };
                    match result {
                        Ok(addresses) => {
                            window.set_server_ip_text(addresses.into());
                            window.set_detail_text("Показаны адреса, возвращённые системным DNS для домена сервера. Фактический адрес соединения может отличаться при балансировке или изменении DNS-записей.".into());
                        }
                        Err(problem) => {
                            window.set_server_ip_text("IP не определён".into());
                            window.set_detail_text(problem.into());
                        }
                    }
                });
            });
        }
    });

    window.on_config_options_sync_requested({
        let window = window.as_weak();
        move || {
            let Some(window) = window.upgrade() else {
                return;
            };
            match sync_config_options(&window) {
                Ok(()) => window.set_detail_text("Параметры и правила перечитаны из JSON.".into()),
                Err(problem) => window.set_detail_text(problem.into()),
            }
        }
    });

    window.on_network_options_save_requested({
        let window = window.as_weak();
        move || {
            let Some(window) = window.upgrade() else { return; };
            let text = window.get_config_editor_text().to_string();
            let result = set_network_options(
                &text,
                window.get_config_tun_enabled(),
                window.get_fakeip_enabled(),
            );
            match result {
                Ok(updated) => {
                    window.set_config_editor_text(updated.into());
                    window.set_use_full_config(true);
                    let _ = sync_config_options(&window);
                    window.set_detail_text(
                        "Параметры записаны в JSON редактора. Проверьте конфиг и переподключитесь, чтобы применить их.".into(),
                    );
                }
                Err(problem) => window.set_detail_text(problem.into()),
            }
        }
    });

    window.on_routing_rule_add_requested({
        let window = window.as_weak();
        move || {
            let Some(window) = window.upgrade() else { return; };
            let text = window.get_config_editor_text().to_string();
            let result = add_routing_rule(
                &text,
                window.get_route_domain_input().as_str(),
                window.get_route_ip_input().as_str(),
                window.get_route_outbound_input().as_str(),
            );
            match result {
                Ok(updated) => {
                    window.set_config_editor_text(updated.into());
                    window.set_route_domain_input("".into());
                    window.set_route_ip_input("".into());
                    let _ = sync_config_options(&window);
                    window.set_use_full_config(true);
                    window.set_detail_text(
                        "Правило записано в JSON. Сетевой уровень применяет его ко всему домену или IP/CIDR, а не к URL-пути.".into(),
                    );
                }
                Err(problem) => window.set_detail_text(problem.into()),
            }
        }
    });

    window.on_routing_rules_clear_requested({
        let window = window.as_weak();
        move || {
            let Some(window) = window.upgrade() else {
                return;
            };
            let text = window.get_config_editor_text().to_string();
            match clear_managed_routing_rules(&text) {
                Ok(updated) => {
                    window.set_config_editor_text(updated.into());
                    let _ = sync_config_options(&window);
                    window.set_detail_text(
                        "Удалены только правила, добавленные из этой формы.".into(),
                    );
                }
                Err(problem) => window.set_detail_text(problem.into()),
            }
        }
    });

    window.on_outbound_selected({
        let window = window.as_weak();
        move |index| {
            let Some(window) = window.upgrade() else {
                return;
            };
            let Some(tag) = usize::try_from(index)
                .ok()
                .and_then(|index| window.get_outbound_model().row_data(index))
            else {
                return;
            };
            window.set_route_outbound_input(tag);
        }
    });

    window.on_android_app_filter_save_requested({
        let window = window.as_weak();
        move || {
            let Some(window) = window.upgrade() else { return; };
            let text = window.get_config_editor_text().to_string();
            match set_android_app_filter(&text, window.get_android_apps_input().as_str()) {
                Ok(updated) => {
                    window.set_config_editor_text(updated.into());
                    window.set_use_full_config(true);
                    window.set_detail_text("Список приложений записан в настройки TUN. Проверьте конфигурацию и переподключитесь.".into());
                }
                Err(problem) => window.set_detail_text(problem.into()),
            }
        }
    });

    window.on_android_app_add_requested({
        let window = window.as_weak();
        move |index| {
            let Some(window) = window.upgrade() else { return; };
            let Some(package) = usize::try_from(index)
                .ok()
                .and_then(|index| android_package_ids.get(index))
            else {
                window.set_detail_text("Выберите приложение из списка Android.".into());
                return;
            };
            let mut packages = split_rule_values(window.get_android_apps_input().as_str());
            if !packages.iter().any(|existing| existing == package) {
                packages.push(package.clone());
            }
            window.set_android_apps_input(packages.join(", ").into());
            window.set_detail_text("Приложение добавлено в список. Нажмите «Сохранить список», чтобы записать его в TUN-конфиг.".into());
        }
    });

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
                            window.set_server_endpoint_text(server_endpoint(&link).into());
                            window.set_server_ip_text(server_ip_initial(&link).into());
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

    #[cfg(any(windows, target_os = "android"))]
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
                            window.set_server_endpoint_text(server_endpoint(&link).into());
                            window.set_server_ip_text(server_ip_initial(&link).into());
                            window.set_detail_text(
                                format!("Профиль получен из {}.", storage_backend_description())
                                    .into(),
                            );
                        }
                        Err(problem) => {
                            window.set_vless_link("".into());
                            window.set_server_endpoint_text("Адрес не определён".into());
                            window.set_server_ip_text("Определите IP узла".into());
                            window.set_detail_text(problem.into());
                        }
                    }
                });
            });
        }
    });

    if let Some(index) = initial_profile_index(window.get_profile_model().row_count()) {
        let index = index as i32;
        window.set_selected_profile_index(index);
        window.invoke_profile_selected(index);
    }

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
                            match sync_config_options(&window) {
                                Ok(()) => window.set_detail_text(
                                    format!("Загружен JSON: {}", path.display()).into(),
                                ),
                                Err(problem) => window.set_detail_text(
                                    format!("JSON загружен, но параметры не прочитаны: {problem}")
                                        .into(),
                                ),
                            }
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
                            match sync_config_options(&window) {
                                Ok(()) => window.set_detail_text(
                                    format!("Загружен JSON: {}", path.display()).into(),
                                ),
                                Err(problem) => window.set_detail_text(
                                    format!("JSON загружен, но параметры не прочитаны: {problem}")
                                        .into(),
                                ),
                            }
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
        #[cfg(not(target_os = "android"))]
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
            #[cfg(not(target_os = "android"))]
            {
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

    #[cfg(windows)]
    window.on_tun_cleanup_requested({
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
            window.set_status_text("Очистка TUN…".into());
            window.set_detail_text(
                "Запускаю штатную очистку только помеченных маршрутов и фильтров Reality Core…"
                    .into(),
            );
            let weak_window = window.as_weak();
            let is_starting = is_starting.clone();
            std::thread::spawn(move || {
                let result = core::cleanup_tun_routes();
                let _ = slint::invoke_from_event_loop(move || {
                    is_starting.store(false, Ordering::Release);
                    let Some(window) = weak_window.upgrade() else {
                        return;
                    };
                    match result {
                        Ok(message) => {
                            window.set_status_text("TUN очищен".into());
                            window.set_detail_text(message.into());
                        }
                        Err(problem) => {
                            window.set_status_text("Очистка не завершена".into());
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
        #[cfg(not(target_os = "android"))]
        let core_session = core_session.clone();
        let is_starting = is_starting.clone();
        move || {
            let Some(window) = window.upgrade() else {
                return;
            };
            if is_starting.swap(true, Ordering::AcqRel) {
                window.set_detail_text("Дождитесь завершения текущей операции.".into());
                return;
            }
            #[cfg(target_os = "android")]
            let session_active = matches!(platform::android_vpn_state(), 1 | 2);
            #[cfg(not(target_os = "android"))]
            let session_active = core_session
                .lock()
                .map(|session| session.is_some())
                .unwrap_or(true);
            if profile_mutation_blocked(false, session_active) {
                is_starting.store(false, Ordering::Release);
                window.set_detail_text("Сначала отключите клиент, затем удаляйте профиль.".into());
                return;
            }
            let index = window.get_selected_profile_index();
            if index < 0 {
                is_starting.store(false, Ordering::Release);
                window.set_detail_text("Сначала выберите профиль для удаления.".into());
                return;
            }
            window.set_detail_text("Удаляю профиль из защищённого хранилища…".into());
            let weak_window = window.as_weak();
            let worker_store = profile_store.clone();
            let worker_link_cache = selected_profile_link.clone();
            let is_starting = is_starting.clone();
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
                    is_starting.store(false, Ordering::Release);
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
                                window.set_server_endpoint_text(server_endpoint(&link).into());
                                window.set_server_ip_text(server_ip_initial(&link).into());
                                window.set_detail_text(
                                    "Профиль удалён; следующий профиль выбран.".into(),
                                );
                            } else if next < 0 {
                                window.set_profile_name("".into());
                                window.set_vless_link("".into());
                                window.set_server_endpoint_text("Выберите сервер".into());
                                window.set_server_ip_text("Определите IP узла".into());
                                window.set_detail_text("Профиль удалён.".into());
                            } else {
                                window.set_vless_link("".into());
                                window.set_server_endpoint_text("Адрес не определён".into());
                                window.set_server_ip_text("Определите IP узла".into());
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
        #[cfg(not(target_os = "android"))]
        let core_session = core_session.clone();
        #[cfg(not(target_os = "android"))]
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
            #[cfg(not(target_os = "android"))]
            {
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
            #[cfg(not(target_os = "android"))]
            {
            let enable_system_proxy = cfg!(windows)
                && window.get_enable_system_proxy()
                && !use_full_config;
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
    let previous_traffic = Arc::new(Mutex::new(None::<(Instant, u64, u64)>));
    let runtime_timer = Timer::default();
    runtime_timer.start(TimerMode::Repeated, Duration::from_secs(1), {
        let window = window.as_weak();
        let core_session = core_session.clone();
        let selectable_groups = selectable_groups.clone();
        let updating_group_controls = updating_group_controls.clone();
        let runtime_poll_pending = runtime_poll_pending.clone();
        let is_starting = is_starting.clone();
        let previous_traffic = previous_traffic.clone();
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
            let previous_traffic = previous_traffic.clone();
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
                            window.set_uploaded_total_text("—".into());
                            window.set_downloaded_total_text("—".into());
                            window.set_active_connection_count_text("—".into());
                            window.set_connection_model(ModelRc::from(Rc::new(VecModel::from(
                                Vec::<SharedString>::new(),
                            ))));
                            window.set_connection_speed_text("↑ —  ↓ —".into());
                            if let Ok(mut previous) = previous_traffic.lock() {
                                *previous = None;
                            }
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
                    let now = Instant::now();
                    let rates = previous_traffic
                        .lock()
                        .ok()
                        .map(|mut previous| {
                            let rates = previous
                                .as_ref()
                                .filter(|(_, up, down)| {
                                    snapshot.uploaded >= *up && snapshot.downloaded >= *down
                                })
                                .map(|(at, up, down)| {
                                    let elapsed = now.duration_since(*at).as_secs_f64().max(0.001);
                                    (
                                        (snapshot.uploaded - up) as f64 / elapsed,
                                        (snapshot.downloaded - down) as f64 / elapsed,
                                    )
                                })
                                .unwrap_or((0.0, 0.0));
                            *previous = Some((now, snapshot.uploaded, snapshot.downloaded));
                            rates
                        })
                        .unwrap_or((0.0, 0.0));
                    window.set_uploaded_total_text(format_bytes(snapshot.uploaded).into());
                    window.set_downloaded_total_text(format_bytes(snapshot.downloaded).into());
                    window
                        .set_active_connection_count_text(snapshot.connections.to_string().into());
                    window.set_connection_model(ModelRc::from(Rc::new(VecModel::from(
                        snapshot
                            .connection_rows
                            .iter()
                            .map(|row| SharedString::from(row.as_str()))
                            .collect::<Vec<_>>(),
                    ))));
                    window.set_connection_speed_text(
                        format!(
                            "↑ {}/с  ↓ {}/с",
                            format_bytes(rates.0.round() as u64),
                            format_bytes(rates.1.round() as u64),
                        )
                        .into(),
                    );
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

const MANAGED_ROUTE_MARKER: &str = "//reality-client-ui";

fn parse_jsonc_value(text: &str) -> Result<serde_json::Value, String> {
    let bytes = text.as_bytes();
    let mut cleaned = Vec::with_capacity(bytes.len());
    let mut index = 0;
    let mut in_string = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            cleaned.push(byte);
            if byte == b'\\' && index + 1 < bytes.len() {
                cleaned.push(bytes[index + 1]);
                index += 2;
                continue;
            }
            if byte == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        match byte {
            b'"' => {
                in_string = true;
                cleaned.push(byte);
                index += 1;
            }
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index += 2;
                while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/')
                {
                    if bytes[index] == b'\n' {
                        cleaned.push(b'\n');
                    }
                    index += 1;
                }
                index = (index + 2).min(bytes.len());
            }
            b',' => {
                let mut lookahead = index + 1;
                while lookahead < bytes.len() && bytes[lookahead].is_ascii_whitespace() {
                    lookahead += 1;
                }
                if !matches!(bytes.get(lookahead), Some(b']') | Some(b'}')) {
                    cleaned.push(byte);
                }
                index += 1;
            }
            _ => {
                cleaned.push(byte);
                index += 1;
            }
        }
    }
    let cleaned = String::from_utf8(cleaned)
        .map_err(|_| "Конфигурация содержит некорректный UTF-8.".to_owned())?;
    serde_json::from_str(&cleaned).map_err(|problem| format!("Некорректный JSON: {problem}"))
}

fn local_socks_proxy_uri(text: &str) -> Result<Option<String>, String> {
    let config = parse_jsonc_value(text)?;
    let Some(inbounds) = config.get("inbounds").and_then(serde_json::Value::as_array) else {
        return Ok(None);
    };
    for inbound in inbounds {
        let inbound_type = inbound
            .get("type")
            .or_else(|| inbound.get("protocol"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if !matches!(inbound_type, "mixed" | "socks") {
            continue;
        }
        let listen = inbound
            .get("listen")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("127.0.0.1");
        let is_loopback = listen == "localhost"
            || listen
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback());
        if !is_loopback {
            continue;
        }
        let port = inbound
            .get("listen_port")
            .or_else(|| inbound.get("port"))
            .and_then(serde_json::Value::as_u64)
            .and_then(|port| u16::try_from(port).ok())
            .filter(|port| *port != 0);
        let Some(port) = port else {
            continue;
        };
        let host = if listen == "localhost" {
            "127.0.0.1"
        } else {
            listen
        };
        let host = if host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_ipv6())
        {
            format!("[{host}]")
        } else {
            host.to_owned()
        };
        return Ok(Some(format!("socks5h://{host}:{port}")));
    }
    Ok(None)
}

fn fetch_public_ip_via_proxy(proxy_uri: &str) -> Result<String, String> {
    let proxy = ureq::Proxy::new(proxy_uri)
        .map_err(|problem| format!("Не удалось настроить локальный SOCKS-прокси: {problem}"))?;
    let agent = ureq::Agent::config_builder()
        .https_only(true)
        .timeout_global(Some(Duration::from_secs(10)))
        .proxy(Some(proxy))
        .build()
        .new_agent();
    let response = agent
        .get("https://api.ipify.org")
        .call()
        .map_err(|problem| {
            format!("Запрос к api.ipify.org через ядро завершился ошибкой: {problem}")
        })?;
    let body = response
        .into_body()
        .with_config()
        .limit(128)
        .read_to_string()
        .map_err(|problem| format!("Не удалось прочитать ответ сервиса проверки IP: {problem}"))?;
    body.trim()
        .parse::<std::net::IpAddr>()
        .map(|address| address.to_string())
        .map_err(|_| "Сервис проверки IP вернул ответ, который не является IP-адресом.".to_owned())
}

fn fetch_public_ip_direct() -> Result<String, String> {
    let agent = ureq::Agent::config_builder()
        .https_only(true)
        .timeout_global(Some(Duration::from_secs(10)))
        .proxy(None)
        .build()
        .new_agent();
    let response = agent
        .get("https://api.ipify.org")
        .call()
        .map_err(|problem| {
            format!("Запрос к api.ipify.org по обычному маршруту завершился ошибкой: {problem}")
        })?;
    let body = response
        .into_body()
        .with_config()
        .limit(128)
        .read_to_string()
        .map_err(|problem| format!("Не удалось прочитать ответ сервиса проверки IP: {problem}"))?;
    body.trim()
        .parse::<std::net::IpAddr>()
        .map(|address| address.to_string())
        .map_err(|_| "Сервис проверки IP вернул ответ, который не является IP-адресом.".to_owned())
}

fn is_xray_config(value: &serde_json::Value) -> bool {
    value
        .get("outbounds")
        .and_then(serde_json::Value::as_array)
        .and_then(|items| items.first())
        .is_some_and(|item| item.get("protocol").is_some() && item.get("type").is_none())
}

fn value_has_tun(value: &serde_json::Value) -> bool {
    value
        .get("inbounds")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|inbound| {
                inbound.get("type").and_then(serde_json::Value::as_str) == Some("tun")
                    || inbound.get("protocol").and_then(serde_json::Value::as_str) == Some("tun")
            })
        })
}

fn value_has_fakeip(value: &serde_json::Value) -> bool {
    let dns = value.get("dns");
    if dns
        .and_then(|dns| dns.get("fakeip"))
        .and_then(|fakeip| fakeip.get("enabled"))
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        return true;
    }
    let has_sing_box_fakeip_server = dns
        .and_then(|dns| dns.get("servers"))
        .and_then(serde_json::Value::as_array)
        .is_some_and(|servers| servers.iter().any(is_sing_box_fakeip_server));
    if has_sing_box_fakeip_server {
        return true;
    }
    value
        .get("fakedns")
        .is_some_and(|pools| pools.as_array().is_none_or(|items| !items.is_empty()))
        && dns
            .and_then(|dns| dns.get("servers"))
            .and_then(serde_json::Value::as_array)
            .is_some_and(|servers| {
                servers.iter().any(|server| {
                    server.as_str() == Some("fakedns")
                        || server.get("address").and_then(serde_json::Value::as_str)
                            == Some("fakedns")
                })
            })
}

fn is_sing_box_fakeip_server(server: &serde_json::Value) -> bool {
    server.get("type").and_then(serde_json::Value::as_str) == Some("fakeip")
        || server.get("address").and_then(serde_json::Value::as_str) == Some("fakeip")
}

fn route_rules_mut(
    value: &mut serde_json::Value,
    xray: bool,
) -> Result<&mut Vec<serde_json::Value>, String> {
    let section = if xray { "routing" } else { "route" };
    let object = value
        .get_mut(section)
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            format!("В конфигурации нет объекта {section}; сначала добавьте его в JSON.")
        })?;
    let rules = object
        .entry("rules")
        .or_insert_with(|| serde_json::json!([]));
    rules
        .as_array_mut()
        .ok_or_else(|| format!("{section}.rules должен быть массивом."))
}

fn sync_config_options(window: &MainWindow) -> Result<(), String> {
    let text = window.get_config_editor_text().to_string();
    let value = parse_jsonc_value(&text)?;
    window.set_config_tun_enabled(value_has_tun(&value));
    window.set_fakeip_enabled(value_has_fakeip(&value));
    window.set_android_apps_input(android_app_filter_values(&value).join(", ").into());
    let xray = is_xray_config(&value);
    let outbound_tags = value
        .get("outbounds")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|outbound| outbound.get("tag").and_then(serde_json::Value::as_str))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let selected_tag = outbound_tags
        .iter()
        .position(|tag| tag == window.get_route_outbound_input().as_str())
        .unwrap_or(0);
    window.set_outbound_model(ModelRc::from(Rc::new(VecModel::from(
        outbound_tags
            .iter()
            .map(|tag| SharedString::from(tag.as_str()))
            .collect::<Vec<_>>(),
    ))));
    window.set_selected_outbound_index(if outbound_tags.is_empty() {
        -1
    } else {
        selected_tag as i32
    });
    window.set_route_outbound_input(
        outbound_tags
            .get(selected_tag)
            .map(String::as_str)
            .unwrap_or("")
            .into(),
    );
    let rules = value
        .get(if xray { "routing" } else { "route" })
        .and_then(|section| section.get("rules"))
        .and_then(serde_json::Value::as_array);
    let managed = rules
        .into_iter()
        .flatten()
        .filter(|rule| rule.get(MANAGED_ROUTE_MARKER).is_some())
        .map(|rule| {
            let domains = if xray {
                rule.get("domain")
            } else {
                rule.get("domain_suffix")
            }
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
            let ips = rule
                .get(if xray { "ip" } else { "ip_cidr" })
                .and_then(serde_json::Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let target = rule
                .get(if xray { "outboundTag" } else { "outbound" })
                .and_then(serde_json::Value::as_str)
                .unwrap_or("?");
            format!(
                "{} → {target}",
                domains
                    .into_iter()
                    .chain(ips)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
        .collect::<Vec<_>>();
    window.set_route_rules_summary(
        if managed.is_empty() {
            "Нет правил, добавленных через эту форму.".to_owned()
        } else {
            managed.join("\n")
        }
        .into(),
    );
    Ok(())
}

fn android_app_filter_values(value: &serde_json::Value) -> Vec<String> {
    value
        .get("inbounds")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .find(|inbound| {
            inbound.get("type").and_then(serde_json::Value::as_str) == Some("tun")
                || inbound.get("protocol").and_then(serde_json::Value::as_str) == Some("tun")
        })
        .and_then(|inbound| inbound.get("include_package"))
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn set_android_app_filter(text: &str, packages_text: &str) -> Result<String, String> {
    if !cfg!(target_os = "android") {
        return Err("Фильтр приложений доступен только в Android-сборке.".into());
    }
    let packages = split_rule_values(packages_text);
    for package in &packages {
        let valid = package.split('.').count() >= 2
            && package.split('.').all(|part| {
                !part.is_empty()
                    && part.as_bytes()[0].is_ascii_alphabetic()
                    && part
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            });
        if !valid {
            return Err(format!("Некорректный Android package ID «{package}»."));
        }
    }
    let mut value = parse_jsonc_value(text)?;
    let tun = value
        .get_mut("inbounds")
        .and_then(serde_json::Value::as_array_mut)
        .and_then(|inbounds| {
            inbounds.iter_mut().find(|inbound| {
                inbound.get("type").and_then(serde_json::Value::as_str) == Some("tun")
                    || inbound.get("protocol").and_then(serde_json::Value::as_str) == Some("tun")
            })
        })
        .ok_or_else(|| "Сначала включите TUN и примените настройки.".to_owned())?;
    let tun = tun
        .as_object_mut()
        .ok_or("Объект TUN должен быть JSON-объектом.")?;
    if packages.is_empty() {
        tun.remove("include_package");
    } else {
        tun.insert("include_package".to_owned(), serde_json::json!(packages));
    }
    serde_json::to_string_pretty(&value)
        .map_err(|problem| format!("Не удалось записать JSON: {problem}"))
}

fn set_network_options(
    text: &str,
    tun_enabled: bool,
    fakeip_enabled: bool,
) -> Result<String, String> {
    if tun_enabled && !cfg!(any(windows, target_os = "linux", target_os = "android")) {
        return Err("TUN в этой сборке недоступен на этой платформе.".into());
    }
    let mut value = parse_jsonc_value(text)?;
    let xray = is_xray_config(&value);
    let inbounds = value
        .as_object_mut()
        .ok_or_else(|| "Корень JSON-конфигурации должен быть объектом.".to_owned())?
        .entry("inbounds")
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .ok_or_else(|| "inbounds должен быть массивом.".to_owned())?;
    let key = if xray { "protocol" } else { "type" };
    if tun_enabled {
        if !inbounds
            .iter()
            .any(|inbound| inbound.get(key).and_then(serde_json::Value::as_str) == Some("tun"))
        {
            let tun = if xray {
                serde_json::json!({
                    "protocol": "tun", "tag": "tun",
                    "address": ["172.19.0.1/30", "fdfe:dcba:9876::1/126"], "mtu": 1500
                })
            } else {
                serde_json::json!({
                    "type": "tun", "tag": "tun",
                    "address": ["172.19.0.1/30", "fdfe:dcba:9876::1/126"], "mtu": 1500
                })
            };
            inbounds.push(tun);
        }
        ensure_dns_for_tun(&mut value, xray)?;
    } else {
        inbounds
            .retain(|inbound| inbound.get(key).and_then(serde_json::Value::as_str) != Some("tun"));
        if inbounds.is_empty() {
            inbounds.push(if xray {
                serde_json::json!({ "protocol": "mixed", "tag": "local", "listen": "127.0.0.1", "port": 1080 })
            } else {
                serde_json::json!({ "type": "mixed", "tag": "local", "listen": "127.0.0.1", "listen_port": 1080 })
            });
        }
    }
    set_fakeip(&mut value, xray, fakeip_enabled)?;
    serde_json::to_string_pretty(&value)
        .map_err(|problem| format!("Не удалось записать JSON: {problem}"))
}

fn default_dns_detour_tag(value: &serde_json::Value) -> Option<String> {
    let outbounds = value.get("outbounds")?.as_array()?;
    let has_tag = |tag: &str| {
        outbounds
            .iter()
            .any(|item| item.get("tag").and_then(serde_json::Value::as_str) == Some(tag))
    };
    value
        .get("route")
        .and_then(|route| route.get("final"))
        .and_then(serde_json::Value::as_str)
        .filter(|tag| has_tag(tag))
        .or_else(|| {
            outbounds
                .iter()
                .find(|item| {
                    matches!(
                        item.get("type").and_then(serde_json::Value::as_str),
                        Some("vless" | "trojan" | "selector" | "urltest")
                    )
                })
                .and_then(|item| item.get("tag"))
                .and_then(serde_json::Value::as_str)
        })
        .or_else(|| {
            outbounds
                .iter()
                .find_map(|item| item.get("tag").and_then(serde_json::Value::as_str))
        })
        .map(str::to_owned)
}

fn ensure_dns_for_tun(value: &mut serde_json::Value, xray: bool) -> Result<(), String> {
    let detour_tag = (!xray).then(|| default_dns_detour_tag(value)).flatten();
    let Some(root) = value.as_object_mut() else {
        return Err("Корень JSON-конфигурации должен быть объектом.".into());
    };
    let dns = root.entry("dns").or_insert_with(|| serde_json::json!({}));
    let dns = dns.as_object_mut().ok_or("dns должен быть объектом.")?;
    if !dns.contains_key("servers")
        || dns
            .get("servers")
            .is_some_and(|servers| servers.as_array().is_some_and(Vec::is_empty))
    {
        if xray {
            dns.insert(
                "servers".to_owned(),
                serde_json::json!(["https://1.1.1.1/dns-query"]),
            );
        } else {
            let detour_tag = detour_tag
                .as_deref()
                .ok_or("Для DNS в TUN нужен выход с непустым тегом в outbounds.")?;
            dns.insert("servers".to_owned(), serde_json::json!([{"type":"https","tag":"remote","server":"1.1.1.1","detour":detour_tag}]));
            dns.entry("final")
                .or_insert_with(|| serde_json::json!("remote"));
        }
    }
    Ok(())
}

fn set_fakeip(value: &mut serde_json::Value, xray: bool, enabled: bool) -> Result<(), String> {
    let detour_tag = default_dns_detour_tag(value);
    let root = value
        .as_object_mut()
        .ok_or_else(|| "Корень JSON-конфигурации должен быть объектом.".to_owned())?;
    if xray {
        if enabled {
            let pools = root.entry("fakedns").or_insert_with(|| {
                serde_json::json!([
                    {"ipPool":"198.18.0.0/15"}, {"ipPool":"fc00::/18"}
                ])
            });
            if pools.is_null() {
                *pools = serde_json::json!([{"ipPool":"198.18.0.0/15"}, {"ipPool":"fc00::/18"}]);
            }
            let dns = root
                .entry("dns")
                .or_insert_with(|| serde_json::json!({"servers":[]}));
            let dns = dns.as_object_mut().ok_or("dns должен быть объектом.")?;
            let servers = dns
                .entry("servers")
                .or_insert_with(|| serde_json::json!([]));
            let servers = servers
                .as_array_mut()
                .ok_or("dns.servers должен быть массивом.")?;
            if !servers.iter().any(|server| {
                server.as_str() == Some("fakedns")
                    || server.get("address").and_then(serde_json::Value::as_str) == Some("fakedns")
            }) {
                servers.push(serde_json::json!("fakedns"));
            }
        } else {
            root.remove("fakedns");
            if let Some(servers) = root
                .get_mut("dns")
                .and_then(|dns| dns.get_mut("servers"))
                .and_then(serde_json::Value::as_array_mut)
            {
                servers.retain(|server| {
                    server.as_str() != Some("fakedns")
                        && server.get("address").and_then(serde_json::Value::as_str)
                            != Some("fakedns")
                });
            }
        }
        return Ok(());
    }
    if enabled {
        let dns = root.entry("dns").or_insert_with(|| serde_json::json!({}));
        let dns = dns.as_object_mut().ok_or("dns должен быть объектом.")?;
        if !dns.contains_key("servers")
            || dns
                .get("servers")
                .is_some_and(|servers| servers.as_array().is_some_and(Vec::is_empty))
        {
            let detour_tag = detour_tag
                .as_deref()
                .ok_or("Для Fake-IP DNS нужен выход с непустым тегом в outbounds.")?;
            dns.insert("servers".to_owned(), serde_json::json!([{"type":"https","tag":"remote","server":"1.1.1.1","detour":detour_tag}]));
            dns.entry("final")
                .or_insert_with(|| serde_json::json!("remote"));
        }
        let servers = dns
            .get_mut("servers")
            .and_then(serde_json::Value::as_array_mut)
            .ok_or("dns.servers должен быть массивом.")?;
        if !servers.iter().any(is_sing_box_fakeip_server) {
            servers.push(serde_json::json!({"type":"fakeip","tag":"reality-client-fakeip"}));
        }
        let fakeip = dns.entry("fakeip").or_insert_with(|| serde_json::json!({}));
        let fakeip = fakeip
            .as_object_mut()
            .ok_or("dns.fakeip должен быть объектом.")?;
        fakeip.insert("enabled".to_owned(), serde_json::json!(true));
        fakeip
            .entry("inet4_range")
            .or_insert_with(|| serde_json::json!("198.18.0.0/15"));
        fakeip
            .entry("inet6_range")
            .or_insert_with(|| serde_json::json!("fc00::/18"));
    } else if let Some(dns) = root
        .get_mut("dns")
        .and_then(serde_json::Value::as_object_mut)
    {
        let removed_tags = dns
            .get("servers")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter(|server| is_sing_box_fakeip_server(server))
            .filter_map(|server| server.get("tag").and_then(serde_json::Value::as_str))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if let Some(servers) = dns
            .get_mut("servers")
            .and_then(serde_json::Value::as_array_mut)
        {
            servers.retain(|server| !is_sing_box_fakeip_server(server));
        }
        if let Some(rules) = dns
            .get_mut("rules")
            .and_then(serde_json::Value::as_array_mut)
        {
            rules.retain(|rule| {
                !rule
                    .get("server")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|tag| removed_tags.iter().any(|removed| removed == tag))
            });
        }
        let final_uses_removed_server = dns
            .get("final")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|tag| removed_tags.iter().any(|removed| removed == tag));
        if final_uses_removed_server {
            let fallback = dns
                .get("servers")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .find_map(|server| server.get("tag").and_then(serde_json::Value::as_str));
            if let Some(fallback) = fallback {
                dns.insert("final".to_owned(), serde_json::json!(fallback));
            } else {
                dns.remove("final");
            }
        }
        if let Some(fakeip) = dns
            .get_mut("fakeip")
            .and_then(serde_json::Value::as_object_mut)
        {
            fakeip.insert("enabled".to_owned(), serde_json::json!(false));
        }
    }
    Ok(())
}

fn split_rule_values(text: &str) -> Vec<String> {
    text.split(|character: char| character.is_whitespace() || character == ',' || character == ';')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

fn valid_ip_cidr(value: &str) -> bool {
    let Some((address, prefix)) = value.split_once('/') else {
        return false;
    };
    let Ok(address) = address.parse::<std::net::IpAddr>() else {
        return false;
    };
    let Ok(prefix) = prefix.parse::<u8>() else {
        return false;
    };
    prefix <= if address.is_ipv4() { 32 } else { 128 }
}

fn valid_domain_suffix(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && !value
            .chars()
            .any(|character| matches!(character, '/' | ':' | '@' | '?' | '#'))
        && value.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && label
                    .as_bytes()
                    .first()
                    .is_some_and(|byte| byte.is_ascii_alphanumeric())
                && label
                    .as_bytes()
                    .last()
                    .is_some_and(|byte| byte.is_ascii_alphanumeric())
        })
}

fn normalize_routing_domain(value: &str) -> Result<String, String> {
    let input = value.trim();
    let candidate = if input.contains("://") {
        input.to_owned()
    } else {
        format!("https://{input}")
    };
    let parsed = url::Url::parse(&candidate)
        .map_err(|_| format!("«{input}» не похоже на домен или URL сайта."))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(format!(
            "«{input}»: укажите обычный HTTP/HTTPS сайт без логина и пароля."
        ));
    }
    let domain = parsed
        .host_str()
        .ok_or_else(|| format!("«{input}» не содержит имени сайта."))?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if !valid_domain_suffix(&domain) {
        return Err(format!("«{input}» не похоже на доменное имя."));
    }
    Ok(domain)
}

fn add_routing_rule(
    text: &str,
    domains_text: &str,
    ips_text: &str,
    outbound: &str,
) -> Result<String, String> {
    let domains = split_rule_values(domains_text)
        .iter()
        .map(|domain| normalize_routing_domain(domain))
        .collect::<Result<Vec<_>, _>>()?;
    let ips = split_rule_values(ips_text);
    if domains.is_empty() && ips.is_empty() {
        return Err("Укажите хотя бы один домен или IP/CIDR.".into());
    }
    if let Some(ip) = ips.iter().find(|ip| !valid_ip_cidr(ip)) {
        return Err(format!("«{ip}» — некорректный IP/CIDR."));
    }
    let outbound = outbound.trim();
    if outbound.is_empty() {
        return Err("Укажите тег выходного узла.".into());
    }
    let mut value = parse_jsonc_value(text)?;
    let xray = is_xray_config(&value);
    let has_outbound = value
        .get("outbounds")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|item| {
                item.get("tag").and_then(serde_json::Value::as_str) == Some(outbound)
                    || (xray
                        && item.get("protocol").and_then(serde_json::Value::as_str)
                            == Some(outbound))
            })
        });
    if !has_outbound {
        return Err(format!("Выход «{outbound}» не найден в outbounds JSON."));
    }
    let mut rule = if xray {
        serde_json::json!({ "type":"field", "outboundTag":outbound })
    } else {
        serde_json::json!({ "action":"route", "outbound":outbound })
    };
    rule[MANAGED_ROUTE_MARKER] = serde_json::json!("v1");
    if xray {
        if !domains.is_empty() {
            rule["domain"] = serde_json::json!(
                domains
                    .iter()
                    .map(|domain| format!("domain:{domain}"))
                    .collect::<Vec<_>>()
            );
        }
        if !ips.is_empty() {
            rule["ip"] = serde_json::json!(ips);
        }
    } else {
        if !domains.is_empty() {
            rule["domain_suffix"] = serde_json::json!(domains);
        }
        if !ips.is_empty() {
            rule["ip_cidr"] = serde_json::json!(ips);
        }
    }
    let rules = route_rules_mut(&mut value, xray)?;
    let insert_at = if xray {
        0
    } else {
        rules
            .iter()
            .take_while(|rule| {
                matches!(
                    rule.get("action").and_then(serde_json::Value::as_str),
                    Some("sniff" | "hijack-dns")
                )
            })
            .count()
    };
    rules.insert(insert_at, rule);
    serde_json::to_string_pretty(&value)
        .map_err(|problem| format!("Не удалось записать JSON: {problem}"))
}

fn clear_managed_routing_rules(text: &str) -> Result<String, String> {
    let mut value = parse_jsonc_value(text)?;
    let xray = is_xray_config(&value);
    let rules = route_rules_mut(&mut value, xray)?;
    rules.retain(|rule| rule.get(MANAGED_ROUTE_MARKER).is_none());
    serde_json::to_string_pretty(&value)
        .map_err(|problem| format!("Не удалось записать JSON: {problem}"))
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

#[cfg(target_os = "android")]
fn read_clipboard_profile_link() -> Result<Zeroizing<String>, String> {
    let text = platform::read_android_clipboard_text()?;
    sanitize_clipboard_profile_link(text)
}

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

#[cfg(test)]
mod network_options_tests {
    use super::{default_dns_detour_tag, set_network_options, value_has_fakeip};

    const DIRECT_ONLY_CONFIG: &str = r#"{
        "inbounds": [{"type":"mixed","tag":"local","listen":"127.0.0.1","listen_port":1080}],
        "outbounds": [{"type":"direct","tag":"direct"}],
        "route": {"final":"direct"}
    }"#;

    #[test]
    fn dns_detour_uses_the_configured_final_outbound() {
        let value: serde_json::Value = serde_json::from_str(DIRECT_ONLY_CONFIG).unwrap();
        assert_eq!(default_dns_detour_tag(&value).as_deref(), Some("direct"));
    }

    #[test]
    fn fakeip_adds_a_dns_server_with_an_existing_outbound_tag() {
        let configured = set_network_options(DIRECT_ONLY_CONFIG, false, true).unwrap();
        let value: serde_json::Value = serde_json::from_str(&configured).unwrap();
        assert_eq!(
            value["dns"]["servers"][0]["detour"].as_str(),
            Some("direct")
        );
    }

    #[test]
    fn disabling_fakeip_removes_its_server_and_returns_dns_final_to_upstream() {
        let enabled = set_network_options(DIRECT_ONLY_CONFIG, false, true).unwrap();
        let disabled = set_network_options(&enabled, false, false).unwrap();
        let value: serde_json::Value = serde_json::from_str(&disabled).unwrap();
        let servers = value["dns"]["servers"].as_array().unwrap();
        assert!(
            servers
                .iter()
                .all(|server| !super::is_sing_box_fakeip_server(server))
        );
        assert_eq!(value["dns"]["final"].as_str(), Some("remote"));
        assert!(!value_has_fakeip(&value));
    }

    #[test]
    fn tun_dns_uses_a_real_outbound_when_route_final_is_absent() {
        let config = r#"{
            "inbounds": [],
            "outbounds": [{"type":"direct","tag":"internet"}]
        }"#;
        let configured = set_network_options(config, true, false).unwrap();
        let value: serde_json::Value = serde_json::from_str(&configured).unwrap();
        assert_eq!(
            value["dns"]["servers"][0]["detour"].as_str(),
            Some("internet")
        );
    }

    #[cfg(windows)]
    #[test]
    fn pinned_core_accepts_generated_fakeip_config_without_proxy_named_outbound() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let configured = set_network_options(DIRECT_ONLY_CONFIG, true, true).unwrap();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "reality-fakeip-config-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("config.json");
        std::fs::write(&config_path, configured).unwrap();
        let result = crate::core::check_config_file(&config_path);
        let _ = std::fs::remove_dir_all(&dir);
        result.unwrap();
    }
}

#[cfg(test)]
mod routing_domain_tests {
    use super::normalize_routing_domain;

    #[test]
    fn routing_domain_accepts_hostnames_and_website_urls() {
        assert_eq!(normalize_routing_domain("vk.com").unwrap(), "vk.com");
        assert_eq!(
            normalize_routing_domain("https://VK.com/video?clip=1").unwrap(),
            "vk.com"
        );
        assert_eq!(
            normalize_routing_domain("http://news.example.org/").unwrap(),
            "news.example.org"
        );
    }

    #[test]
    fn routing_domain_rejects_credentials_and_non_web_schemes() {
        assert!(normalize_routing_domain("https://user:password@vk.com/").is_err());
        assert!(normalize_routing_domain("ftp://vk.com/").is_err());
        assert!(normalize_routing_domain("*.vk.com").is_err());
    }
}

#[cfg(test)]
mod clipboard_tests {
    use super::{profile_mutation_blocked, sanitize_clipboard_profile_link};

    #[test]
    fn profile_mutation_is_blocked_during_operation_or_live_session() {
        assert!(!profile_mutation_blocked(false, false));
        assert!(profile_mutation_blocked(true, false));
        assert!(profile_mutation_blocked(false, true));
    }

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

#[cfg(test)]
mod profile_startup_tests {
    use super::initial_profile_index;

    #[test]
    fn selects_first_saved_profile_on_startup() {
        assert_eq!(initial_profile_index(1), Some(0));
        assert_eq!(initial_profile_index(100), Some(0));
    }

    #[test]
    fn leaves_selection_empty_when_no_profiles_exist() {
        assert_eq!(initial_profile_index(0), None);
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

fn format_connection_row(connection: &serde_json::Value) -> String {
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

#[cfg(test)]
mod public_ip_proxy_tests {
    use super::local_socks_proxy_uri;

    #[test]
    fn resolves_sing_box_mixed_inbound_to_loopback_socks_uri() {
        let config = r#"{"inbounds":[{"type":"mixed","listen_port":1080}]}"#;
        assert_eq!(
            local_socks_proxy_uri(config).unwrap().as_deref(),
            Some("socks5h://127.0.0.1:1080")
        );
    }

    #[test]
    fn resolves_xray_socks_inbound_and_ipv6_loopback() {
        let config = r#"{"inbounds":[{"protocol":"socks","listen":"::1","port":2080}]}"#;
        assert_eq!(
            local_socks_proxy_uri(config).unwrap().as_deref(),
            Some("socks5h://[::1]:2080")
        );
    }

    #[test]
    fn refuses_remote_and_non_socks_inbounds() {
        let remote = r#"{"inbounds":[{"type":"mixed","listen":"0.0.0.0","listen_port":1080}]}"#;
        let http = r#"{"inbounds":[{"protocol":"http","listen":"127.0.0.1","port":8080}]}"#;
        assert_eq!(local_socks_proxy_uri(remote).unwrap(), None);
        assert_eq!(local_socks_proxy_uri(http).unwrap(), None);
    }

    #[test]
    fn reports_invalid_full_config() {
        assert!(local_socks_proxy_uri("not JSON").is_err());
    }
}

#[cfg(test)]
mod server_address_tests {
    use super::{server_ip_initial, server_socket_target};

    #[test]
    fn extracts_domain_and_port_without_retaining_link_credentials() {
        let link = "vless://user-secret@example.com:443?security=reality";
        assert_eq!(
            server_socket_target(link).unwrap(),
            ("example.com".to_owned(), 443)
        );
        assert_eq!(server_ip_initial(link), "Определите IP узла");
    }

    #[test]
    fn uses_literal_ipv6_address_without_dns_lookup() {
        let link = "vless://user-secret@[2001:db8::1]:8443?security=reality";
        assert_eq!(
            server_socket_target(link).unwrap(),
            ("2001:db8::1".to_owned(), 8443)
        );
        assert_eq!(server_ip_initial(link), "2001:db8::1");
    }
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
