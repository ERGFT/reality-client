# SPDX-License-Identifier: GPL-3.0-or-later
$ErrorActionPreference = 'Stop'

$archive = Join-Path $PSScriptRoot 'third_party\vpn-core-source.zip'
$revisionFile = Join-Path $PSScriptRoot 'third_party\vpn-core-source.commit'
$expectedCommit = 'ee68039943ebb2aaf3287bf622ae34c18bfa0cae'
$expectedSha256 = 'DF789EABC39029A403D72BA78367637E4347D372626A5FC79F35EC901AF1317D'
if (-not (Test-Path -LiteralPath $archive)) { throw "Не найден архив исходников ядра: $archive" }
if ((Get-Content -LiteralPath $revisionFile -Raw).Trim() -ne $expectedCommit) { throw 'Ревизия архива исходников не совпадает с закреплённой.' }
if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne $expectedSha256) { throw 'SHA-256 архива исходников не совпадает с закреплённым значением.' }

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
$env:CARGO_TARGET_DIR = Join-Path $env:TEMP 'reality-client-ffi-target-ee680399'

$sourceRoot = Join-Path $env:TEMP ('reality-client-ffi-source-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $sourceRoot | Out-Null
try {
    Expand-Archive -LiteralPath $archive -DestinationPath $sourceRoot
    if (-not (Test-Path -LiteralPath (Join-Path $sourceRoot 'Cargo.toml'))) { throw 'В архиве не найден корневой Cargo.toml ядра.' }
    $corePatch = Join-Path $PSScriptRoot 'rust-client\patches\apply_core_tun_fd_ownership.py'
    & python $corePatch $sourceRoot
    if ($LASTEXITCODE -ne 0) { throw "Не удалось применить проверенную правку владения Android TUN-дескриптором (код $LASTEXITCODE)." }
    Push-Location $sourceRoot
    try {
        & cargo build --locked --release -p reality-ffi
        if ($LASTEXITCODE -ne 0) { throw "Сборка FFI-библиотеки завершилась с кодом $LASTEXITCODE." }
    }
    finally { Pop-Location }

    $builtLibrary = Join-Path $env:CARGO_TARGET_DIR 'release\reality.dll'
    if (-not (Test-Path -LiteralPath $builtLibrary)) { throw "Cargo не создал ожидаемую библиотеку: $builtLibrary" }
    $outputDirectory = Join-Path $PSScriptRoot 'rust-client\third_party'
    New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null
    $outputLibrary = Join-Path $outputDirectory 'reality.dll'
    Copy-Item -LiteralPath $builtLibrary -Destination $outputLibrary -Force
    $sha256 = (Get-FileHash -LiteralPath $outputLibrary -Algorithm SHA256).Hash
    Write-Output "CORE_SOURCE_COMMIT=$expectedCommit"
    Write-Output "CORE_FFI_DLL=$outputLibrary"
    Write-Output "CORE_FFI_SHA256=$sha256"
}
finally {
    $tempRoot = [IO.Path]::GetFullPath($env:TEMP).TrimEnd('\') + '\'
    $resolvedSource = [IO.Path]::GetFullPath($sourceRoot)
    if (-not $resolvedSource.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase)) { throw 'Путь временных исходников оказался вне TEMP; очистка отменена.' }
    Remove-Item -LiteralPath $resolvedSource -Recurse -Force
}
