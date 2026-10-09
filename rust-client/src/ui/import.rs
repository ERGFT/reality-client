//! A single import draft, independent of the currently selected server.
use slint::ComponentHandle;
use zeroize::Zeroizing;

use crate::MainWindow;

enum LinkKind {
    Server(String),
    Subscription,
}

fn clipboard_draft_is_current(current: i32, requested: i32, visible: bool, busy: bool) -> bool {
    current == requested && visible && !busy
}

fn classify(link: &str) -> Result<LinkKind, String> {
    if link.starts_with("https://") {
        crate::subscriptions::validate_url(link)?;
        Ok(LinkKind::Subscription)
    } else if link.starts_with("vless://") {
        crate::profiles::validate_vless_link(link)?;
        let imported = crate::subscriptions::parse(link.as_bytes())?;
        let server = imported
            .servers
            .into_iter()
            .next()
            .ok_or("Некорректные или неподдерживаемые параметры VLESS-ссылки.")?;
        Ok(LinkKind::Server(server.name))
    } else {
        Err("Вставьте ссылку vless:// на сервер или https:// на подписку.".into())
    }
}

pub(super) fn install(window: &MainWindow, state: &super::UiState) {
    window.on_import_submit({
        let weak = window.as_weak();
        let busy = state.is_starting.clone();
        let store = state.profile_store.clone();
        move || {
            let Some(window) = weak.upgrade() else {
                return;
            };
            if busy.load(std::sync::atomic::Ordering::Acquire) || window.get_subscription_busy() {
                window.set_import_status("Дождитесь завершения текущей операции.".into());
                return;
            }
            let link = Zeroizing::new(window.get_import_link().trim().to_owned());
            let name = window.get_import_name().trim().to_owned();
            match classify(&link) {
                Ok(LinkKind::Server(suggested_name)) => {
                    let edit_index = window.get_import_edit_index();
                    if edit_index >= 0 {
                        let valid = store
                            .try_lock()
                            .ok()
                            .and_then(|guard| {
                                guard.as_ref().map(|store| {
                                    store.revision.to_string()
                                        == window.get_import_edit_revision().as_str()
                                        && store.names().get(edit_index as usize).is_some()
                                        && store
                                            .subscription_identity(edit_index as usize)
                                            .is_none()
                                })
                            })
                            .unwrap_or(false);
                        if !valid {
                            window.set_import_status(
                                "Список серверов изменился. Откройте редактирование заново.".into(),
                            );
                            return;
                        }
                    }
                    window.set_selected_profile_index(edit_index);
                    window.set_profile_name(if name.is_empty() {
                        suggested_name.into()
                    } else {
                        name.into()
                    });
                    window.set_vless_link(link.as_str().into());
                    window.set_import_busy(true);
                    window.invoke_save_profile();
                    window.set_import_status("Сохраняю сервер…".into());
                }
                Ok(LinkKind::Subscription) => {
                    if window.get_import_edit_index() >= 0 {
                        window.set_import_status(
                            "Для подписки нажмите «Добавить сервер или подписку».".into(),
                        );
                        return;
                    }
                    window.set_subscription_name(if name.is_empty() {
                        "Подписка".into()
                    } else {
                        name.into()
                    });
                    window.set_subscription_url(link.as_str().into());
                    window.invoke_subscription_add();
                    window.set_import_busy(window.get_subscription_busy());
                    window.set_import_status(window.get_subscription_status());
                }
                Err(error) => window.set_import_status(error.into()),
            }
        }
    });
    window.on_import_edit_selected({
        let weak = window.as_weak();
        let store = state.profile_store.clone();
        let cache = state.selected_profile_link.clone();
        let busy = state.is_starting.clone();
        move || {
            let Some(window) = weak.upgrade() else { return; };
            if busy.load(std::sync::atomic::Ordering::Acquire) || window.get_subscription_busy() { return; }
            let index = window.get_selected_profile_index();
            if index < 0 { return; }
            let metadata = store.try_lock().ok().and_then(|guard| guard.as_ref().and_then(|store| {
                if store.subscription_identity(index as usize).is_some() { return None; }
                store.names().get(index as usize).map(|name| (name.clone(), store.revision))
            }));
            let link = cache.lock().ok().and_then(|cache| cache.as_ref().and_then(|(cached_index, link)|
                (*cached_index == index as usize).then(|| Zeroizing::new(link.to_string()))));
            let (Some((name, revision)), Some(link)) = (metadata, link) else {
                window.set_detail_text("Сервер ещё загружается или принадлежит подписке. Серверы подписки изменяются через обновление группы.".into());
                return;
            };
            window.set_import_edit_index(index);
            window.set_import_generation(window.get_import_generation().wrapping_add(1));
            window.set_import_edit_revision(revision.to_string().into());
            window.set_import_name(name.into());
            window.set_import_link(link.as_str().into());
            window.set_import_status("".into());
            window.set_import_show_link(false);
            window.set_import_visible(true);
        }
    });
    #[cfg(any(windows, target_os = "android", target_os = "linux"))]
    window.on_import_paste({
        let weak = window.as_weak();
        move || {
            let Some(window) = weak.upgrade() else {
                return;
            };
            let generation = window.get_import_generation();
            let weak = weak.clone();
            std::thread::spawn(move || {
                let result = crate::clipboard::read_clipboard_profile_link();
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(window) = weak.upgrade() else {
                        return;
                    };
                    if !clipboard_draft_is_current(
                        window.get_import_generation(),
                        generation,
                        window.get_import_visible(),
                        window.get_import_busy() || window.get_subscription_busy(),
                    ) {
                        return;
                    }
                    match result {
                        Ok(link) => {
                            window.set_import_link(link.as_str().into());
                            window
                                .set_import_status("Ссылка вставлена. Нажмите «Добавить».".into());
                        }
                        Err(error) => window.set_import_status(error.into()),
                    }
                });
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn late_clipboard_result_cannot_fill_another_or_closed_import_draft() {
        assert!(clipboard_draft_is_current(7, 7, true, false));
        assert!(!clipboard_draft_is_current(8, 7, true, false));
        assert!(!clipboard_draft_is_current(7, 7, false, false));
        assert!(!clipboard_draft_is_current(7, 7, true, true));
    }
    #[test]
    fn import_detects_server_and_subscription_without_echoing_secrets() {
        assert!(matches!(
            classify("vless://00000000-0000-4000-8000-000000000000@server.invalid:443"),
            Ok(LinkKind::Server(_))
        ));
        assert!(matches!(
            classify("https://service.invalid/sub/token"),
            Ok(LinkKind::Subscription)
        ));
        let error = classify("http://service.invalid/private-token")
            .err()
            .unwrap();
        assert!(!error.contains("private-token"));
        assert!(classify("https://service.invalid/sub\nsecret").is_err());
        let Ok(LinkKind::Server(name)) =
            classify("vless://00000000-0000-4000-8000-000000000000@server.invalid:443#My%20Server")
        else {
            panic!("valid server rejected")
        };
        assert_eq!(name, "My Server");
    }
}
