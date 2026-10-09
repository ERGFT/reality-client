use std::{rc::Rc, sync::atomic::Ordering};

use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use zeroize::Zeroizing;

use super::{UiState, profile_flow::profile_mutation_blocked};
#[cfg(any(windows, target_os = "android", target_os = "linux"))]
use crate::clipboard::read_clipboard_profile_link;
#[cfg(target_os = "android")]
use crate::platform;
use crate::{
    MainWindow,
    profiles::storage_backend_description,
    server_info::{server_endpoint, server_ip_initial},
};

pub(super) fn install(window: &MainWindow, state: &UiState) {
    window.on_save_profile({
        let window = window.as_weak();
        let profile_store = state.profile_store.clone();
        let selected_profile_link = state.selected_profile_link.clone();
        let is_starting = state.is_starting.clone();
        move || {
            let Some(window) = window.upgrade() else {
                return;
            };
            if is_starting
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                window.set_detail_text("Дождитесь завершения текущей операции.".into());
                return;
            }
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
            let is_starting = is_starting.clone();
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
                let operation_flag = is_starting.clone();
                let dispatched = slint::invoke_from_event_loop(move || {
                    is_starting.store(false, Ordering::Release);
                    let Some(window) = weak_window.upgrade() else {
                        return;
                    };
                    window.set_import_busy(false);
                    match result {
                        Ok((index, names, count, profile_name, link)) => {
                            window.set_import_visible(false);
                            window.set_import_edit_index(-1);
                            window.set_import_edit_revision("".into());
                            window.set_import_link("".into());
                            window.set_import_name("".into());
                            window.set_import_show_link(false);
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
                        Err(problem) => {
                            window.set_import_status(problem.as_str().into());
                            window.set_detail_text(problem.into());
                        }
                    }
                });
                if dispatched.is_err() {
                    operation_flag.store(false, Ordering::Release);
                }
            });
        }
    });

    #[cfg(any(windows, target_os = "android", target_os = "linux"))]
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
        let profile_store = state.profile_store.clone();
        let selected_profile_link = state.selected_profile_link.clone();
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
                            .map(|link| (link, store.names(), store.revision)),
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
                        Ok((link, names, revision)) => {
                            // A subscription refresh can replace/reorder the
                            // list while this worker is reading. Never apply
                            // a stale secret to the same numeric index.
                            let current_revision = profile_store
                                .try_lock()
                                .ok()
                                .and_then(|guard| guard.as_ref().map(|store| store.revision));
                            if current_revision != Some(revision) {
                                window.invoke_profile_selected(index);
                                return;
                            }
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

    window.on_confirm_delete_profile({
        let window = window.as_weak();
        let profile_store = state.profile_store.clone();
        let selected_profile_link = state.selected_profile_link.clone();
        #[cfg(not(target_os = "android"))]
        let core_session = state.core_session.clone();
        let is_starting = state.is_starting.clone();
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
}
