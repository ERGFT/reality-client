//! Сохранение и применение профиля при подключении.

use std::{
    rc::Rc,
    sync::{Arc, Mutex},
};

use slint::{ModelRc, SharedString, VecModel};
use zeroize::Zeroizing;

use crate::{
    MainWindow,
    profiles::ProfileStore,
    server_info::{server_endpoint, server_ip_initial},
};

pub(crate) struct SavedProfile {
    pub(crate) index: usize,
    pub(crate) names: Vec<String>,
    pub(crate) name: String,
    pub(crate) link: Zeroizing<String>,
}

pub(crate) type SelectedProfileLink = Arc<Mutex<Option<(usize, Zeroizing<String>)>>>;

pub(crate) fn initial_profile_index(profile_count: usize) -> Option<usize> {
    (profile_count > 0).then_some(0)
}

pub(crate) fn profile_mutation_blocked(operation_busy: bool, session_active: bool) -> bool {
    operation_busy || session_active
}

pub(crate) fn save_profile_for_connection(
    profile_store: &Arc<Mutex<Option<ProfileStore>>>,
    name: &str,
    link: &Zeroizing<String>,
    selected: Option<usize>,
) -> Result<SavedProfile, String> {
    let mut guard = profile_store
        .lock()
        .map_err(|_| "Хранилище профилей недоступно.".to_owned())?;
    let store = guard.as_mut().ok_or_else(|| {
        "Защищённое хранилище профилей недоступно; ссылка не сохранена.".to_owned()
    })?;
    let name = if name.trim().is_empty() {
        "Текущий профиль"
    } else {
        name.trim()
    };
    if let Some(index) = selected.filter(|index| store.subscription_identity(*index).is_some()) {
        if store.read_link(index)?.as_str() != link.as_str() {
            return Err(
                "Сервер подписки нельзя изменять вручную. Добавьте отдельный ручной профиль."
                    .into(),
            );
        }
        let names = store.names();
        return Ok(SavedProfile {
            index,
            name: names[index].clone(),
            names,
            link: Zeroizing::new(link.to_string()),
        });
    }
    let index = store.save(name, link, selected)?;
    Ok(SavedProfile {
        index,
        names: store.names(),
        name: name.to_owned(),
        link: Zeroizing::new(link.to_string()),
    })
}

pub(crate) fn apply_saved_profile(
    window: &MainWindow,
    profile: &SavedProfile,
    selected_profile_link: &SelectedProfileLink,
) {
    window.set_profile_model(ModelRc::from(Rc::new(VecModel::from(
        profile
            .names
            .iter()
            .map(|name| SharedString::from(name.as_str()))
            .collect::<Vec<_>>(),
    ))));
    window.set_selected_profile_index(profile.index as i32);
    window.set_profile_count(format!("Профили: {}", profile.names.len()).into());
    window.set_profile_name(profile.name.as_str().into());
    window.set_vless_link(profile.link.as_str().into());
    window.set_server_endpoint_text(server_endpoint(profile.link.as_str()).into());
    window.set_server_ip_text(server_ip_initial(profile.link.as_str()).into());
    if let Ok(mut cache) = selected_profile_link.lock() {
        *cache = Some((profile.index, Zeroizing::new(profile.link.to_string())));
    }
}
#[cfg(test)]
mod profile_startup_tests {
    use super::{initial_profile_index, profile_mutation_blocked};

    #[test]
    fn profile_mutation_is_blocked_during_operation_or_live_session() {
        assert!(!profile_mutation_blocked(false, false));
        assert!(profile_mutation_blocked(true, false));
        assert!(profile_mutation_blocked(false, true));
    }

    #[test]
    fn selects_first_saved_profile_on_startup() {
        assert_eq!(initial_profile_index(1), Some(0));
        assert_eq!(initial_profile_index(100), Some(0));
    }

    #[test]
    fn leaves_selection_empty_when_no_profiles_exist() {
        assert_eq!(initial_profile_index(0), None);
    }
}

#[cfg(all(test, windows))]
mod connect_profile_tests {
    use super::{ProfileStore, Zeroizing, save_profile_for_connection};
    use std::sync::{Arc, Mutex};

