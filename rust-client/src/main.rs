slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let window = MainWindow::new()?;

    window.on_save_profile({
        let window = window.as_weak();
        move || {
            let Some(window) = window.upgrade() else {
                return;
            };
            let name = window.get_profile_name().trim().to_owned();
            let link = window.get_vless_link().trim().to_owned();

            let message = if name.is_empty() {
                "Укажите название профиля.".to_owned()
            } else if !link.starts_with("vless://") {
                "Ссылка должна начинаться с vless://.".to_owned()
            } else {
                // This spike intentionally keeps the profile only in memory.
                // Secure platform storage is added in a later migration phase.
                format!(
                    "Профиль «{name}» принят в прототипе. Постоянное сохранение ещё не подключено."
                )
            };

            window.set_detail_text(message.into());
        }
    });

    window.on_connect_requested({
        let window = window.as_weak();
        move || {
            if let Some(window) = window.upgrade() {
                window.set_detail_text(
                    "Запуск ядра ещё не подключён: это UI-прототип, VPN-трафик не идёт.".into(),
                );
            }
        }
    });

    window.run()
}
