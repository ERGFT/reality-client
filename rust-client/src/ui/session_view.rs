//! Android's service outlives an Activity. Keep display metadata outside the
//! window so recreation cannot replace the active endpoint with a selected one.
use crate::profiles::ProfileKey;

#[derive(Clone)]
pub(super) struct SessionView {
    pub name: String,
    pub endpoint: String,
    pub selection: Option<ProfileKey>,
    pub diagnostic_proxy: Option<String>,
}

#[cfg(target_os = "android")]
static ACTIVE: std::sync::Mutex<Option<SessionView>> = std::sync::Mutex::new(None);

#[cfg(target_os = "android")]
pub(super) fn set(view: Option<SessionView>) {
    if let Ok(mut active) = ACTIVE.lock() {
        *active = view;
    }
}

#[cfg(target_os = "android")]
pub(super) fn get() -> Option<SessionView> {
    ACTIVE.lock().ok().and_then(|active| active.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connected_snapshot_is_independent_of_window_selection() {
        let active = SessionView {
            name: "Connected server".into(),
            endpoint: "server.example:443".into(),
            selection: Some(ProfileKey::Subscription(7, 9)),
            diagnostic_proxy: Some("socks5h://127.0.0.1:1080".into()),
        };
        let restored = active.clone();
        let mut selected = active;
        selected.name = "Another selection".into();
        selected.endpoint = "other.example:443".into();
        selected.selection = Some(ProfileKey::Subscription(7, 10));
        selected.diagnostic_proxy = Some("socks5h://127.0.0.1:2080".into());
        assert_eq!(restored.name, "Connected server");
        assert_eq!(restored.endpoint, "server.example:443");
        assert_eq!(restored.selection, Some(ProfileKey::Subscription(7, 9)));
        assert_eq!(
            restored.diagnostic_proxy.as_deref(),
            Some("socks5h://127.0.0.1:1080")
        );
    }
}
