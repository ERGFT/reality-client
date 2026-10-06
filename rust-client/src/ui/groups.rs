use std::{rc::Rc, sync::atomic::Ordering};

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use super::UiState;
use crate::MainWindow;

pub(super) fn install(window: &MainWindow, state: &UiState) {
    window.on_group_selected({
        let window = window.as_weak();
        let selectable_groups = state.selectable_groups.clone();
        let updating_group_controls = state.updating_group_controls.clone();
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
        let selectable_groups = state.selectable_groups.clone();
        let core_session = state.core_session.clone();
        let updating_group_controls = state.updating_group_controls.clone();
        let is_starting = state.is_starting.clone();
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
}
