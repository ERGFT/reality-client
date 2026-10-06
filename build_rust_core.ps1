# SPDX-License-Identifier: GPL-3.0-or-later
$ErrorActionPreference = 'Stop'

$coreRevision = (Get-Content -LiteralPath (Join-Path $PSScriptRoot 'third_party\vpn-core.rev') -Raw).Trim()

$msys = if ($env:REALITY_MINGW_BIN) {
    $env:REALITY_MINGW_BIN
} else {
    Join-Path $env:LOCALAPPDATA 'Programs\msys64\mingw64\bin'
}
if (-not (Test-Path -LiteralPath (Join-Path $msys 'gcc.exe'))) { throw "Не найден MinGW-w64 GCC: $msys" }
$env:PATH = "$env:USERPROFILE\.cargo\bin;$msys;$env:PATH"
$env:RUSTUP_TOOLCHAIN = 'stable-x86_64-pc-windows-gnu'
$env:RUSTFLAGS = '-C link-arg=-static'
$env:AWS_LC_SYS_PREBUILT_NASM = '1'
$env:CMAKE_GENERATOR = 'Ninja'
$env:CARGO_TARGET_DIR = Join-Path $env:TEMP ('reality-client-core-target-' + $coreRevision.Substring(0, 8))

# Ядро — Cargo-зависимость клиента (path-зависимость на third_party\vpn-core);
# отдельно собирается только его консольная программа reality-client.exe.
$sourceRoot = Join-Path $PSScriptRoot 'third_party\vpn-core'
& (Join-Path $PSScriptRoot 'scripts\fetch-core.ps1') -Destination $sourceRoot
Push-Location $sourceRoot
try {
    & cargo build --locked --release -p reality-client
    if ($LASTEXITCODE -ne 0) { throw "Сборка reality-client завершилась с кодом $LASTEXITCODE." }
}
finally { Pop-Location }

$builtCli = Join-Path $env:CARGO_TARGET_DIR 'release\reality-client.exe'
if (-not (Test-Path -LiteralPath $builtCli)) { throw "Cargo не создал ожидаемую программу: $builtCli" }
$outputDirectory = Join-Path $PSScriptRoot 'rust-client\third_party'
New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null
$outputCli = Join-Path $outputDirectory 'reality-client.exe'
Copy-Item -LiteralPath $builtCli -Destination $outputCli -Force
$sha256 = (Get-FileHash -LiteralPath $outputCli -Algorithm SHA256).Hash
Write-Output "CORE_SOURCE_COMMIT=$coreRevision"
Write-Output "CORE_CLI_EXE=$outputCli"
Write-Output "CORE_CLI_SHA256=$sha256"
