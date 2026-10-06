use std::{path::PathBuf, rc::Rc, sync::atomic::Ordering};

use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use zeroize::Zeroizing;

use super::UiState;
use crate::{
    MainWindow,
    config_json::{
        MANAGED_ROUTE_MARKER, add_routing_rule, android_app_filter_values,
        clear_managed_routing_rules, is_xray_config, parse_jsonc_value, set_android_app_filter,
        set_network_options, split_rule_values, value_has_fakeip, value_has_tun,
    },
    core::check_config_file,
    security::redact_sensitive_text,
};

pub(crate) fn sync_config_options(window: &MainWindow) -> Result<(), String> {
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

pub(super) fn install(window: &MainWindow, state: &UiState) {
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
        let android_package_ids = state.android_package_ids.clone();
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

    window.on_config_load_requested({
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
        let is_starting = state.is_starting.clone();
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
        let is_starting = state.is_starting.clone();
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
        let is_starting = state.is_starting.clone();
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
        let is_starting = state.is_starting.clone();
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
        let is_starting = state.is_starting.clone();
        let core_session = state.core_session.clone();
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
}
