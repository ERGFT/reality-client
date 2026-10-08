//! Subscription metadata and secret references. Manual profiles remain in
//! their original file; replacing a group requires one atomic file commit.

use std::{
    fs::File,
    io::Read,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::{
    profiles::{delete_protected, protect, unprotect, write_private_file_atomic},
    subscriptions::{ImportResult, MAX_SERVERS, validate_url},
};

const MAX_FILE_BYTES: usize = 32 * 1024 * 1024;
const MAX_GROUPS: usize = 50;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SubscriptionServer {
    pub id: u64,
    pub name: String,
    protected_link: Vec<u8>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Subscription {
    pub id: u64,
    pub name: String,
    protected_url: Vec<u8>,
    pub servers: Vec<SubscriptionServer>,
    pub last_success: u64,
    pub last_error: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Vault {
    version: u32,
    next_id: u64,
    subscriptions: Vec<Subscription>,
}

pub(crate) struct SubscriptionStore {
    path: PathBuf,
    vault: Vault,
}

pub(crate) fn validate_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 100 || name.chars().any(char::is_control) {
        return Err("Название подписки должно содержать от 1 до 100 печатных символов.".into());
    }
    Ok(name)
}

fn read_secret(reference: &[u8]) -> Result<Zeroizing<String>, String> {
    let mut clear = unprotect(reference)?;
    match String::from_utf8(std::mem::take(&mut *clear)) {
        Ok(text) => Ok(Zeroizing::new(text)),
        Err(error) => {
            error.into_bytes().zeroize();
            Err("Секрет подписки повреждён.".into())
        }
    }
}

impl SubscriptionStore {
    pub fn open_at(path: PathBuf) -> Result<Self, String> {
        let vault = if path.exists() {
            let file = File::open(&path).map_err(|_| "Не удалось открыть подписки.".to_owned())?;
            let mut bytes = Vec::new();
            file.take(MAX_FILE_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| "Не удалось прочитать подписки.".to_owned())?;
            if bytes.len() > MAX_FILE_BYTES {
                return Err("Хранилище подписок превышает 32 МиБ.".into());
            }
            let vault: Vault = serde_json::from_slice(&bytes)
                .map_err(|_| "Хранилище подписок повреждено; данные не изменены.".to_owned())?;
            validate_vault(&vault)?;
            vault
        } else {
            Vault {
                version: 1,
                next_id: 1,
                subscriptions: Vec::new(),
            }
        };
        Ok(Self { path, vault })
    }

    pub fn groups(&self) -> &[Subscription] {
        &self.vault.subscriptions
    }

    pub fn url(&self, id: u64) -> Result<Zeroizing<String>, String> {
        read_secret(&self.group(id)?.protected_url)
    }

    pub fn link(&self, group: u64, server: u64) -> Result<Zeroizing<String>, String> {
        let server = self
            .group(group)?
            .servers
            .iter()
            .find(|entry| entry.id == server)
            .ok_or("Сервер подписки больше не существует.")?;
        read_secret(&server.protected_link)
    }

    fn group(&self, id: u64) -> Result<&Subscription, String> {
        self.groups()
            .iter()
            .find(|group| group.id == id)
            .ok_or("Подписка не найдена.".into())
    }

    /// Build a replacement first; commit metadata before deleting old secrets.
    /// On any failure, neither the previous group nor manual profiles change.
    pub fn replace(
        &mut self,
        id: Option<u64>,
        name: &str,
        url: &str,
        imported: &ImportResult,
    ) -> Result<u64, String> {
        let name = validate_name(name)?.to_owned();
        validate_url(url)?;
        if imported.servers.is_empty() {
            return Err(format!(
                "Нет поддерживаемых серверов; предыдущие серверы сохранены. {}",
                imported.summary()
            ));
        }
        if id.is_none() && self.groups().len() >= MAX_GROUPS {
            return Err("Предел — 50 подписок.".into());
        }
        let old = id.map(|id| self.group(id).cloned()).transpose()?;
        let total: usize = self
            .groups()
            .iter()
            .filter(|group| Some(group.id) != id)
            .map(|group| group.servers.len())
            .sum();
        if total + imported.servers.len() > MAX_SERVERS {
            return Err("Предел — 1000 серверов во всех подписках.".into());
        }
        let mut updated = self.vault.clone();
        let mut created = Vec::new();
        let result = (|| {
            let group_id = match id {
                Some(id) => id,
                None => allocate_id(&mut updated)?,
            };
            let protected_url = protect(url.as_bytes())?;
            created.push(protected_url.clone());
            let mut previous = Vec::new();
            if let Some(old) = &old {
                for server in &old.servers {
                    let link = read_secret(&server.protected_link)?;
                    let config = reality_core::vless::uri::VlessConfig::parse(&link)
                        .map_err(|_| "Сохранённый сервер подписки повреждён.".to_owned())?;
                    previous.push((server.id, Zeroizing::new(config.pool_key())));
                }
            }
            let mut servers = Vec::new();
            for server in &imported.servers {
                let config = reality_core::vless::uri::VlessConfig::parse(&server.link)
                    .map_err(|_| "Некорректный сервер подписки.".to_owned())?;
                let identity = Zeroizing::new(config.pool_key());
                let id = match previous.iter().find(|(_, key)| **key == *identity) {
                    Some((id, _)) => *id,
                    None => allocate_id(&mut updated)?,
                };
                let protected_link = protect(server.link.as_bytes())?;
                created.push(protected_link.clone());
                servers.push(SubscriptionServer {
                    id,
                    name: server.name.clone(),
                    protected_link,
                });
            }
            let group = Subscription {
                id: group_id,
                name,
                protected_url,
                servers,
                last_success: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                last_error: None,
            };
            match updated
                .subscriptions
                .iter()
                .position(|group| group.id == group_id)
            {
                Some(index) => updated.subscriptions[index] = group,
                None => updated.subscriptions.push(group),
            }
            self.commit(&updated)?;
            self.vault = updated;
            Ok(group_id)
        })();
        if result.is_err() {
            for reference in created {
                let _ = delete_protected(&reference);
            }
        } else if let Some(old) = old {
            delete_group_secrets(&old);
        }
        result
    }

    pub fn rename(&mut self, id: u64, name: &str) -> Result<(), String> {
        let name = validate_name(name)?.to_owned();
        let mut updated = self.vault.clone();
        updated
            .subscriptions
            .iter_mut()
            .find(|group| group.id == id)
            .ok_or("Подписка не найдена.")?
            .name = name;
        self.commit(&updated)?;
        self.vault = updated;
        Ok(())
    }

    /// Caller supplies a sanitized error, never a raw network/parse error.
    pub fn record_failure(&mut self, id: u64, error: &str) -> Result<(), String> {
        let mut updated = self.vault.clone();
        updated
            .subscriptions
            .iter_mut()
            .find(|group| group.id == id)
            .ok_or("Подписка не найдена.")?
            .last_error = Some(error.chars().take(2048).collect());
        self.commit(&updated)?;
        self.vault = updated;
        Ok(())
    }

    pub fn delete(&mut self, id: u64) -> Result<(), String> {
        let mut updated = self.vault.clone();
        let index = updated
            .subscriptions
            .iter()
            .position(|group| group.id == id)
            .ok_or("Подписка не найдена.")?;
        let old = updated.subscriptions.remove(index);
        self.commit(&updated)?;
        self.vault = updated;
        delete_group_secrets(&old);
        Ok(())
    }

    fn commit(&self, updated: &Vault) -> Result<(), String> {
        validate_vault(updated)?;
        let bytes =
            serde_json::to_vec(updated).map_err(|_| "Не удалось сохранить подписки.".to_owned())?;
        write_private_file_atomic(&self.path, &bytes, MAX_FILE_BYTES)
    }
}

fn allocate_id(vault: &mut Vault) -> Result<u64, String> {
    let id = vault.next_id;
    vault.next_id = id
        .checked_add(1)
        .ok_or("Исчерпаны идентификаторы подписок.")?;
    Ok(id)
}

fn validate_vault(vault: &Vault) -> Result<(), String> {
    let invalid = || "Некорректное хранилище подписок; данные не изменены.".to_owned();
    if vault.version != 1
        || vault.next_id == 0
        || vault.subscriptions.len() > MAX_GROUPS
        || vault
            .subscriptions
            .iter()
            .map(|group| group.servers.len())
            .sum::<usize>()
            > MAX_SERVERS
    {
        return Err(invalid());
    }
    let mut ids = std::collections::HashSet::new();
    for group in &vault.subscriptions {
        validate_name(&group.name)?;
        if group.id == 0
            || group.id >= vault.next_id
            || !ids.insert(group.id)
            || !(1..=16 * 1024).contains(&group.protected_url.len())
            || group.servers.is_empty()
            || group
                .last_error
                .as_ref()
                .is_some_and(|error| error.len() > 8192)
        {
            return Err(invalid());
        }
        for server in &group.servers {
            if server.id == 0
                || server.id >= vault.next_id
                || !ids.insert(server.id)
                || server.name.chars().count() > 100
                || server.name.chars().any(char::is_control)
                || !(1..=16 * 1024).contains(&server.protected_link.len())
            {
                return Err(invalid());
            }
        }
    }
    Ok(())
}

fn delete_group_secrets(group: &Subscription) {
    let _ = delete_protected(&group.protected_url);
    for server in &group.servers {
        let _ = delete_protected(&server.protected_link);
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::subscriptions::parse;
    const URL: &str = "https://service.example.org/sublink/private-fixture";
    const FIRST: &str = "vless://00000000-0000-4000-8000-000000000000@edge.example.org:443#First";

    #[test]
    fn protected_store_preserves_identity_and_old_data_on_invalid_refresh() {
        let root = std::env::temp_dir().join(format!(
            "subscription-store-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("subscriptions.json");
        let mut store = SubscriptionStore::open_at(path.clone()).unwrap();
        let imported = parse(FIRST.as_bytes()).unwrap();
        let id = store.replace(None, "Example", URL, &imported).unwrap();
        let server_id = store.groups()[0].servers[0].id;
        assert_eq!(store.url(id).unwrap().as_str(), URL);
        assert_eq!(store.link(id, server_id).unwrap().as_str(), FIRST);
        let bytes = std::fs::read_to_string(&path).unwrap();
        assert!(!bytes.contains("private-fixture"));
        assert!(!bytes.contains("vless://"));
        let changed = parse(FIRST.replace("#First", "#Renamed").as_bytes()).unwrap();
        store.replace(Some(id), "Example", URL, &changed).unwrap();
        assert_eq!(store.groups()[0].servers[0].id, server_id);
        let before = std::fs::read(&path).unwrap();
        let original_path = store.path.clone();
        store.path = root.join("blocked");
        std::fs::create_dir(&store.path).unwrap();
        assert!(
            store
                .replace(Some(id), "Must not commit", URL, &changed)
                .is_err()
        );
        store.path = original_path;
        assert_eq!(store.groups()[0].name, "Example");
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(
            store.link(id, server_id).unwrap().as_str(),
            changed.servers[0].link.as_str()
        );
        let empty = parse(b"trojan://unsupported@example.org:443").unwrap();
        assert!(store.replace(Some(id), "Example", URL, &empty).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        store.record_failure(id, "Network unavailable").unwrap();
        store.rename(id, "Renamed group").unwrap();
        let mut store = SubscriptionStore::open_at(path).unwrap();
        assert_eq!(store.groups()[0].name, "Renamed group");
        assert_eq!(
            store.groups()[0].last_error.as_deref(),
            Some("Network unavailable")
        );
        assert_eq!(store.groups()[0].servers[0].id, server_id);
        store.delete(id).unwrap();
        assert!(store.groups().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}
