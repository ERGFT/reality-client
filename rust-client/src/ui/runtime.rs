use std::{
    collections::VecDeque,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[cfg(any(windows, target_os = "linux"))]
use slint::CloseRequestResponse;
use slint::{ComponentHandle, ModelRc, SharedString, Timer, TimerMode, VecModel};

use super::UiState;
#[cfg(any(windows, target_os = "linux"))]
use crate::core;
#[cfg(target_os = "android")]
use crate::platform;
use crate::{
    MainWindow,
    runtime_stats::{fetch_runtime_snapshot, format_bytes},
};

/// Таймеры нужно держать живыми, пока работает окно.
pub(super) struct Timers {
    _runtime: Timer,
    _log: Timer,
    #[cfg(any(windows, target_os = "linux"))]
    _close: Timer,
}

pub(super) fn install(window: &MainWindow, state: &UiState) -> Timers {
    #[cfg(any(windows, target_os = "linux"))]
    window.window().on_close_requested({
        let close_state = state.close_state.clone();
        let is_starting = state.is_starting.clone();
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

    let runtime_poll_pending = Arc::new(AtomicBool::new(false));
    let previous_traffic = Arc::new(Mutex::new(None::<(Instant, u64, u64)>));
    let runtime_timer = Timer::default();
    runtime_timer.start(TimerMode::Repeated, Duration::from_secs(1), {
        let window = window.as_weak();
        let core_session = state.core_session.clone();
        let selectable_groups = state.selectable_groups.clone();
        let updating_group_controls = state.updating_group_controls.clone();
        let runtime_poll_pending = runtime_poll_pending.clone();
        let is_starting = state.is_starting.clone();
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
                let (result, network_warning) = match core_session.lock() {
                    Ok(session) => match session.as_ref() {
                        Some(session) => (
                            fetch_runtime_snapshot(session).map(Some),
                            session.network_warning(),
                        ),
                        None => (Ok(None), String::new()),
                    },
                    Err(_) => (Err("Сессия ядра недоступна.".to_owned()), String::new()),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    runtime_poll_pending.store(false, Ordering::Release);
                    if is_starting.load(Ordering::Acquire) {
                        return;
                    }
                    let Some(window) = weak_window.upgrade() else {
                        return;
                    };
                    window.set_network_warning(network_warning.into());
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
        let core_logs = state.core_logs.clone();
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
            let close_state = state.close_state.clone();
            let core_session = state.core_session.clone();
            let is_starting = state.is_starting.clone();
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

    Timers {
        _runtime: runtime_timer,
        _log: log_timer,
        #[cfg(any(windows, target_os = "linux"))]
        _close: _close_timer,
    }
}
