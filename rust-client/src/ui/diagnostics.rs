use std::sync::atomic::Ordering;

use slint::ComponentHandle;

use super::UiState;
use crate::{
    MainWindow, platform,
    server_info::{
        fetch_public_ip_direct, fetch_public_ip_via_proxy, local_socks_proxy_uri,
        resolve_server_ips, server_socket_target,
    },
};

pub(super) fn install(window: &MainWindow, state: &UiState) {
    window.on_open_repository_requested({
        let window = window.as_weak();
        move || {
            if let Err(problem) = platform::open_repository()
                && let Some(window) = window.upgrade()
            {
                window.set_detail_text(problem.into());
            }
        }
    });

    window.on_public_ip_check_requested({
        let window = window.as_weak();
        #[cfg(not(target_os = "android"))]
        let core_session = state.core_session.clone();
        let public_ip_check_busy = state.public_ip_check_busy.clone();
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
        let public_ip_check_busy = state.public_ip_check_busy.clone();
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
        let public_ip_check_busy = state.public_ip_check_busy.clone();
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
}
