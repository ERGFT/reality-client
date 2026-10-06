//! Чтение ссылки VLESS из буфера обмена.

#[cfg(windows)]
use std::time::Duration;

use zeroize::Zeroizing;

#[cfg(windows)]
pub(crate) fn read_clipboard_profile_link() -> Result<Zeroizing<String>, String> {
    use windows::Win32::{
        Foundation::HGLOBAL,
        System::{
            DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard},
            Memory::{GlobalLock, GlobalSize, GlobalUnlock},
        },
    };

    struct ClipboardGuard;
    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseClipboard();
            }
        }
    }

    struct ClipboardMemoryGuard(HGLOBAL);
    impl Drop for ClipboardMemoryGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = GlobalUnlock(self.0);
            }
        }
    }

    let mut opened = false;
    for _ in 0..8 {
        if unsafe { OpenClipboard(None) }.is_ok() {
            opened = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if !opened {
        return Err("Буфер обмена занят другим приложением. Попробуйте ещё раз.".into());
    }
    let _clipboard_guard = ClipboardGuard;

    const CF_UNICODETEXT: u32 = 13;
    const MAX_CLIPBOARD_BYTES: usize = 1024 * 1024;
    let handle = unsafe { GetClipboardData(CF_UNICODETEXT) }
        .map_err(|_| "В буфере обмена нет текста со ссылкой.".to_owned())?;
    let memory = HGLOBAL(handle.0);
    let size = unsafe { GlobalSize(memory) };
    if !(2..=MAX_CLIPBOARD_BYTES).contains(&size) || size % 2 != 0 {
        return Err("Текст в буфере обмена имеет недопустимый размер.".into());
    }
    let pointer = unsafe { GlobalLock(memory) }.cast::<u16>();
    if pointer.is_null() {
        return Err("Не удалось прочитать текст из буфера обмена.".into());
    }
    let _memory_guard = ClipboardMemoryGuard(memory);
    // SAFETY: the clipboard stays open and the global memory is locked until
    // both guards leave scope; `size` comes from GlobalSize for this handle.
    let utf16 = unsafe { std::slice::from_raw_parts(pointer, size / 2) };
    let Some(end) = utf16.iter().position(|unit| *unit == 0) else {
        return Err("Текст в буфере обмена не завершён корректно.".into());
    };
    let text = String::from_utf16(&utf16[..end])
        .map_err(|_| "Текст в буфере обмена содержит некорректный Unicode.".to_owned())?;
    sanitize_clipboard_profile_link(text)
}

#[cfg(target_os = "android")]
pub(crate) fn read_clipboard_profile_link() -> Result<Zeroizing<String>, String> {
    let text = platform::read_android_clipboard_text()?;
    sanitize_clipboard_profile_link(text)
}

#[cfg(target_os = "linux")]
pub(crate) fn read_clipboard_profile_link() -> Result<Zeroizing<String>, String> {
    let text = arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.get_text())
        .map_err(|problem| {
            format!("Не удалось прочитать текст из буфера обмена Linux: {problem}")
        })?;
    sanitize_clipboard_profile_link(text)
}

pub(crate) fn sanitize_clipboard_profile_link(text: String) -> Result<Zeroizing<String>, String> {
    let text = Zeroizing::new(text);
    if text.contains(['\r', '\n', '\0']) {
        return Err("В буфере несколько строк. Скопируйте только одну ссылку VLESS.".into());
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("В буфере нет текста со ссылкой VLESS.".into());
    }
    Ok(Zeroizing::new(trimmed.to_owned()))
}

#[cfg(test)]
mod clipboard_tests {
    use super::sanitize_clipboard_profile_link;

    #[test]
    fn clipboard_paste_trims_outer_whitespace_and_preserves_link() {
        let pasted =
            sanitize_clipboard_profile_link("  vless://secret@example.com:443  ".into()).unwrap();
        assert_eq!(pasted.as_str(), "vless://secret@example.com:443");
    }

    #[test]
    fn clipboard_paste_rejects_multiline_content() {
        let error = sanitize_clipboard_profile_link("vless://one\nvless://two".into()).unwrap_err();
        assert!(error.contains("несколько строк"));
        assert!(!error.contains("vless://"));
    }

    #[test]
    fn clipboard_paste_rejects_empty_text() {
        assert!(sanitize_clipboard_profile_link(" \t ".into()).is_err());
    }
}
