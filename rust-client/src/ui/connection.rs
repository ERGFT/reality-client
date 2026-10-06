use std::{path::PathBuf, sync::atomic::Ordering};

use slint::{ComponentHandle, Model};
use zeroize::Zeroizing;

use super::{
    UiState,
    profile_flow::{apply_saved_profile, save_profile_for_connection},
};
#[cfg(target_os = "android")]
use crate::ffi_session;
#[cfg(target_os = "android")]
use crate::platform;
use crate::{MainWindow, core, ffi_session::CoreSession};

pub(super) fn install(window: &MainWindow, state: &UiState) {
    window.on_recover_proxy_requested({
        let window = window.as_weak();
        let is_starting = state.is_starting.clone();
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
        let is_starting = state.is_starting.clone();
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
        let is_starting = state.is_starting.clone();
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

    window.on_connect_requested({
        let window = window.as_weak();
        let selected_profile_link = state.selected_profile_link.clone();
        let profile_store = state.profile_store.clone();
        #[cfg(not(target_os = "android"))]
        let core_session = state.core_session.clone();
        #[cfg(not(target_os = "android"))]
        let core_logs = state.core_logs.clone();
        let is_starting = state.is_starting.clone();
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
}
