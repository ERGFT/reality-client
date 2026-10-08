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

struct StagingDirectory(PathBuf);

impl Drop for StagingDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn create_staging_directory(destination: &Path) -> Result<StagingDirectory> {
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    let name = destination
        .file_name()
        .ok_or_else(|| format!("Недопустимый путь каталога: {}", destination.display()))?
        .to_string_lossy();
    for attempt in 0..32 {
        let staging = parent.join(format!(".{name}.fetch-{}-{attempt}", std::process::id()));
        match fs::create_dir(&staging) {
            Ok(()) => return Ok(StagingDirectory(staging)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("{}: {error}", staging.display())),
        }
    }
    Err(format!(
        "Не удалось создать временный каталог рядом с {}",
        destination.display()
    ))
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
    let staging = create_staging_directory(destination)?;
    let staging_path = &staging.0;
    let git = |args: &[&str]| run(Command::new("git").arg("-C").arg(staging_path).args(args));
    git(&["init", "-q"])?;
    git(&["remote", "add", "origin", &url])?;
    git(&["fetch", "-q", "--depth", "1", "origin", &revision])?;
    git(&["checkout", "-q", "FETCH_HEAD"])?;
    let actual = git_output(staging_path, &["rev-parse", "HEAD"]).unwrap_or_default();
    if actual != revision {
        return Err(format!(
            "Скачана другая версия ядра: {actual} вместо {revision}"
        ));
    }
    if !staging_path.join("Cargo.toml").is_file() {
        return Err("В скачанных исходниках нет корневого Cargo.toml ядра.".into());
    }
    if destination.exists() {
        fs::remove_dir(destination).map_err(|e| {
            format!(
                "Не удалось заменить пустой каталог {}: {e}",
                destination.display()
            )
        })?;
    }
    fs::rename(staging_path, destination).map_err(|e| {
        format!(
            "Не удалось переместить проверенное ядро в {}: {e}",
            destination.display()
        )
    })?;
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
