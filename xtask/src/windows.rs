// SPDX-License-Identifier: GPL-3.0-or-later
//! Пакет Windows x64 и тесты на GNU-тулчейне (замена `build_rust_core.ps1`,
//! `rust-client/build-windows.ps1` и `rust-client/tests/windows-test.ps1`).

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use sha2::{Digest, Sha256};

use crate::{Result, fetch, repo_root, run};

const TARGET: &str = "x86_64-pc-windows-gnu";
const TOOLCHAIN: &str = "stable-x86_64-pc-windows-gnu";
const WINTUN_SHA256: &str = "E5DA8447DC2C320EDC0FC52FA01885C103DE8C118481F683643CACC3220DAFCE";

/// Папка `bin` MinGW-w64 (MSYS2): `REALITY_MINGW_BIN` или путь по умолчанию.
fn mingw_bin() -> Result<PathBuf> {
    let bin = match std::env::var_os("REALITY_MINGW_BIN") {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(
            std::env::var_os("LOCALAPPDATA")
                .ok_or("LOCALAPPDATA не задан; укажите REALITY_MINGW_BIN")?,
        )
        .join("Programs/msys64/mingw64/bin"),
    };
    if bin.join("gcc.exe").is_file() {
        Ok(bin)
    } else {
        Err(format!("Не найден MinGW-w64 GCC: {}", bin.display()))
    }
}

