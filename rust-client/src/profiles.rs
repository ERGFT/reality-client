use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use zeroize::{Zeroize, Zeroizing};

const HEADER: &[u8] = b"RCLIENT1";
const MAX_PROFILES: usize = 100;
const MAX_PROTECTED_LINK: usize = 16 * 1024;
const MAX_VAULT_SIZE: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug)]
struct SavedProfile {
    name: String,
    protected_link: Vec<u8>,
}

pub struct ProfileStore {
    path: PathBuf,
    profiles: Vec<SavedProfile>,
}

pub fn storage_backend_description() -> &'static str {
    #[cfg(windows)]
    {
        "Windows DPAPI"
    }
    #[cfg(target_os = "linux")]
    {
        "Linux Secret Service"
    }
    #[cfg(target_os = "android")]
    {
        "Android Keystore"
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "android")))]
    {
        "защищённое хранилище этой платформы"
    }
}

impl ProfileStore {
    pub fn open_default() -> Result<Self, String> {
        let root = data_directory()?;
        crate::platform::ensure_private_dir(&root)?;
        let path = root.join("profiles.dat");
        Self::open_at(path)
    }

    pub(super) fn open_at(path: PathBuf) -> Result<Self, String> {
        let profiles = if path.exists() {
            let file = File::open(&path).map_err(|e| format!("Не удалось открыть профили: {e}"))?;
            let mut bytes = Vec::new();
            file.take(MAX_VAULT_SIZE as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| format!("Не удалось прочитать профили: {e}"))?;
            if bytes.len() > MAX_VAULT_SIZE {
                return Err("Файл профилей превышает допустимый размер 2 МиБ.".into());
            }
            decode_legacy_vault(&bytes)?
        } else {
            Vec::new()
        };

        Ok(Self { path, profiles })
    }

    pub fn len(&self) -> usize {
        self.profiles.len()
    }

    pub fn names(&self) -> Vec<String> {
        self.profiles.iter().map(|p| p.name.clone()).collect()
    }

    pub fn read_link(&self, index: usize) -> Result<Zeroizing<String>, String> {
        let profile = self.profiles.get(index).ok_or("Профиль не найден.")?;
        let clear = unprotect(&profile.protected_link)?;
        let text = String::from_utf8(clear.to_vec())
            .map_err(|_| "Сохранённая ссылка профиля повреждена.".to_owned())?;
        Ok(Zeroizing::new(text))
    }

    pub fn save(
        &mut self,
        name: &str,
        link: &str,
        selected: Option<usize>,
    ) -> Result<usize, String> {
        validate_vless_link(link)?;
        if link.len() > MAX_PROTECTED_LINK {
            return Err("VLESS-ссылка превышает допустимый размер 16 КиБ.".into());
        }
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 100 || name.chars().any(char::is_control) {
            return Err("Название профиля должно содержать от 1 до 100 печатных символов.".into());
        }

        if self.profiles.len() >= MAX_PROFILES && selected.is_none() {
            return Err("Достигнут предел в 100 профилей.".into());
        }
        if selected.is_some_and(|index| index >= self.profiles.len()) {
            return Err("Выбранный профиль больше не существует.".into());
        }

        let mut clear = Zeroizing::new(link.as_bytes().to_vec());
        let protected_link = protect(&clear)?;
        clear.zeroize();
        let new_reference = protected_link.clone();

        let mut updated = self.profiles.clone();
        let record = SavedProfile {
            name: name.to_owned(),
            protected_link,
        };
        let index = match selected {
            Some(i) if i < updated.len() => {
                updated[i] = record;
                i
            }
            Some(_) => unreachable!("selected index validated before secret storage"),
            None => {
                updated.push(record);
                updated.len() - 1
            }
        };

        if let Err(problem) = write_vault_atomic(&self.path, &encode_vault(&updated)) {
            let _ = delete_protected(&new_reference);
            return Err(problem);
        }
        let old_reference =
            selected.map(|old_index| self.profiles[old_index].protected_link.clone());
        self.profiles = updated;
        if let Some(old_reference) = old_reference {
            let _ = delete_protected(&old_reference);
        }
        Ok(index)
    }

