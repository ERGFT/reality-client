// SPDX-License-Identifier: GPL-3.0-or-later
//! Скачивание исходников vpn-core по хешу коммита (замена `scripts/fetch-core.{sh,ps1}`).
//! Хеш коммита сам гарантирует содержимое, отдельная контрольная сумма не нужна.
//! `REALITY_CORE_URL` подменяет адрес репозитория (например, на локальную копию).

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use crate::{Result, repo_root, run};

const DEFAULT_URL: &str = "https://github.com/ERGFT/vpn-core.git";

pub fn revision() -> Result<String> {
    let path = repo_root().join("third_party/vpn-core.rev");
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let revision = text.trim().to_owned();
    if revision.len() == 40
        && revision
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        Ok(revision)
    } else {
        Err(format!(
            "third_party/vpn-core.rev должен содержать полный хеш коммита, получено: '{revision}'"
        ))
    }
}

fn git_output(dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub fn run_fetch(destination: &Path) -> Result<()> {
    let revision = revision()?;
    let url = std::env::var("REALITY_CORE_URL").unwrap_or_else(|_| DEFAULT_URL.to_owned());
    let non_empty = destination
        .read_dir()
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false);
    if non_empty {
        // Уже скачанное ядро нужной версии не трогаем.
        let clean =
            git_output(destination, &["status", "--porcelain"]).is_some_and(|s| s.is_empty());
        if clean && git_output(destination, &["rev-parse", "HEAD"]).as_deref() == Some(&revision) {
            println!("CORE_SOURCE_COMMIT={revision}");
            return Ok(());
        }
        return Err(format!(
            "Каталог не пуст и содержит не ту версию ядра: {}",
            destination.display()
        ));
    }
    fs::create_dir_all(destination).map_err(|e| e.to_string())?;
    let git = |args: &[&str]| run(Command::new("git").arg("-C").arg(destination).args(args));
    git(&["init", "-q"])?;
    git(&["remote", "add", "origin", &url])?;
    git(&["fetch", "-q", "--depth", "1", "origin", &revision])?;
    git(&["checkout", "-q", "FETCH_HEAD"])?;
    let actual = git_output(destination, &["rev-parse", "HEAD"]).unwrap_or_default();
    if actual != revision {
        return Err(format!(
            "Скачана другая версия ядра: {actual} вместо {revision}"
        ));
    }
    if !destination.join("Cargo.toml").is_file() {
        return Err("В скачанных исходниках нет корневого Cargo.toml ядра.".into());
    }
    println!("CORE_SOURCE_COMMIT={revision}");
    Ok(())
}

pub fn run_default() -> Result<PathBuf> {
    let destination = repo_root().join("third_party/vpn-core");
    run_fetch(&destination)?;
    Ok(destination)
}

pub fn command(destination: Option<PathBuf>) -> Result<()> {
    match destination {
        Some(path) => run_fetch(&path),
        None => run_default().map(|_| ()),
    }
}
