// SPDX-License-Identifier: GPL-3.0-or-later
//! `cargo xtask <команда>` — сборочные задачи вместо PowerShell-скриптов.
//!
//! Команды:
//! - `fetch-core [каталог]` — скачать vpn-core ровно по хешу из `third_party/vpn-core.rev`
//!   (по умолчанию в `third_party/vpn-core`); чистую копию нужной версии не трогает.
//! - `core-cli` — собрать консольную программу ядра `reality-client.exe` в `rust-client/third_party/`.
//! - `package-windows` — ядро, консольная программа ядра, клиент и пакет Windows x64.
//! - `test-windows` — тесты и Clippy клиента на GNU-тулчейне без MSVC `link.exe`.

mod fetch;
mod windows;

use std::{
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

pub type Result<T> = std::result::Result<T, String>;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_default();
    let rest: Vec<String> = args.collect();
    let result = match command.as_str() {
        "fetch-core" => fetch::command(rest.first().map(PathBuf::from)),
        "core-cli" => windows::core_cli().map(|_| ()),
        "package-windows" => windows::package(),
        "test-windows" => windows::test(),
        _ => {
            eprintln!(
                "Использование: cargo xtask <fetch-core [каталог] | core-cli | package-windows | test-windows>"
            );
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(problem) => {
            eprintln!("xtask: {problem}");
            ExitCode::FAILURE
        }
    }
}

/// Корень репозитория (`xtask/..`).
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask лежит в корне репозитория")
        .to_path_buf()
}

/// Запускает команду и требует код 0.
pub fn run(command: &mut Command) -> Result<()> {
    let status = command
        .status()
        .map_err(|e| format!("не удалось запустить {command:?}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{command:?} завершилась с кодом {status}"))
    }
}
