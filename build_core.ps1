# SPDX-License-Identifier: GPL-3.0-or-later
$ErrorActionPreference = 'Stop'
$expectedCommit = (Get-Content -LiteralPath (Join-Path $PSScriptRoot 'third_party\vpn-core.rev') -Raw).Trim()
$sourceRoot = Join-Path $env:TEMP ('reality-client-source-' + [Guid]::NewGuid().ToString('N'))
& cargo xtask fetch-core $sourceRoot
$source = $sourceRoot

$msys = Join-Path $env:LOCALAPPDATA 'Programs\msys64'
$mingwBin = Join-Path $msys 'mingw64\bin'
$gcc = Join-Path $mingwBin 'x86_64-w64-mingw32-gcc.exe'
$nasm = Join-Path $mingwBin 'nasm.exe'
$cmake = Join-Path $mingwBin 'cmake.exe'
foreach ($tool in @($gcc, $nasm, $cmake)) {
    if (-not (Test-Path -LiteralPath $tool)) { throw "Не найден инструмент сборки: $tool" }
}

$env:PATH = "$env:USERPROFILE\.cargo\bin;$mingwBin;$env:PATH"
$env:AWS_LC_SYS_PREBUILT_NASM = '1'
$env:CMAKE_GENERATOR = 'Ninja'
$env:RUSTFLAGS = '-C link-arg=-static'
$env:RUSTUP_TOOLCHAIN = 'stable-x86_64-pc-windows-gnu'

$env:CARGO_TARGET_DIR = Join-Path $env:TEMP ('reality-client-target-' + $expectedCommit.Substring(0, 8))
Push-Location $source
try {
    & cargo build --locked --release -p reality-client
    if ($LASTEXITCODE -ne 0) { throw "Сборка свежего ядра завершилась с кодом $LASTEXITCODE." }
    $built = Join-Path $env:CARGO_TARGET_DIR 'release\reality-client.exe'
    if (-not (Test-Path -LiteralPath $built)) { throw 'Cargo завершился без файла reality-client.exe.' }
    & $built --version
    if ($LASTEXITCODE -ne 0) { throw 'Собранное ядро не запускается.' }
    Copy-Item -LiteralPath $built -Destination (Join-Path $PSScriptRoot 'third_party\reality-client.exe') -Force
}
finally
{
    Pop-Location
    $tempRoot = [IO.Path]::GetFullPath($env:TEMP).TrimEnd('\') + '\'
    $resolvedSource = [IO.Path]::GetFullPath($sourceRoot)
    if (-not $resolvedSource.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase)) { throw 'Путь временных исходников оказался вне TEMP; очистка отменена.' }
    Remove-Item -LiteralPath $resolvedSource -Recurse -Force
}

$objdump = Join-Path $mingwBin 'objdump.exe'
$dependencies = & $objdump -p (Join-Path $PSScriptRoot 'third_party\reality-client.exe') |
    Select-String 'DLL Name:' | ForEach-Object { ($_ -split ':', 2)[1].Trim() } | Sort-Object -Unique
$nonSystem = @($dependencies | Where-Object { $_ -notmatch '^(api-ms-win-.*|ext-ms-win-.*|KERNEL32|USER32|ADVAPI32|SHELL32|WS2_32|WININET|OLE32|OLEAUT32|CRYPT32|BCRYPT|BCRYPTPRIMITIVES|NTDLL|IPHLPAPI|MSVCRT|CFGMGR32|DNSAPI|FWPUCLNT|SETUPAPI|WINHTTP|USERENV|GDI32|SECUR32|WINMM|PSAPI|COMDLG32|COMCTL32|SHLWAPI|NORMALIZ|VERSION|UCRTBASE)\.dll$' })
if ($nonSystem.Count -gt 0) {
    throw ('У ядра остались внешние DLL: ' + ($nonSystem -join ', '))
}

& (Join-Path $PSScriptRoot 'build.ps1')
if ($LASTEXITCODE -ne 0) { throw 'Сборка GUI после ядра не прошла.' }
$coreHash = (Get-FileHash -LiteralPath (Join-Path $PSScriptRoot 'third_party\reality-client.exe') -Algorithm SHA256).Hash
$guiHash = (Get-FileHash -LiteralPath (Join-Path $PSScriptRoot 'dist\RealityClient.exe') -Algorithm SHA256).Hash
$manifestPath = Join-Path $PSScriptRoot 'BUILD-MANIFEST.txt'
$manifest = [string[]]@(
    ('GuiExeSha256=' + $guiHash),
    ('CoreExeSha256=' + $coreHash),
    'CoreVersion=0.1.0',
    ('CoreSourceCommit=' + $expectedCommit)
)
$manifestText = [String]::Join([Environment]::NewLine, $manifest) + [Environment]::NewLine
[IO.File]::WriteAllText($manifestPath, $manifestText, [Text.Encoding]::ASCII)
Write-Output "CORE_BUILD=PASS"
Write-Output "CORE_EXE_SHA256=$coreHash"
