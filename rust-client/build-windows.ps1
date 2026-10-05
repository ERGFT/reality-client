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

cargo +stable-x86_64-pc-windows-gnu build --manifest-path (Join-Path $PSScriptRoot 'Cargo.toml') --locked --release --target x86_64-pc-windows-gnu
if ($LASTEXITCODE -ne 0) { throw "Сборка Rust-клиента завершилась с кодом $LASTEXITCODE." }

$package = Join-Path $PSScriptRoot 'dist\windows-x64'
$thirdParty = Join-Path $package 'third_party'
New-Item -ItemType Directory -Force -Path $thirdParty | Out-Null
$exe = Join-Path $PSScriptRoot 'target\x86_64-pc-windows-gnu\release\reality-client-rs.exe'
Copy-Item -LiteralPath $exe -Destination (Join-Path $package 'RealityClient-Rust.exe') -Force
Copy-Item -LiteralPath $core -Destination (Join-Path $thirdParty 'reality-client.exe') -Force
Copy-Item -LiteralPath $ffi -Destination (Join-Path $package 'reality.dll') -Force
Copy-Item -LiteralPath (Join-Path $repo 'LICENSE.txt') -Destination (Join-Path $package 'LICENSE.txt') -Force
Copy-Item -LiteralPath (Join-Path $repo 'THIRD_PARTY.md') -Destination (Join-Path $package 'THIRD_PARTY.md') -Force
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'README.md') -Destination (Join-Path $package 'README.md') -Force
Copy-Item -LiteralPath (Join-Path $repo 'third_party\vpn-core-source.zip') -Destination (Join-Path $thirdParty 'vpn-core-source.zip') -Force
Copy-Item -LiteralPath (Join-Path $repo 'third_party\vpn-core-source.commit') -Destination (Join-Path $thirdParty 'vpn-core-source.commit') -Force

$shell = New-Object -ComObject WScript.Shell
$shortcut = $shell.CreateShortcut((Join-Path $package 'Reality Client Rust.lnk'))
$shortcut.TargetPath = Join-Path $package 'RealityClient-Rust.exe'
$shortcut.WorkingDirectory = $package
$shortcut.Description = 'Экспериментальная Rust-версия Reality Client'
$shortcut.Save()

$sha = (Get-FileHash -LiteralPath (Join-Path $package 'RealityClient-Rust.exe') -Algorithm SHA256).Hash
Write-Output 'RUST_CLIENT_BUILD=PASS'
Write-Output "PACKAGE=$package"
Write-Output "SHA256=$sha"
