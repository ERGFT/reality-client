//! Background subscription operations; the running core is never stopped or
//! reloaded here. Profile indexes are reconciled using stable server IDs.

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::{
    rc::Rc,
    sync::atomic::Ordering,
    time::{SystemTime, UNIX_EPOCH},
};
use zeroize::Zeroizing;

use super::UiState;
use crate::MainWindow;

#[cfg(all(test, windows))]
mod tests;

#[cfg(all(target_os = "android", feature = "subscription-device-check"))]
pub(super) mod device_check;

#[derive(Clone, Copy)]
enum Action {
    Add,
    Refresh,
    Rename,
    Delete,
}

fn model(names: Vec<String>) -> ModelRc<SharedString> {
    ModelRc::from(Rc::new(VecModel::from(
        names
            .into_iter()
            .map(SharedString::from)
            .collect::<Vec<_>>(),
    )))
}

fn sync_groups(window: &MainWindow, state: &UiState) {
    let Ok(guard) = state.profile_store.lock() else {
        return;
    };
    let Some(store) = guard.as_ref() else {
        return;
    };
    if let Some(error) = &store.subscription_error {
        window.set_subscription_status(error.as_str().into());
        return;
    }
    let names = store
        .subscriptions
        .as_ref()
        .map(|subscriptions| {
            subscriptions
                .groups()
                .iter()
                .map(|group| format!("{} · {} серверов", group.name, group.servers.len()))
                .collect()
        })
        .unwrap_or_default();
    window.set_subscription_model(model(names));
}

pub(super) fn install(window: &MainWindow, state: &UiState) {
    sync_groups(window, state);
    window.on_subscription_selected({
        let weak = window.as_weak();
        let store = state.profile_store.clone();
        move |index| {
            let Some(window) = weak.upgrade() else {
                return;
            };
            let Ok(guard) = store.lock() else {
                return;
            };
            let Some(group) = guard
                .as_ref()
                .and_then(|store| store.subscriptions.as_ref())
                .and_then(|store| store.groups().get(index as usize))
            else {
                return;
            };
            window.set_subscription_name(group.name.as_str().into());
            let ago = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                .saturating_sub(group.last_success);
            let status = format!(
                "Серверов: {}. Успешное обновление: {} сек. назад.{}",
                group.servers.len(),
                ago,
                group
                    .last_error
                    .as_ref()
                    .map(|error| format!("\nПоследняя ошибка: {error}"))
                    .unwrap_or_default()
            );
            window.set_subscription_status(status.into());
        }
    });
    for action in [Action::Add, Action::Refresh, Action::Rename, Action::Delete] {
        let weak = window.as_weak();
        let state = state.clone();
        let callback = move || {
            let Some(window) = weak.upgrade() else {
                return;
            };
            begin(&window, &state, action);
        };
        match action {
            Action::Add => window.on_subscription_add(callback),
            Action::Refresh => window.on_subscription_refresh(callback),
            Action::Rename => window.on_subscription_rename(callback),
            Action::Delete => window.on_subscription_delete(callback),
        }
    }
    #[cfg(any(windows, target_os = "linux", target_os = "android"))]
    window.on_subscription_paste({
        let weak = window.as_weak();
        move || {
            let weak = weak.clone();
            std::thread::spawn(move || {
                let result = crate::clipboard::read_clipboard_profile_link().and_then(|link| {
                    crate::subscriptions::validate_url(&link)?;
                    Ok(link)
                });
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(window) = weak.upgrade() else {
                        return;
                    };
                    match result {
                        Ok(link) => window.set_subscription_url(link.as_str().into()),
                        Err(error) => window.set_subscription_status(error.into()),
                    }
                });
            });
        }
    });
}