    pub fn delete(&mut self, index: usize) -> Result<(), String> {
        if index >= self.profiles.len() {
            return Err("Профиль не найден.".into());
        }
        let mut updated = self.profiles.clone();
        let removed = updated.remove(index);
        write_vault_atomic(&self.path, &encode_vault(&updated))?;
        self.profiles = updated;
        let _ = delete_protected(&removed.protected_link);
        Ok(())
    }
}

pub fn validate_vless_link(link: &str) -> Result<(), String> {
    if link.trim().is_empty() {
        return Err("Вставьте VLESS-ссылку.".into());
    }
    if link.contains(['\r', '\n', '\0']) {
        return Err("Ссылка должна занимать одну строку.".into());
    }
    let uri = url::Url::parse(link).map_err(|_| "Проверьте формат VLESS-ссылки.".to_owned())?;
    if !uri.scheme().eq_ignore_ascii_case("vless") {
        return Err("Ожидается ссылка формата vless://UUID@сервер:порт?...".into());
    }
    if uri.username().is_empty() || uri.host_str().is_none() || uri.port().is_none() {
        return Err("В ссылке должны быть указаны UUID, имя сервера и порт.".into());
    }
    Ok(())
}

fn decode_legacy_vault(bytes: &[u8]) -> Result<Vec<SavedProfile>, String> {
    let mut cursor = Cursor::new(bytes);
    if cursor.dotnet_string()?.as_bytes() != HEADER {
        return Err("Неизвестный формат хранилища профилей.".into());
    }
    let count = cursor.i32()?;
    if !(0..=MAX_PROFILES as i32).contains(&count) {
        return Err("Некорректное количество профилей.".into());
    }

    let mut profiles = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let name = cursor.dotnet_string()?;
        let size = cursor.i32()?;
        if !(1..=MAX_PROTECTED_LINK as i32).contains(&size) {
            return Err("Некорректный размер сохранённого профиля.".into());
        }
        profiles.push(SavedProfile {
            name,
            protected_link: cursor.take(size as usize)?.to_vec(),
        });
    }
    if !cursor.is_empty() {
        return Err("В хранилище обнаружены лишние данные.".into());
    }
    Ok(profiles)
}

fn encode_vault(profiles: &[SavedProfile]) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_dotnet_string(&mut bytes, "RCLIENT1");
    bytes.extend_from_slice(&(profiles.len() as i32).to_le_bytes());
    for profile in profiles {
        write_dotnet_string(&mut bytes, &profile.name);
        bytes.extend_from_slice(&(profile.protected_link.len() as i32).to_le_bytes());
        bytes.extend_from_slice(&profile.protected_link);
    }
    bytes
}

fn write_dotnet_string(out: &mut Vec<u8>, value: &str) {
    let utf8 = value.as_bytes();
    let mut length = utf8.len() as u32;
    while length >= 0x80 {
        out.push((length as u8) | 0x80);
        length >>= 7;
    }
    out.push(length as u8);
    out.extend_from_slice(utf8);
}

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn take(&mut self, size: usize) -> Result<&'a [u8], String> {
        let end = self
            .position
            .checked_add(size)
            .ok_or("Повреждённое хранилище профилей.")?;
        let slice = self
            .bytes
            .get(self.position..end)
            .ok_or("Файл профилей повреждён.")?;
        self.position = end;
        Ok(slice)
    }

    fn i32(&mut self) -> Result<i32, String> {
        let bytes: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_| "Файл профилей повреждён.")?;
        Ok(i32::from_le_bytes(bytes))
    }

    fn dotnet_string(&mut self) -> Result<String, String> {
        let mut length = 0u32;
        let mut shift = 0;
        loop {
            if shift >= 35 {
                return Err("Некорректная длина строки в хранилище.".into());
            }
            let byte = *self.take(1)?.first().ok_or("Файл профилей повреждён.")?;
            if shift == 28 && byte > 0x0f {
                return Err("Некорректная длина строки в хранилище.".into());
            }
            length |= ((byte & 0x7f) as u32) << shift;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
        }
        let text = std::str::from_utf8(self.take(length as usize)?)
            .map_err(|_| "Название профиля повреждено.".to_owned())?;
        Ok(text.to_owned())
    }

    fn is_empty(&self) -> bool {
        self.position == self.bytes.len()
    }
}

