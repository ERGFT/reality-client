use slint::ComponentHandle;

use super::UiState;
use crate::{MainWindow, platform};
#[cfg(any(target_os = "android", feature = "android-bridge-check"))]
use crate::{Theme, material};

pub(super) fn install(window: &MainWindow, state: &UiState) {
    window.on_theme_selected({
        let window = window.as_weak();
        let theme_path = state.theme_path.clone();
        #[cfg(target_os = "android")]
        let system_palette = state.system_palette.clone();
        move |index| {
            let Some(window) = window.upgrade() else {
                return;
            };
            let index = index.clamp(0, 1);
            window.set_theme_index(index);
            window.set_dark_theme(index == 0);
            #[cfg(target_os = "android")]
            apply_material_theme(&window, &system_palette, index == 0);
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
}

/// Применяет системную палитру Material You к теме интерфейса. Без палитры
/// (Android ниже 12, ошибка чтения) остаётся встроенная тональная тема.
#[cfg(any(target_os = "android", feature = "android-bridge-check"))]
pub(super) fn apply_material_theme(window: &MainWindow, palette_text: &str, dark: bool) {
    let theme = window.global::<Theme>();
    let Some(palette) = material::Palette::parse(palette_text) else {
        theme.set_dynamic(false);
        return;
    };
    let tokens = palette.tokens(dark);
    let color = |argb: u32| slint::Color::from_argb_encoded(argb);
    theme.set_dyn_bg(color(tokens.bg));
    theme.set_dyn_surface(color(tokens.surface));
    theme.set_dyn_surface_2(color(tokens.surface_2));
    theme.set_dyn_surface_3(color(tokens.surface_3));
    theme.set_dyn_outline(color(tokens.outline));
    theme.set_dyn_text(color(tokens.text));
    theme.set_dyn_dim(color(tokens.dim));
    theme.set_dyn_accent(color(tokens.accent));
    theme.set_dyn_accent_soft(color(tokens.accent_soft));
    theme.set_dyn_on_accent(color(tokens.on_accent));
    theme.set_dyn_on_accent_soft(color(tokens.on_accent_soft));
    theme.set_dynamic(true);
}