fn begin(window: &MainWindow, state: &UiState, action: Action) {
    if window.get_subscription_busy() {
        return;
    }
    if state.is_starting.load(Ordering::Acquire) {
        window.set_subscription_status("Дождитесь завершения операции подключения.".into());
        return;
    }
    let index = window.get_selected_subscription_index();
    let id = if matches!(action, Action::Add) {
        None
    } else {
        state.profile_store.lock().ok().and_then(|guard| {
            guard
                .as_ref()
                .and_then(|store| store.subscriptions.as_ref())
                .and_then(|store| store.groups().get(index as usize))
                .map(|group| group.id)
        })
    };
    if !matches!(action, Action::Add) && id.is_none() {
        window.set_subscription_status("Выберите подписку.".into());
        return;
    }
    let name = window.get_subscription_name().trim().to_owned();
    let url = Zeroizing::new(window.get_subscription_url().trim().to_owned());
    if matches!(action, Action::Add | Action::Rename)
        && let Err(error) = crate::subscription_store::validate_name(&name)
    {
        window.set_subscription_status(error.into());
        return;
    }
    if matches!(action, Action::Add)
        && let Err(error) = crate::subscriptions::validate_url(&url)
    {
        window.set_subscription_status(error.into());
        return;
    }
    window.set_subscription_busy(true);
    window.set_subscription_status("Обрабатываю подписку…".into());
    let weak = window.as_weak();
    let state = state.clone();
    std::thread::spawn(move || {
        let loaded =
            if matches!(action, Action::Add | Action::Refresh) {
                let url = if matches!(action, Action::Refresh) {
                    state
                        .profile_store
                        .lock()
                        .map_err(|_| "Хранилище недоступно.".to_owned())
                        .and_then(|guard| {
                            guard
                                .as_ref()
                                .and_then(|store| store.subscriptions.as_ref())
                                .ok_or("Подписки недоступны.".to_owned())?
                                .url(id.ok_or("Подписка не найдена.".to_owned())?)
                        })
                } else {
                    Ok(url)
                };
                Some(url.and_then(|url| {
                    crate::subscriptions::download(&url).map(|result| (url, result))
                }))
            } else {
                None
            };
        let _ = slint::invoke_from_event_loop(move || {
            let Some(window) = weak.upgrade() else {
                return;
            };
            // Acquire on the UI thread, where connection commands also acquire
            // their flag. A worker must not race their check-then-start sequence.
            if state
                .is_starting
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                window.set_import_busy(false);
                window.set_subscription_busy(false);
                window.set_import_status("Подключение изменилось во время загрузки. Повторите операцию после его завершения.".into());
                window.set_subscription_status(
                    "Подключение изменяется. Повторите операцию подписки после его завершения."
                        .into(),
                );
                return;
            }
            commit_in_background(&window, state, action, id, name, loaded);
        });
    });
}

type Loaded = Option<Result<(Zeroizing<String>, crate::subscriptions::ImportResult), String>>;

fn commit_in_background(
    window: &MainWindow,
    state: UiState,
    action: Action,
    id: Option<u64>,
    name: String,
    loaded: Loaded,
) {
    let weak = window.as_weak();
    std::thread::spawn(move || {
        let result = apply(&state, action, id, &name, loaded);
        let operation_flag = state.is_starting.clone();
        let dispatched = slint::invoke_from_event_loop(move || {
            if let Some(window) = weak.upgrade() {
                window.set_import_busy(false);
                window.set_subscription_busy(false);
                match result {
                    Ok((group_id, old_identities, old_manual_count, summary)) => {
                        if matches!(action, Action::Add) {
                            window.set_import_visible(false);
                            window.set_import_edit_index(-1);
                            window.set_import_edit_revision("".into());
                            window.set_import_link("".into());
                            window.set_import_name("".into());
                            window.set_import_show_link(false);
                        }
                        reconcile_selection(&window, &state, &old_identities, old_manual_count);
                        sync_groups(&window, &state);
                        let next = state.profile_store.lock().ok().and_then(|guard| {
                            guard
                                .as_ref()
                                .and_then(|store| store.subscriptions.as_ref())
                                .and_then(|store| {
                                    store
                                        .groups()
                                        .iter()
                                        .position(|group| Some(group.id) == group_id)
                                })
                        });
                        window
                            .set_selected_subscription_index(next.map_or(-1, |index| index as i32));
                        window.set_subscription_url("".into());
                        window.set_subscription_status(summary.into());
                    }
                    Err(error) => {
                        window.set_import_status(error.as_str().into());
                        window.set_subscription_status(error.into());
                    }
                }
            }
            state.is_starting.store(false, Ordering::Release);
        });
        if dispatched.is_err() {
            operation_flag.store(false, Ordering::Release);
        }
    });
}

