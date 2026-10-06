# SPDX-License-Identifier: GPL-3.0-or-later
#
# Скачивает исходники vpn-core ровно той версии, что записана в
# third_party/vpn-core.rev (хеш коммита), в указанный пустой каталог.
# Переменная REALITY_CORE_URL подменяет адрес репозитория.
param([Parameter(Mandatory = $true)][string]$Destination)
$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$revision = (Get-Content -LiteralPath (Join-Path $repoRoot 'third_party\vpn-core.rev') -Raw).Trim()
$url = if ($env:REALITY_CORE_URL) { $env:REALITY_CORE_URL } else { 'https://github.com/ERGFT/vpn-core.git' }
if ($revision -notmatch '^[0-9a-f]{40}$') { throw "third_party\vpn-core.rev должен содержать полный хеш коммита, получено: '$revision'" }
if ((Test-Path -LiteralPath $Destination) -and (Get-ChildItem -LiteralPath $Destination -Force | Select-Object -First 1)) {
    throw "Каталог не пуст: $Destination"
}

New-Item -ItemType Directory -Force -Path $Destination | Out-Null
function Invoke-Git {
    & git -C $Destination @args
    if ($LASTEXITCODE -ne 0) { throw "git $($args -join ' ') завершился с кодом $LASTEXITCODE." }
}
Invoke-Git init -q
Invoke-Git remote add origin $url
Invoke-Git fetch -q --depth 1 origin $revision
Invoke-Git checkout -q FETCH_HEAD

$actual = (& git -C $Destination rev-parse HEAD).Trim()
if ($actual -ne $revision) { throw "Скачана другая версия ядра: $actual вместо $revision" }
if (-not (Test-Path -LiteralPath (Join-Path $Destination 'Cargo.toml'))) { throw 'В скачанных исходниках нет корневого Cargo.toml ядра.' }
Write-Output "CORE_SOURCE_COMMIT=$revision"