fn write_vault_atomic(path: &Path, contents: &[u8]) -> Result<(), String> {
    if contents.len() > MAX_VAULT_SIZE {
        return Err("Файл профилей превышает допустимый размер 2 МиБ.".into());
    }
    let parent = path.parent().ok_or("Некорректный путь хранилища.")?;
    fs::create_dir_all(parent).map_err(|e| format!("Не удалось создать папку данных: {e}"))?;
    let temporary = path.with_extension("dat.tmp");
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temporary)
        .map_err(|e| format!("Не удалось создать временное хранилище: {e}"))?;
    file.write_all(contents)
        .map_err(|e| format!("Не удалось записать хранилище: {e}"))?;
    file.sync_all()
        .map_err(|e| format!("Не удалось синхронизировать хранилище: {e}"))?;
    drop(file);
    fs::rename(&temporary, path).map_err(|e| {
        let _ = fs::remove_file(&temporary);
        format!("Не удалось атомарно заменить хранилище: {e}")
    })
}

fn data_directory() -> Result<PathBuf, String> {
    crate::platform::app_data_dir()
}

#[cfg(windows)]
fn protect(clear: &[u8]) -> Result<Vec<u8>, String> {
    use windows::{
        Win32::{
            Foundation::{HLOCAL, LocalFree},
            Security::Cryptography::{
                CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData,
            },
        },
        core::PCWSTR,
    };
    if clear.is_empty() || clear.len() > MAX_PROTECTED_LINK {
        return Err("Ссылка имеет недопустимую длину.".into());
    }
    let input = CRYPT_INTEGER_BLOB {
        cbData: clear.len() as u32,
        pbData: clear.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(
            &input,
            PCWSTR::null(),
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|e| format!("Windows не смогла защитить профиль: {e}"))?;
        if output.pbData.is_null() || output.cbData == 0 {
            if !output.pbData.is_null() {
                let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
            }
            return Err("Windows вернула пустую защищённую ссылку.".into());
        }
        if output.cbData as usize > MAX_PROTECTED_LINK {
            let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
            return Err("Защищённая ссылка превышает допустимый размер профиля.".into());
        }
        let protected = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
        Ok(protected)
    }
}

#[cfg(target_os = "linux")]
fn protect(clear: &[u8]) -> Result<Vec<u8>, String> {
    let id = uuid::Uuid::new_v4().to_string();
    let entry = keyring::Entry::new("reality-client", &id)
        .map_err(|_| "Не удалось создать запись в системном хранилище секретов.".to_owned())?;
    entry
        .set_secret(clear)
        .map_err(|_| "Linux Secret Service не смог сохранить секрет профиля.".to_owned())?;
    Ok(id.into_bytes())
}

#[cfg(target_os = "android")]
fn protect(clear: &[u8]) -> Result<Vec<u8>, String> {
    use android_native_keyring_store::by_store::Store;
    use keyring_core::api::CredentialStoreApi;

    let id = uuid::Uuid::new_v4().to_string();
    let store =
        Store::new().map_err(|_| "Не удалось открыть Android Keystore для профиля.".to_owned())?;
    let entry = store
        .build("reality-client", &id, None)
        .map_err(|_| "Не удалось создать запись профиля в Android Keystore.".to_owned())?;
    entry
        .set_secret(clear)
        .map_err(|_| "Android Keystore не смог сохранить секрет профиля.".to_owned())?;
    Ok(id.into_bytes())
}

