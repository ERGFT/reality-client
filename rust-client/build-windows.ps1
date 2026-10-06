# SPDX-License-Identifier: GPL-3.0-or-later
$ErrorActionPreference = 'Stop'

$msysGcc = if ($env:REALITY_MINGW_BIN) {
    $env:REALITY_MINGW_BIN
} else {
    Join-Path $env:LOCALAPPDATA 'Programs\msys64\mingw64\bin'
}
if (-not (Test-Path -LiteralPath (Join-Path $msysGcc 'gcc.exe'))) {
    throw "Не найден MinGW-w64 GCC: $msysGcc"
}
$env:PATH = "$msysGcc;$env:PATH"

$repo = Split-Path -Parent $PSScriptRoot
$core = Join-Path $repo 'third_party\reality-client.exe'
if (-not (Test-Path -LiteralPath $core)) { throw "Не найден закреплённый файл ядра: $core" }
$ffi = Join-Path $PSScriptRoot 'third_party\reality.dll'
if (-not (Test-Path -LiteralPath $ffi)) { throw "Не найдена библиотека Rust-ядра: $ffi. Сначала выполните ..\build_rust_core.ps1." }
$wintun = Join-Path $repo 'third_party\wintun\wintun.dll'
$wintunLicense = Join-Path $repo 'third_party\wintun\LICENSE.txt'
$expectedWintunSha256 = 'E5DA8447DC2C320EDC0FC52FA01885C103DE8C118481F683643CACC3220DAFCE'
if (-not (Test-Path -LiteralPath $wintun) -or -not (Test-Path -LiteralPath $wintunLicense)) {
    throw 'Не найден официальный Wintun DLL и его лицензия в third_party\wintun.'
}
if ((Get-FileHash -LiteralPath $wintun -Algorithm SHA256).Hash -ne $expectedWintunSha256) {
    throw 'SHA-256 Wintun DLL не совпадает с закреплённым официальным файлом.'
}

cargo +stable-x86_64-pc-windows-gnu build --manifest-path (Join-Path $PSScriptRoot 'Cargo.toml') --locked --release --target x86_64-pc-windows-gnu
if ($LASTEXITCODE -ne 0) { throw "Сборка Rust-клиента завершилась с кодом $LASTEXITCODE." }

$package = Join-Path $PSScriptRoot 'dist\windows-x64'
$thirdParty = Join-Path $package 'third_party'
New-Item -ItemType Directory -Force -Path $thirdParty | Out-Null
$exe = Join-Path $PSScriptRoot 'target\x86_64-pc-windows-gnu\release\reality-client-rs.exe'
Copy-Item -LiteralPath $exe -Destination (Join-Path $package 'RealityClient-Rust.exe') -Force
Copy-Item -LiteralPath $core -Destination (Join-Path $thirdParty 'reality-client.exe') -Force
Copy-Item -LiteralPath $ffi -Destination (Join-Path $package 'reality.dll') -Force
Copy-Item -LiteralPath $wintun -Destination (Join-Path $package 'wintun.dll') -Force
Copy-Item -LiteralPath $wintunLicense -Destination (Join-Path $package 'WINTUN-LICENSE.txt') -Force
Copy-Item -LiteralPath (Join-Path $repo 'LICENSE.txt') -Destination (Join-Path $package 'LICENSE.txt') -Force
Copy-Item -LiteralPath (Join-Path $repo 'THIRD_PARTY.md') -Destination (Join-Path $package 'THIRD_PARTY.md') -Force
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'PACKAGE-README.md') -Destination (Join-Path $package 'README.md') -Force
Copy-Item -LiteralPath (Join-Path $repo 'third_party\vpn-core-source.zip') -Destination (Join-Path $thirdParty 'vpn-core-source.zip') -Force
Copy-Item -LiteralPath (Join-Path $repo 'third_party\vpn-core-source.commit') -Destination (Join-Path $thirdParty 'vpn-core-source.commit') -Force

# WScript.Shell does not expose IShellLink::SetRelativePath, so its shortcuts
# keep an absolute target and break when the package folder is moved. Create a
# Shell Link directly and record the link's original location for relocation.
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

[ComImport, InterfaceType(ComInterfaceType.InterfaceIsIUnknown), Guid("000214F9-0000-0000-C000-000000000046")]
public interface IShellLinkW {
    void GetPath(IntPtr file, IntPtr data, uint dataSize, IntPtr findData, uint flags);
    void GetIDList(out IntPtr item);
    void SetIDList(IntPtr item);
    void GetDescription(IntPtr name, int maxLength);
    void SetDescription([MarshalAs(UnmanagedType.LPWStr)] string name);
    void GetWorkingDirectory(IntPtr directory, int maxLength);
    void SetWorkingDirectory([MarshalAs(UnmanagedType.LPWStr)] string directory);
    void GetArguments(IntPtr arguments, int maxLength);
    void SetArguments([MarshalAs(UnmanagedType.LPWStr)] string arguments);
    void GetHotkey(out short hotkey);
    void SetHotkey(short hotkey);
    void GetShowCmd(out int showCommand);
    void SetShowCmd(int showCommand);
    void GetIconLocation(IntPtr iconPath, int maxLength, out int iconIndex);
    void SetIconLocation([MarshalAs(UnmanagedType.LPWStr)] string iconPath, int iconIndex);
    void SetRelativePath([MarshalAs(UnmanagedType.LPWStr)] string linkPath, uint reserved);
    void Resolve(IntPtr window, uint flags);
    void SetPath([MarshalAs(UnmanagedType.LPWStr)] string target);
}

[ComImport, InterfaceType(ComInterfaceType.InterfaceIsIUnknown), Guid("0000010b-0000-0000-C000-000000000046")]
public interface IPersistFile {
    void GetClassID(out Guid classId);
    [PreserveSig] int IsDirty();
    void Load([MarshalAs(UnmanagedType.LPWStr)] string fileName, uint mode);
    void Save([MarshalAs(UnmanagedType.LPWStr)] string fileName, [MarshalAs(UnmanagedType.Bool)] bool remember);
    void SaveCompleted([MarshalAs(UnmanagedType.LPWStr)] string fileName);
    void GetCurFile([MarshalAs(UnmanagedType.LPWStr)] out string fileName);
}

public static class RealityClientShortcutBuilder {
    public static void Create(string shortcutPath, string targetPath, string workingDirectory, string description) {
        var type = Type.GetTypeFromCLSID(new Guid("00021401-0000-0000-C000-000000000046"));
        var link = (IShellLinkW)Activator.CreateInstance(type);
        link.SetPath(targetPath);
        link.SetWorkingDirectory(workingDirectory);
        link.SetDescription(description);
        link.SetRelativePath(shortcutPath, 0);
        ((IPersistFile)link).Save(shortcutPath, true);
    }
}
'@

$shortcutPath = Join-Path $package 'Reality Client Rust.lnk'
[RealityClientShortcutBuilder]::Create(
    $shortcutPath,
    (Join-Path $package 'RealityClient-Rust.exe'),
    $package,
    'Экспериментальная Rust-версия Reality Client'
)

$sha = (Get-FileHash -LiteralPath (Join-Path $package 'RealityClient-Rust.exe') -Algorithm SHA256).Hash
Write-Output 'RUST_CLIENT_BUILD=PASS'
Write-Output "PACKAGE=$package"
Write-Output "SHA256=$sha"