    #[test]
    fn subscription_connection_preserves_manual_vault_and_stable_identity() {
        let dir = std::env::temp_dir().join(format!(
            "subscription-mixed-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("profiles.dat");
        let mut store = ProfileStore::open_at(path.clone()).unwrap();
        let manual = "vless://00000000-0000-4000-8000-000000000000@manual.example.org:443";
        store.save("Manual", manual, None).unwrap();
        let original = std::fs::read(&path).unwrap();
        let a = "vless://00000000-0000-4000-8000-000000000000@a.example.org:443#A";
        let b = "vless://00000000-0000-4000-8000-000000000000@b.example.org:443#B";
        let parsed = crate::subscriptions::parse(format!("{a}\n{b}").as_bytes()).unwrap();
        let url = "https://subscription.example.org/fixture";
        let group = store
            .subscriptions
            .as_mut()
            .unwrap()
            .replace(None, "Group", url, &parsed)
            .unwrap();
        let identity = store.subscription_identity(2).unwrap();
        let reordered = crate::subscriptions::parse(format!("{b}\n{a}").as_bytes()).unwrap();
        store
            .subscriptions
            .as_mut()
            .unwrap()
            .replace(Some(group), "Group", url, &reordered)
            .unwrap();
        assert_eq!(store.index_for_identity(identity), Some(1));
        assert_eq!(store.read_link(0).unwrap().as_str(), manual);
        let shared = Arc::new(Mutex::new(Some(store)));
        let link = Zeroizing::new(b.to_owned());
        let saved = save_profile_for_connection(&shared, "Ignored edit", &link, Some(1)).unwrap();
        assert_eq!(saved.index, 1);
        assert_eq!(saved.name, "Group · B");
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let changed = Zeroizing::new(a.to_owned());
        assert!(save_profile_for_connection(&shared, "Ignored", &changed, Some(1)).is_err());
        let reopened = ProfileStore::open_at(path.clone()).unwrap();
        assert_eq!(reopened.len(), 3);
        assert_eq!(reopened.names(), ["Manual", "Group · B", "Group · A"]);
        assert_eq!(reopened.read_link(1).unwrap().as_str(), b);
        let remaining = crate::subscriptions::parse(a.as_bytes()).unwrap();
        shared
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .subscriptions
            .as_mut()
            .unwrap()
            .replace(Some(group), "Group", url, &remaining)
            .unwrap();
        assert!(
            shared
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .index_for_identity(identity)
                .is_none()
        );
        std::fs::write(dir.join("subscriptions.json"), b"corrupted fixture").unwrap();
        let manual_only = ProfileStore::open_at(path).unwrap();
        assert!(manual_only.subscription_error.is_some());
        assert_eq!(manual_only.read_link(0).unwrap().as_str(), manual);
        assert_eq!(std::fs::read(dir.join("profiles.dat")).unwrap(), original);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn connect_profile_save_creates_then_replaces_protected_profile() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "reality-connect-profile-test-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("profiles.dat");
        let store = ProfileStore::open_at(path.clone()).unwrap();
        let shared_store = Arc::new(Mutex::new(Some(store)));

        let initial_link = Zeroizing::new(
            "vless://00000000-0000-4000-8000-000000000000@edge.example.org:443?encryption=none"
                .to_owned(),
        );
        let created = save_profile_for_connection(&shared_store, " ", &initial_link, None).unwrap();
        assert_eq!(created.index, 0);
        assert_eq!(created.name, "Текущий профиль");
        assert_eq!(created.names, ["Текущий профиль"]);

        let edited_link = Zeroizing::new(
            "vless://11111111-1111-4111-8111-111111111111@edge.example.org:8443?encryption=none"
                .to_owned(),
        );
        let updated = save_profile_for_connection(
            &shared_store,
            "Обновлённый",
            &edited_link,
            Some(created.index),
        )
        .unwrap();
        assert_eq!(updated.index, 0);
        assert_eq!(updated.names, ["Обновлённый"]);
        assert_eq!(updated.link.as_str(), edited_link.as_str());

        let reopened = ProfileStore::open_at(path).unwrap();
        assert_eq!(reopened.names(), ["Обновлённый"]);
        assert_eq!(
            reopened.read_link(0).unwrap().as_str(),
            edited_link.as_str()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