fn with_mingw_path(bin: &Path) -> std::ffi::OsString {
    let mut paths = vec![bin.to_path_buf()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    std::env::join_paths(paths).expect("PATH из допустимых путей")
}

/// Окружение сборки ядра и клиента: GNU-тулчейн, статическая линковка, aws-lc через CMake + Ninja.
fn build_env(command: &mut Command, bin: &Path) {
    command
        .env("PATH", with_mingw_path(bin))
        .env("RUSTUP_TOOLCHAIN", TOOLCHAIN)
        .env("RUSTFLAGS", "-C link-arg=-static")
        .env("AWS_LC_SYS_PREBUILT_NASM", "1")
        .env("CMAKE_GENERATOR", "Ninja");
}

fn sha256_hex(path: &Path) -> Result<String> {
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(format!("{:X}", Sha256::digest(bytes)))
}

fn copy(from: &Path, to: &Path) -> Result<()> {
    fs::copy(from, to).map(|_| ()).map_err(|e| {
        format!(
            "не удалось скопировать {} → {}: {e}",
            from.display(),
            to.display()
        )
    })
}

/// Консольная программа ядра `reality-client.exe` → `rust-client/third_party/` (рядом с ней её ищет клиент и тесты).
/// Само ядро — Cargo-зависимость клиента (third_party/vpn-core) и собирается вместе с ним.
pub fn core_cli() -> Result<PathBuf> {
    let root = repo_root();
    let bin = mingw_bin()?;
    let revision = fetch::revision()?;
    let core = fetch::run_default()?;
    let core_target =
        std::env::temp_dir().join(format!("reality-client-core-target-{}", &revision[..8]));
    let mut cli = Command::new("cargo");
    cli.current_dir(&core)
        .args(["build", "--locked", "--release", "-p", "reality-client"])
        .env("CARGO_TARGET_DIR", &core_target);
    build_env(&mut cli, &bin);
    run(&mut cli)?;
    let client_third_party = root.join("rust-client/third_party");
    fs::create_dir_all(&client_third_party).map_err(|e| e.to_string())?;
    let core_cli = client_third_party.join("reality-client.exe");
    copy(&core_target.join("release/reality-client.exe"), &core_cli)?;
    println!("CORE_SOURCE_COMMIT={revision}");
    println!("CORE_CLI_SHA256={}", sha256_hex(&core_cli)?);
    Ok(core_cli)
}

pub fn package() -> Result<()> {
    let root = repo_root();
    let bin = mingw_bin()?;
    let revision = fetch::revision()?;
    let core_cli = core_cli()?;

    let wintun_dir = root.join("third_party/wintun");
    let wintun = wintun_dir.join("wintun.dll");
    let wintun_license = wintun_dir.join("LICENSE.txt");
    if !wintun.is_file() || !wintun_license.is_file() {
        return Err("Не найден официальный Wintun DLL и его лицензия в third_party/wintun.".into());
    }
    if sha256_hex(&wintun)? != WINTUN_SHA256 {
        return Err("SHA-256 Wintun DLL не совпадает с закреплённым официальным файлом.".into());
    }

    let mut client = Command::new("cargo");
    client
        .args(["build", "--manifest-path"])
        .arg(root.join("rust-client/Cargo.toml"))
        .args(["--locked", "--release", "--target", TARGET]);
    build_env(&mut client, &bin);
    run(&mut client)?;

    let package = root.join("rust-client/dist/windows-x64");
    if package.exists() {
        fs::remove_dir_all(&package).map_err(|e| e.to_string())?;
    }
    let package_third_party = package.join("third_party");
    fs::create_dir_all(&package_third_party).map_err(|e| e.to_string())?;
    let exe = package.join("RealityClient-Rust.exe");
    let client_target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .map(|path| -> Result<PathBuf> {
            if path.is_absolute() {
                Ok(path)
            } else {
                std::env::current_dir()
                    .map(|cwd| cwd.join(path))
                    .map_err(|e| {
                        format!("не удалось определить текущую папку для CARGO_TARGET_DIR: {e}")
                    })
            }
        })
        .transpose()?
        .unwrap_or_else(|| root.join("rust-client/target"));
    copy(
        &client_target.join(format!("{TARGET}/release/reality-client-rs.exe")),
        &exe,
    )?;
    copy(&core_cli, &package_third_party.join("reality-client.exe"))?;
    copy(&wintun, &package.join("wintun.dll"))?;
    copy(&wintun_license, &package.join("WINTUN-LICENSE.txt"))?;
    copy(&root.join("LICENSE.txt"), &package.join("LICENSE.txt"))?;
    copy(
        &root.join("THIRD_PARTY.md"),
        &package.join("THIRD_PARTY.md"),
    )?;
    copy(
        &root.join("rust-client/PACKAGE-README.md"),
        &package.join("README.md"),
    )?;
    // Исходники ядра (GPL): репозиторий и точный коммит, из которого собраны ядро внутри
    // RealityClient-Rust.exe и reality-client.exe.
    fs::write(
        package.join("CORE-SOURCE.txt"),
        format!(
            "Reality Core source: https://github.com/ERGFT/vpn-core\nCommit: {revision}\nArchive: https://github.com/ERGFT/vpn-core/archive/{revision}.zip\n"
        ),
    )
    .map_err(|e| e.to_string())?;
    shortcut::create(
        &package.join("Reality Client Rust.lnk"),
        &exe,
        &package,
        "Экспериментальная Rust-версия Reality Client",
    )?;

    println!("RUST_CLIENT_BUILD=PASS");
    println!("PACKAGE={}", package.display());
    println!("SHA256={}", sha256_hex(&exe)?);
    Ok(())
}

/// Тесты и Clippy клиента на GNU-тулчейне. `shlwapi.dll` нужен линкеру `windows`-крейта,
/// а библиотеки импорта в MinGW для него нет, поэтому собираем её `dlltool` из `shlwapi.def`.
pub fn test() -> Result<()> {
    let root = repo_root();
    let bin = mingw_bin()?;
    let dlltool = bin.join("dlltool.exe");
    if !dlltool.is_file() {
        return Err(format!(
            "MinGW-w64 dlltool.exe не найден: {}. Укажите REALITY_MINGW_BIN.",
            dlltool.display()
        ));
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let lib_dir = std::env::temp_dir().join(format!(
        "reality-client-test-importlib-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&lib_dir).map_err(|e| e.to_string())?;
    let result = (|| {
        run(Command::new(&dlltool)
            .args(["--machine", "i386:x86-64", "--input-def"])
            .arg(root.join("rust-client/tests/windows/shlwapi.def"))
            .args(["--dllname", "shlwapi.dll", "--output-lib"])
            .arg(lib_dir.join("libshlwapi.a")))?;
        let mut library_path = vec![lib_dir.clone()];
        library_path.extend(std::env::split_paths(
            &std::env::var_os("LIBRARY_PATH").unwrap_or_default(),
        ));
        let library_path = std::env::join_paths(library_path).map_err(|e| e.to_string())?;
        let cargo = |subcommand: &str, args: &[&str]| -> Result<()> {
            let mut command = Command::new("cargo");
            command
                .current_dir(root.join("rust-client"))
                .env("PATH", with_mingw_path(&bin))
                .env("LIBRARY_PATH", &library_path)
                .env("RUSTUP_TOOLCHAIN", TOOLCHAIN)
                .env("AWS_LC_SYS_PREBUILT_NASM", "1")
                .env("CMAKE_GENERATOR", "Ninja")
                .arg(subcommand)
                .args(["--locked", "--release", "--target", TARGET])
                .args(args);
            run(&mut command)
        };
        cargo("test", &[])?;
        cargo("test", &["--features", "android-bridge-check"])?;
        cargo(
            "clippy",
            &["--all-targets", "--", "-D", "warnings", "-A", "dead_code"],
        )?;
        cargo(
            "clippy",
            &[
                "--all-targets",
                "--features",
                "android-bridge-check",
                "--",
                "-D",
                "warnings",
                "-A",
                "dead_code",
            ],
        )
    })();
    let _ = fs::remove_dir_all(&lib_dir);
    result
}

#[cfg(windows)]
mod shortcut {
    use std::path::Path;

    use windows::{
        Win32::{
            System::Com::{
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
                IPersistFile,
            },
            UI::Shell::{IShellLinkW, ShellLink},
        },
        core::{HSTRING, Interface},
    };

    use crate::Result;

    /// Ярлык с относительным путём: `WScript.Shell` его не умеет, и такой ярлык ломается при
    /// переносе папки пакета. `SetRelativePath` записывает исходное положение ярлыка для поиска.
    pub fn create(
        shortcut: &Path,
        target: &Path,
        working_dir: &Path,
        description: &str,
    ) -> Result<()> {
        let to_error = |e: windows::core::Error| format!("не удалось создать ярлык: {e}");
        // SAFETY: обычный вызов COM из одного потока; интерфейсы живут до конца функции.
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED)
                .ok()
                .map_err(to_error)?;
            let link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(to_error)?;
            link.SetPath(&HSTRING::from(target.as_os_str()))
                .map_err(to_error)?;
            link.SetWorkingDirectory(&HSTRING::from(working_dir.as_os_str()))
                .map_err(to_error)?;
            link.SetDescription(&HSTRING::from(description))
                .map_err(to_error)?;
            link.SetRelativePath(&HSTRING::from(shortcut.as_os_str()), 0)
                .map_err(to_error)?;
            let file: IPersistFile = link.cast().map_err(to_error)?;
            file.Save(&HSTRING::from(shortcut.as_os_str()), true)
                .map_err(to_error)
        }
    }
}

#[cfg(not(windows))]
mod shortcut {
    use std::path::Path;

    use crate::Result;

    pub fn create(_: &Path, _: &Path, _: &Path, _: &str) -> Result<()> {
        Err("Ярлык .lnk создаётся только на Windows; пакет Windows собирают на Windows.".into())
    }
}
