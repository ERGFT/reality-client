#!/usr/bin/env python3
"""Apply the Android TUN-fd ownership fix to the hash-pinned vpn-core source."""

from __future__ import annotations

import sys
from pathlib import Path


def replace_once(path: Path, old: str, new: str) -> None:
    text = path.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"Expected exactly one patch location in {path}, found {count}.")
    path.write_text(text.replace(old, new, 1), encoding="utf-8", newline="\n")


def append_once(path: Path, marker: str, addition: str) -> None:
    text = path.read_text(encoding="utf-8")
    if marker in text:
        raise SystemExit(f"Patch marker already exists in {path}; refusing a second application.")
    path.write_text(text.rstrip() + "\n\n" + addition.rstrip() + "\n", encoding="utf-8", newline="\n")


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("Usage: apply_core_tun_fd_ownership.py <extracted-vpn-core-source>")
    root = Path(sys.argv[1]).resolve()
    if not (root / "ffi/src/lib.rs").is_file() or not (root / "core/src/app/tun/mod.rs").is_file():
        raise SystemExit(f"Not a vpn-core source root: {root}")

    config = root / "core/src/app/config/mod.rs"
    replace_once(
        config,
        "use std::net::SocketAddr;\n",
        "use std::net::SocketAddr;\n#[cfg(unix)]\nuse std::os::fd::OwnedFd;\n",
    )
    replace_once(
        config,
        "use std::path::{Path, PathBuf};\n",
        "use std::path::{Path, PathBuf};\n#[cfg(unix)]\nuse std::sync::Arc;\n",
    )
    replace_once(
        config,
        "    pub tun_fd: Option<i32>,\n",
        "    pub tun_fd: Option<i32>,\n"
        "    /// Keeps a system-provided TUN descriptor alive across config parsing and startup.\n"
        "    #[cfg(unix)]\n"
        "    pub tun_fd_owner: Option<Arc<OwnedFd>>,\n",
    )
    replace_once(
        config,
        "            tun_fd: None,\n",
        "            tun_fd: None,\n"
        "            #[cfg(unix)]\n"
        "            tun_fd_owner: None,\n",
    )
    replace_once(
        root / "bin/client/src/main.rs",
        "            tun_fd: None,\n        }],\n",
        "            tun_fd: None,\n"
        "            #[cfg(unix)]\n"
        "            tun_fd_owner: None,\n"
        "        }],\n",
    )

    tun = root / "core/src/app/tun/mod.rs"
    replace_once(
        tun,
        "    pub fd: Option<i32>,\n",
        "    pub fd: Option<i32>,\n"
        "    /// Shared owner retained until the operating-system device duplicates the descriptor.\n"
        "    #[cfg(unix)]\n"
        "    pub fd_owner: Option<Arc<std::os::fd::OwnedFd>>,\n",
    )
    replace_once(
        tun,
        "        if let Some(fd) = s.fd {\n            return self.device_from_fd(fd);\n        }\n",
        "        if let Some(fd) = s.fd {\n"
        "            #[cfg(unix)]\n"
        "            if let Some(owner) = &s.fd_owner {\n"
                "                use std::os::fd::IntoRawFd;\n"
                "                let duplicate = owner.try_clone().map_err(|e| {\n"
                "                    Error::Config(format!(\n"
                "                        \"tun: не удалось дублировать системный дескриптор: {e}\"\n"
                "                    ))\n"
        "                })?;\n"
        "                return self.device_from_fd(duplicate.into_raw_fd());\n"
        "            }\n"
        "            return self.device_from_fd(fd);\n"
        "        }\n",
    )
    replace_once(
        tun,
        "        fd: i.tun_fd,\n",
        "        fd: i.tun_fd,\n"
        "        #[cfg(unix)]\n"
        "        fd_owner: i.tun_fd_owner.clone(),\n",
    )

    ffi = root / "ffi/src/lib.rs"
    replace_once(
        ffi,
        "use std::ffi::{c_char, c_int, c_void, CStr, CString};\n",
        "use std::ffi::{c_char, c_int, c_void, CStr, CString};\n"
        "#[cfg(unix)]\n"
        "use std::os::fd::{FromRawFd, OwnedFd};\n",
    )
    replace_once(
        ffi,
        "use tokio::sync::broadcast::error::RecvError;\n",
        "use tokio::sync::broadcast::error::RecvError;\n\n"
        "#[cfg(unix)]\n"
        "type FfiTunFd = Arc<OwnedFd>;\n"
        "#[cfg(not(unix))]\n"
        "type FfiTunFd = i32;\n",
    )
    replace_once(ffi, "    tun_fd: Option<i32>,\n", "    tun_fd: Option<FfiTunFd>,\n")
    replace_once(
        ffi,
        "fn parse_config(text: &str, base: &std::path::Path, tun_fd: Option<i32>) -> Result<Config, String> {\n"
        "    let mut cfg = Config::parse_at(text, base).map_err(|e| e.to_string())?;\n"
        "    if let Some(fd) = tun_fd {\n"
        "        let tun = cfg\n"
        "            .inbounds\n"
        "            .iter_mut()\n"
        "            .find(|i| i.kind == InboundKind::Tun)\n"
        "            .ok_or(\"дескриптор TUN передан, но в настройках нет входа tun\")?;\n"
        "        tun.tun_fd = Some(fd);\n"
        "    }\n"
        "    Ok(cfg)\n"
        "}\n",
        "fn parse_config(\n"
        "    text: &str,\n"
        "    base: &std::path::Path,\n"
        "    tun_fd: Option<FfiTunFd>,\n"
        ") -> Result<Config, String> {\n"
        "    #[cfg(unix)]\n"
        "    let raw_fd = tun_fd.as_ref().map(|fd| {\n"
        "        use std::os::fd::AsRawFd;\n"
        "        fd.as_raw_fd()\n"
        "    });\n"
        "    #[cfg(not(unix))]\n"
        "    let raw_fd = tun_fd;\n"
        "    let mut cfg = Config::parse_at(text, base).map_err(|e| e.to_string())?;\n"
        "    if let Some(fd) = raw_fd {\n"
        "        let tun = cfg\n"
        "            .inbounds\n"
        "            .iter_mut()\n"
        "            .find(|i| i.kind == InboundKind::Tun)\n"
        "            .ok_or(\"дескриптор TUN передан, но в настройках нет входа tun\")?;\n"
        "        tun.tun_fd = Some(fd);\n"
        "        #[cfg(unix)]\n"
        "        {\n"
        "            tun.tun_fd_owner = tun_fd.clone();\n"
        "        }\n"
        "    }\n"
        "    Ok(cfg)\n"
        "}\n",
    )
    replace_once(
        ffi,
        "    let r = catch_unwind(AssertUnwindSafe(|| -> Result<RcCore, String> {\n"
        "        init_logging();\n"
        "        // SAFETY: обещание вызывающего (см. выше).\n"
        "        let text = unsafe { arg(config) }?.ok_or(\"config — NULL\")?;\n"
        "        let base = PathBuf::from(unsafe { arg(base_dir) }?.unwrap_or(\".\"));\n"
        "        let fd = (tun_fd >= 0).then_some(tun_fd);\n"
        "        let cfg = parse_config(text, &base, fd)?;\n",
        "    // The FFI takes ownership immediately; the guard closes the descriptor on every error path.\n"
        "    #[cfg(unix)]\n"
        "    let fd = (tun_fd >= 0).then(|| Arc::new(unsafe { OwnedFd::from_raw_fd(tun_fd) }));\n"
        "    #[cfg(not(unix))]\n"
        "    let fd = (tun_fd >= 0).then_some(tun_fd);\n"
        "    let r = catch_unwind(AssertUnwindSafe(|| -> Result<RcCore, String> {\n"
        "        init_logging();\n"
        "        // SAFETY: обещание вызывающего (см. выше).\n"
        "        let text = unsafe { arg(config) }?.ok_or(\"config — NULL\")?;\n"
        "        let base = PathBuf::from(unsafe { arg(base_dir) }?.unwrap_or(\".\"));\n"
        "        let cfg = parse_config(text, &base, fd.clone())?;\n",
    )
    replace_once(
        ffi,
        "        let cfg = parse_config(text, &core.base, core.tun_fd)?;\n",
        "        let cfg = parse_config(text, &core.base, core.tun_fd.clone())?;\n",
    )
    replace_once(
        root / "ffi/include/reality.h",
        " *   tun_fd   — дескриптор TUN от системы (Android: ParcelFileDescriptor\n"
        " *              .detachFd() — владение переходит ядру) или -1; с дескриптором\n",
        " *   tun_fd   — дескриптор TUN от системы (Android: ParcelFileDescriptor\n"
        " *              .detachFd() — владение переходит ядру при вызове; при ошибке\n"
        " *              rc_start ядро закроет его само) или -1; с дескриптором\n",
    )
    replace_once(
        root / "docs/LIBRARY.md",
        "3. `rc_start(config, filesDir, pfd.detachFd(), &err)` — владение\n"
        "   дескриптором переходит ядру (он закроется в `rc_stop`). В настройках —\n",
        "3. `rc_start(config, filesDir, pfd.detachFd(), &err)` — владение\n"
        "   дескриптором переходит ядру при вызове: ядро закроет его и при ошибке\n"
        "   запуска, и в `rc_stop` после успешного запуска. В настройках —\n",
    )
    replace_once(
        root / "docs/LIBRARY.en.md",
        "   - ownership of the descriptor passes to the core (it is closed in\n"
        "     `rc_stop`);\n",
        "   - ownership passes to the core on call; it closes the descriptor on\n"
        "     startup failure and in `rc_stop` after a successful start;\n",
    )

    append_once(
        ffi,
        "rc_start_closes_system_tun_fd_when_config_parse_fails",
        "#[cfg(all(test, target_os = \"linux\"))]\n"
        "mod tun_fd_ownership_tests {\n"
        "    use super::rc_start;\n"
        "    use std::{ffi::CString, fs::File, os::fd::IntoRawFd, path::PathBuf};\n\n"
        "    #[test]\n"
        "    fn rc_start_closes_system_tun_fd_when_config_parse_fails() {\n"
        "        let fd = File::open(\"/dev/null\").unwrap().into_raw_fd();\n"
        "        let config = CString::new(\"{invalid json\").unwrap();\n"
        "        let base = CString::new(\".\").unwrap();\n"
        "        let mut error = std::ptr::null_mut();\n"
        "        let core = unsafe { rc_start(config.as_ptr(), base.as_ptr(), fd, &mut error) };\n"
        "        assert!(core.is_null());\n"
        "        assert!(!error.is_null());\n"
        "        let open = PathBuf::from(format!(\"/proc/self/fd/{fd}\")).exists();\n"
        "        if open {\n"
        "            unsafe extern \"C\" {\n"
        "                fn close(fd: i32) -> i32;\n"
        "            }\n"
        "            unsafe {\n"
        "                let _ = close(fd);\n"
        "            }\n"
        "        }\n"
        "        assert!(\n"
        "            !open,\n"
        "            \"rc_start leaked the TUN descriptor on a parse failure\"\n"
        "        );\n"
        "        unsafe { super::rc_free_string(error) };\n"
        "    }\n"
        "}\n",
    )

    print("CORE_TUN_FD_OWNERSHIP_PATCH=PASS")


if __name__ == "__main__":
    main()