type Applied = (Option<u64>, Vec<Option<(u64, u64)>>, usize, String);

fn apply(
    state: &UiState,
    action: Action,
    id: Option<u64>,
    name: &str,
    loaded: Loaded,
) -> Result<Applied, String> {
    let mut guard = state
        .profile_store
        .lock()
        .map_err(|_| "Хранилище недоступно.".to_owned())?;
    let store = guard.as_mut().ok_or("Хранилище недоступно.")?;
    let identities: Vec<_> = (0..store.len())
        .map(|index| store.subscription_identity(index))
        .collect();
    let manual_count = identities
        .iter()
        .take_while(|identity| identity.is_none())
        .count();
    let subscriptions = store
        .subscriptions
        .as_mut()
        .ok_or("Хранилище подписок недоступно.")?;
    match action {
        Action::Add | Action::Refresh => {
            let loaded = loaded.ok_or("Ответ подписки отсутствует.")?;
            let (url, imported) = match loaded {
                Ok(result) => result,
                Err(error) => {
                    if let Some(id) = id {
                        subscriptions.record_failure(id, &error)?;
                    }
                    return Err(error);
                }
            };
            let name = if matches!(action, Action::Refresh) {
                subscriptions
                    .groups()
                    .iter()
                    .find(|group| Some(group.id) == id)
                    .ok_or("Подписка не найдена.")?
                    .name
                    .clone()
            } else {
                name.to_owned()
            };
            let target = if matches!(action, Action::Add) {
                None
            } else {
                id
            };
            let group = match subscriptions.replace(target, &name, &url, &imported) {
                Ok(group) => group,
                Err(error) => {
                    if let Some(id) = target {
                        subscriptions.record_failure(id, &error)?;
                    }
                    return Err(error);
                }
            };
            store.revision = store.revision.wrapping_add(1);
            Ok((
                Some(group),
                identities,
                manual_count,
                format!("Подписка сохранена. {}", imported.summary()),
            ))
        }
        Action::Rename => {
            subscriptions.rename(id.ok_or("Подписка не найдена.")?, name)?;
            store.revision = store.revision.wrapping_add(1);
            Ok((
                id,
                identities,
                manual_count,
                "Подписка переименована.".into(),
            ))
        }
        Action::Delete => {
            subscriptions.delete(id.ok_or("Подписка не найдена.")?)?;
            store.revision = store.revision.wrapping_add(1);
            Ok((
                None,
                identities,
                manual_count,
                "Подписка и её серверы удалены. Действующее подключение не изменено.".into(),
            ))
        }
    }
}

fn reconcile_selection(
    window: &MainWindow,
    state: &UiState,
    previous: &[Option<(u64, u64)>],
    manual_count: usize,
) {
    let Ok(guard) = state.profile_store.lock() else {
        return;
    };
    let Some(store) = guard.as_ref() else {
        return;
    };
    let selected = window.get_selected_profile_index();
    let next = if selected < 0 {
        None
    } else if (selected as usize) < manual_count {
        Some(selected as usize)
    } else {
        previous
            .get(selected as usize)
            .copied()
            .flatten()
            .and_then(|identity| store.index_for_identity(identity))
    };
    let names = store.names();
    drop(guard);
    window.set_profile_model(model(names.clone()));
    window.set_profile_count(format!("Профили: {}", names.len()).into());
    window.set_selected_profile_index(next.map_or(-1, |index| index as i32));
    if selected < 0 {
        // A manual profile draft is independent of subscription changes.
        return;
    }
    if let Some(index) = next {
        window.invoke_profile_selected(index as i32);
    } else {
        if let Ok(mut cache) = state.selected_profile_link.lock() {
            *cache = None;
        }
        window.set_profile_name("".into());
        window.set_vless_link("".into());
        window.set_detail_text(
            "Выберите сервер. Активное соединение продолжает работать независимо от списка.".into(),
        );
    }
}
