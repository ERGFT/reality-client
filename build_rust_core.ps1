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

$sourceRoot = Join-Path $env:TEMP ('reality-client-ffi-source-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $sourceRoot | Out-Null
try {
    & (Join-Path $PSScriptRoot 'scripts\fetch-core.ps1') -Destination $sourceRoot
    Push-Location $sourceRoot
    try {
        & cargo build --locked --release -p reality-ffi -p reality-client
        if ($LASTEXITCODE -ne 0) { throw "Сборка FFI-библиотеки завершилась с кодом $LASTEXITCODE." }
    }
    finally { Pop-Location }

    $builtLibrary = Join-Path $env:CARGO_TARGET_DIR 'release\reality.dll'
    if (-not (Test-Path -LiteralPath $builtLibrary)) { throw "Cargo не создал ожидаемую библиотеку: $builtLibrary" }
    $builtCli = Join-Path $env:CARGO_TARGET_DIR 'release\reality-client.exe'
    if (-not (Test-Path -LiteralPath $builtCli)) { throw "Cargo не создал ожидаемую программу: $builtCli" }
    $outputDirectory = Join-Path $PSScriptRoot 'rust-client\third_party'
    New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null
    $outputLibrary = Join-Path $outputDirectory 'reality.dll'
    Copy-Item -LiteralPath $builtLibrary -Destination $outputLibrary -Force
    Copy-Item -LiteralPath $builtCli -Destination (Join-Path $outputDirectory 'reality-client.exe') -Force
    $sha256 = (Get-FileHash -LiteralPath $outputLibrary -Algorithm SHA256).Hash
    Write-Output "CORE_SOURCE_COMMIT=$coreRevision"
    Write-Output "CORE_FFI_DLL=$outputLibrary"
    Write-Output "CORE_FFI_SHA256=$sha256"
}
finally {
    $tempRoot = [IO.Path]::GetFullPath($env:TEMP).TrimEnd('\') + '\'
    $resolvedSource = [IO.Path]::GetFullPath($sourceRoot)
    if (-not $resolvedSource.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase)) { throw 'Путь временных исходников оказался вне TEMP; очистка отменена.' }
    Remove-Item -LiteralPath $resolvedSource -Recurse -Force
}
