# SPDX-License-Identifier: GPL-3.0-or-later
param(
    [string]$CoreExe = (Join-Path $PSScriptRoot 'third_party\reality-client.exe')
)

$ErrorActionPreference = 'Stop'
$compiler = Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
if (-not (Test-Path -LiteralPath $compiler)) {
    $compiler = Join-Path $env:WINDIR 'Microsoft.NET\Framework\v4.0.30319\csc.exe'
}
if (-not (Test-Path -LiteralPath $compiler)) { throw 'Не найден компилятор .NET Framework csc.exe.' }
if (-not (Test-Path -LiteralPath $CoreExe)) { throw "Не найдено ядро reality-client.exe: $CoreExe" }

$dist = Join-Path $PSScriptRoot 'dist'
New-Item -ItemType Directory -Force -Path $dist | Out-Null
$output = Join-Path $dist 'RealityClient.exe'
$resource = $CoreExe + ',RealityClientGui.reality-client.exe'
$sources = Get-ChildItem -LiteralPath (Join-Path $PSScriptRoot 'src') -Filter '*.cs' -File | ForEach-Object { $_.FullName }

$arguments = @(
    '/nologo', '/optimize+', '/target:winexe', '/platform:anycpu', '/langversion:5',
    "/out:$output", "/resource:$resource",
    '/reference:System.dll', '/reference:System.Core.dll', '/reference:System.Drawing.dll',
    '/reference:System.Security.dll', '/reference:System.Windows.Forms.dll',
    '/reference:Microsoft.CSharp.dll'
) + $sources
& $compiler @arguments
if ($LASTEXITCODE -ne 0) { throw "Компиляция RealityClient.exe завершилась с кодом $LASTEXITCODE." }

$shell = New-Object -ComObject WScript.Shell
$shortcutPath = Join-Path $dist 'Reality Client.lnk'
$shortcut = $shell.CreateShortcut($shortcutPath)
$shortcut.TargetPath = $output
$shortcut.WorkingDirectory = $dist
$shortcut.Description = 'Reality Client — GUI для reality-core'
$shortcut.Save()

$hash = (Get-FileHash -LiteralPath $output -Algorithm SHA256).Hash
$coreHash = (Get-FileHash -LiteralPath $CoreExe -Algorithm SHA256).Hash
$manifest = [string[]]@(
    ('GuiExeSha256=' + $hash),
    ('CoreExeSha256=' + $coreHash),
    'CoreVersion=0.1.0',
    'CoreSourceCommit=ee68039943ebb2aaf3287bf622ae34c18bfa0cae'
)
$manifestText = [String]::Join([Environment]::NewLine, $manifest) + [Environment]::NewLine
[IO.File]::WriteAllText((Join-Path $PSScriptRoot 'BUILD-MANIFEST.txt'), $manifestText, [Text.Encoding]::ASCII)
Write-Output "BUILD=PASS"
Write-Output "EXE=$output"
Write-Output "SHA256=$hash"