fn delete_protected(reference: &[u8]) -> Result<(), String> {
    #[cfg(windows)]
    {
        let _ = reference;
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        let entry = linux_keyring_entry(reference)?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("Не удалось удалить секрет профиля из Linux Secret Service.".into()),
        }
    }
    #[cfg(target_os = "android")]
    {
        use android_native_keyring_store::by_store::Store;
        use keyring_core::api::CredentialStoreApi;

        let id = std::str::from_utf8(reference)
            .map_err(|_| "Ссылка на секрет профиля повреждена.".to_owned())?;
        uuid::Uuid::parse_str(id).map_err(|_| "Ссылка на секрет профиля повреждена.".to_owned())?;
        let store = Store::new()
            .map_err(|_| "Не удалось открыть Android Keystore для профиля.".to_owned())?;
        let entry = store
            .build("reality-client", id, None)
            .map_err(|_| "Не удалось открыть запись профиля в Android Keystore.".to_owned())?;
        entry
            .delete_credential()
            .map_err(|_| "Не удалось удалить секрет профиля из Android Keystore.".to_owned())
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "android")))]
    {
        let _ = reference;
        Ok(())
    }
}

#[cfg(target_os = "android")]
fn unprotect(protected: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
    use android_native_keyring_store::by_store::Store;
    use keyring_core::api::CredentialStoreApi;

    let id = std::str::from_utf8(protected)
        .map_err(|_| "Ссылка на секрет профиля повреждена.".to_owned())?;
    uuid::Uuid::parse_str(id).map_err(|_| "Ссылка на секрет профиля повреждена.".to_owned())?;
    let store =
        Store::new().map_err(|_| "Не удалось открыть Android Keystore для профиля.".to_owned())?;
    let entry = store
        .build("reality-client", id, None)
        .map_err(|_| "Не удалось открыть запись профиля в Android Keystore.".to_owned())?;
    let mut secret = entry
        .get_secret()
        .map_err(|_| "Не удалось получить профиль из Android Keystore.".to_owned())?;
    if secret.is_empty() || secret.len() > MAX_PROTECTED_LINK {
        secret.zeroize();
        return Err("Сохранённая ссылка профиля имеет недопустимый размер.".into());
    }
    Ok(Zeroizing::new(secret))
}

#[cfg(target_os = "linux")]
fn linux_keyring_entry(reference: &[u8]) -> Result<keyring::Entry, String> {
    let id = std::str::from_utf8(reference)
        .map_err(|_| "Ссылка на секрет профиля повреждена.".to_owned())?;
    uuid::Uuid::parse_str(id).map_err(|_| "Ссылка на секрет профиля повреждена.".to_owned())?;
    keyring::Entry::new("reality-client", id)
        .map_err(|_| "Не удалось открыть запись профиля в Linux Secret Service.".to_owned())
}

#[cfg(target_os = "linux")]
fn unprotect(protected: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
    let entry = linux_keyring_entry(protected)?;
    let mut secret = entry
        .get_secret()
        .map_err(|_| "Не удалось получить профиль из Linux Secret Service. Проверьте, что хранилище разблокировано.".to_owned())?;
    if secret.is_empty() || secret.len() > MAX_PROTECTED_LINK {
        secret.zeroize();
        return Err("Сохранённая ссылка профиля имеет недопустимый размер.".into());
    }
    Ok(Zeroizing::new(secret))
}

#[cfg(not(any(windows, target_os = "linux", target_os = "android")))]
fn protect(_: &[u8]) -> Result<Vec<u8>, String> {
    Err("Защищённое хранилище профилей для этой ОС ещё не подключено.".into())
}

