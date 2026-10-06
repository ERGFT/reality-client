//! Динамические цвета Material You (Android 12+).
//!
//! Kotlin-слой читает системную тональную палитру (`system_accent1_*`,
//! `system_neutral1_*`, `system_neutral2_*`) и отдаёт её строкой
//! `ключ=AARRGGBB,…`. Здесь она разбирается и переводится в токены темы
//! интерфейса (`Theme.dyn-*` в `ui/main.slint`) для светлой и тёмной темы.
//! Модуль не зависит от Android и проверяется обычными тестами.

use std::collections::HashMap;

/// Ключи, без которых палитра считается неполной.
const REQUIRED: [&str; 17] = [
    "a1_0", "a1_100", "a1_200", "a1_600", "a1_700", "a1_800", "a1_900", "n1_10", "n1_50", "n1_100",
    "n1_200", "n1_700", "n1_800", "n1_900", "n2_200", "n2_700", "n2_900",
];

pub struct Palette(HashMap<String, u32>);

/// Цвета интерфейса в формате ARGB.
#[derive(Debug, PartialEq, Eq)]
pub struct Tokens {
    pub bg: u32,
    pub surface: u32,
    pub surface_2: u32,
    pub surface_3: u32,
    pub outline: u32,
    pub text: u32,
    pub dim: u32,
    pub accent: u32,
    pub accent_soft: u32,
    pub on_accent: u32,
    pub on_accent_soft: u32,
}

impl Palette {
    /// Разбирает `ключ=AARRGGBB,…`. Неполная или повреждённая палитра — `None`.
    pub fn parse(text: &str) -> Option<Self> {
        let mut map = HashMap::new();
        for part in text.split(',') {
            let (key, value) = part.trim().split_once('=')?;
            if value.len() != 8 {
                return None;
            }
            map.insert(key.to_owned(), u32::from_str_radix(value, 16).ok()?);
        }
        REQUIRED
            .iter()
            .all(|key| map.contains_key(*key))
            .then_some(Self(map))
    }

    fn get(&self, key: &str) -> u32 {
        self.0[key]
    }

    /// Токены по схеме Material 3: основной цвет — акцент 1, поверхности —
    /// нейтральные тона 1, контуры и вторичный текст — нейтральные тона 2.
    pub fn tokens(&self, dark: bool) -> Tokens {
        let c = |key: &str| self.get(key);
        if dark {
            Tokens {
                bg: c("n1_900"),
                surface: lerp(c("n1_900"), c("n1_800"), 0.45),
                surface_2: lerp(c("n1_900"), c("n1_800"), 0.85),
                surface_3: lerp(c("n1_800"), c("n1_700"), 0.35),
                outline: c("n2_700"),
                text: c("n1_100"),
                dim: c("n2_200"),
                accent: c("a1_200"),
                accent_soft: c("a1_700"),
                on_accent: c("a1_800"),
                on_accent_soft: c("a1_100"),
            }
        } else {
            Tokens {
                bg: c("n1_10"),
                surface: lerp(c("n1_50"), c("n1_100"), 0.5),
                surface_2: c("n1_100"),
                surface_3: lerp(c("n1_100"), c("n1_200"), 0.5),
                outline: c("n2_200"),
                text: c("n1_900"),
                dim: c("n2_700"),
                accent: c("a1_600"),
                accent_soft: c("a1_100"),
                on_accent: c("a1_0"),
                on_accent_soft: c("a1_900"),
            }
        }
    }
}

fn lerp(from: u32, to: u32, t: f32) -> u32 {
    let channel = |shift: u32| {
        let a = ((from >> shift) & 0xff) as f32;
        let b = ((to >> shift) & 0xff) as f32;
        (a + (b - a) * t).round().clamp(0.0, 255.0) as u32
    };
    0xff00_0000 | (channel(16) << 16) | (channel(8) << 8) | channel(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "a1_0=ffffffff,a1_100=ffbcf0e6,a1_200=ff80d5c9,a1_600=ff00695e,\
a1_700=ff005046,a1_800=ff003730,a1_900=ff00201c,n1_10=fff4fbf9,n1_50=ffeef5f2,\
n1_100=ffdde4e1,n1_200=ffc1c8c5,n1_700=ff414847,n1_800=ff2c3231,n1_900=ff191c1b,\
n2_200=ffbec9c5,n2_700=ff3f4946,n2_900=ff141d1b";

    #[test]
    fn parses_a_complete_palette() {
        assert!(Palette::parse(SAMPLE).is_some());
    }

    #[test]
    fn rejects_incomplete_or_malformed_palettes() {
        assert!(Palette::parse("").is_none());
        assert!(Palette::parse("a1_0=ffffffff").is_none());
        assert!(Palette::parse(&SAMPLE.replace("n1_900=ff191c1b", "n1_900=zz191c1b")).is_none());
        assert!(Palette::parse(&SAMPLE.replace("a1_100=ffbcf0e6", "a1_100=bcf0e6")).is_none());
        assert!(Palette::parse(&format!("{SAMPLE},broken")).is_none());
    }

    #[test]
    fn dark_theme_uses_light_accent_on_dark_surface() {
        let tokens = Palette::parse(SAMPLE).unwrap().tokens(true);
        assert_eq!(tokens.bg, 0xff19_1c1b);
        assert_eq!(tokens.accent, 0xff80_d5c9);
        assert_eq!(tokens.on_accent, 0xff00_3730);
        assert_eq!(tokens.text, 0xffdd_e4e1);
    }

    #[test]
    fn light_theme_uses_dark_accent_on_light_surface() {
        let tokens = Palette::parse(SAMPLE).unwrap().tokens(false);
        assert_eq!(tokens.bg, 0xfff4_fbf9);
        assert_eq!(tokens.accent, 0xff00_695e);
        assert_eq!(tokens.on_accent, 0xffff_ffff);
        assert_eq!(tokens.text, 0xff19_1c1b);
    }

    #[test]
    fn surfaces_step_towards_the_text_colour() {
        let dark = Palette::parse(SAMPLE).unwrap().tokens(true);
        let luma = |c: u32| ((c >> 16) & 0xff) + ((c >> 8) & 0xff) + (c & 0xff);
        assert!(luma(dark.bg) < luma(dark.surface));
        assert!(luma(dark.surface) < luma(dark.surface_2));
        assert!(luma(dark.surface_2) < luma(dark.surface_3));
    }

    #[test]
    fn lerp_hits_endpoints_and_stays_opaque() {
        assert_eq!(lerp(0xff00_0000, 0xffff_ffff, 0.0), 0xff00_0000);
        assert_eq!(lerp(0xff00_0000, 0xffff_ffff, 1.0), 0xffff_ffff);
        assert_eq!(lerp(0x0000_0000, 0x0000_0000, 0.5) >> 24, 0xff);
    }
}