#[cfg(windows)]
fn unprotect(protected: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
    use windows::Win32::{
        Foundation::{HLOCAL, LocalFree},
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData,
        },
    };
    if protected.is_empty() || protected.len() > MAX_PROTECTED_LINK {
        return Err("Сохранённая ссылка профиля повреждена.".into());
    }
    let input = CRYPT_INTEGER_BLOB {
        cbData: protected.len() as u32,
        pbData: protected.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(
            &input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|_| {
            "Windows не смогла расшифровать профиль для текущего пользователя.".to_owned()
        })?;
        if output.pbData.is_null() || output.cbData == 0 {
            if !output.pbData.is_null() {
                let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
            }
            return Err("Windows вернула пустую ссылку профиля.".into());
        }
        let clear = Zeroizing::new(
            std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec(),
        );
        // DPAPI allocated a second plaintext copy outside Rust's zeroizing
        // containers; wipe that native buffer before returning it to Windows.
        std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize).zeroize();
        let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
        Ok(clear)
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "android")))]
fn unprotect(_: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
    Err("Защищённое хранилище профилей для этой ОС ещё не подключено.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    struct TempDirectory(PathBuf);

    #[cfg(windows)]
    impl Drop for TempDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(windows)]
    fn dotnet_dpapi_transform(input: &Path, output: &Path, operation: &str) {
        use std::process::Command;

        const HELPER: &str = r#"
using System.IO;
using System.Security.Cryptography;
public static class RealityClientDpapiInterop {
    public static void Protect(string input, string output) {
        byte[] clear = File.ReadAllBytes(input);
        File.WriteAllBytes(output, ProtectedData.Protect(clear, null, DataProtectionScope.CurrentUser));
    }
    public static void Unprotect(string input, string output) {
        byte[] protectedBytes = File.ReadAllBytes(input);
        File.WriteAllBytes(output, ProtectedData.Unprotect(protectedBytes, null, DataProtectionScope.CurrentUser));
    }
}
"#;
        let method = match operation {
            "protect" => "Protect",
            "unprotect" => "Unprotect",
            _ => panic!("unsupported test operation"),
        };
        let script = format!(
            "$ErrorActionPreference = 'Stop'; Add-Type -TypeDefinition $env:REALITY_DPAPI_CSHARP -Language CSharp -ReferencedAssemblies ([System.Security.Cryptography.ProtectedData].Assembly.Location); [RealityClientDpapiInterop]::{method}($env:REALITY_DPAPI_INPUT, $env:REALITY_DPAPI_OUTPUT)"
        );
        let result = Command::new("pwsh.exe")
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &script,
            ])
            .env("REALITY_DPAPI_CSHARP", HELPER)
            .env("REALITY_DPAPI_INPUT", input)
            .env("REALITY_DPAPI_OUTPUT", output)
            .output()
            .expect("PowerShell 7 must be installed on the Windows test host");
        assert!(
            result.status.success(),
            "C# DPAPI {operation} failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    #[test]
    fn accepts_vless_link_with_host_port_and_userinfo() {
        assert!(validate_vless_link("vless://00000000-0000-4000-8000-000000000000@edge.example.org:443?encryption=none&security=reality").is_ok());
    }

    #[test]
    fn rejects_wrong_scheme_missing_port_and_multiline_input() {
        assert!(validate_vless_link("https://id@example.org:443").is_err());
        assert!(validate_vless_link("vless://id@example.org").is_err());
        assert!(validate_vless_link("vless://id@example.org:443\nsecret").is_err());
    }

    #[test]
    fn refuses_oversized_vault_before_decoding() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "reality-client-oversized-vault-{}-{nonce}.dat",
            std::process::id()
        ));
        std::fs::write(&path, vec![0; MAX_VAULT_SIZE + 1]).unwrap();
        let result = ProfileStore::open_at(path.clone());
        let _ = fs::remove_file(path);
        assert!(matches!(result, Err(problem) if problem.contains("2 МиБ")));
    }

    #[test]
    fn reads_dotnet_binary_writer_golden_vault() {
        // Generated with System.IO.BinaryWriter from the C# reference client:
        // header "RCLIENT1", one UTF-8 profile name, and a four-byte blob.
        let fixture = [
            0x08, 0x52, 0x43, 0x4c, 0x49, 0x45, 0x4e, 0x54, 0x31, 0x01, 0x00, 0x00, 0x00, 0x10,
            0xd0, 0x9f, 0xd1, 0x80, 0xd0, 0xbe, 0xd1, 0x84, 0xd0, 0xb8, 0xd0, 0xbb, 0xd1, 0x8c,
            0x20, 0x31, 0x04, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04,
        ];
        let decoded = decode_legacy_vault(&fixture).unwrap();
        assert_eq!(decoded[0].name, "Профиль 1");
        assert_eq!(decoded[0].protected_link, [1, 2, 3, 4]);
        assert_eq!(encode_vault(&decoded), fixture);
    }

    #[test]
    fn rejects_trailing_data_and_unreasonable_count() {
        let mut encoded = encode_vault(&[]);
        encoded.push(0);
        assert!(decode_legacy_vault(&encoded).is_err());
        assert!(
            decode_legacy_vault(&[
                8, b'R', b'C', b'L', b'I', b'E', b'N', b'T', b'1', 101, 0, 0, 0
            ])
            .is_err()
        );
    }

    #[cfg(windows)]
    #[test]
    fn dpapi_roundtrip_preserves_link_bytes() {
        let link =
            b"vless://00000000-0000-4000-8000-000000000000@edge.example.org:443?encryption=none";
        let protected = protect(link).unwrap();
        assert_ne!(protected, link);
        assert_eq!(&*unprotect(&protected).unwrap(), link);
    }

    #[cfg(windows)]
    #[test]
    fn dotnet_and_rust_dpapi_blobs_are_interoperable() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = TempDirectory(std::env::temp_dir().join(format!(
            "reality-client-dotnet-dpapi-{}-{nonce}",
            std::process::id()
        )));
        fs::create_dir(&directory.0).unwrap();

        let clear = b"non-secret C# and Rust DPAPI interoperability fixture";
        let plaintext_path = directory.0.join("plaintext.bin");
        let csharp_cipher_path = directory.0.join("csharp-protected.bin");
        let rust_cipher_path = directory.0.join("rust-protected.bin");
        let recovered_path = directory.0.join("csharp-recovered.bin");
        fs::write(&plaintext_path, clear).unwrap();

        dotnet_dpapi_transform(&plaintext_path, &csharp_cipher_path, "protect");
        let csharp_cipher = fs::read(&csharp_cipher_path).unwrap();
        assert_eq!(&*unprotect(&csharp_cipher).unwrap(), clear);

        let rust_cipher = protect(clear).unwrap();
        fs::write(&rust_cipher_path, &rust_cipher).unwrap();
        dotnet_dpapi_transform(&rust_cipher_path, &recovered_path, "unprotect");
        assert_eq!(fs::read(&recovered_path).unwrap(), clear);
    }

    #[cfg(windows)]
    #[test]
    fn profile_store_persists_replaces_and_deletes_profiles() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "reality-client-test-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("profiles.dat");
        let mut store = ProfileStore {
            path: path.clone(),
            profiles: Vec::new(),
        };
        let first =
            "vless://00000000-0000-4000-8000-000000000000@edge.example.org:443?encryption=none";
        let second =
            "vless://11111111-1111-4111-8111-111111111111@edge.example.org:8443?encryption=none";
        store.save("Example", first, None).unwrap();
        assert_eq!(store.names(), vec!["Example"]);
        assert_eq!(store.read_link(0).unwrap().as_str(), first);

        store.save("Example", second, Some(0)).unwrap();
        assert_eq!(store.len(), 1);
        assert_eq!(store.read_link(0).unwrap().as_str(), second);
        let on_disk = std::fs::read(&path).unwrap();
        let decoded = decode_legacy_vault(&on_disk).unwrap();
        assert_eq!(decoded.len(), 1);
        assert!(
            !decoded[0]
                .protected_link
                .windows(first.len())
                .any(|w| w == first.as_bytes())
        );

        store.delete(0).unwrap();
        assert!(store.names().is_empty());
        assert_eq!(
            decode_legacy_vault(&std::fs::read(&path).unwrap())
                .unwrap()
                .len(),
            0
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
