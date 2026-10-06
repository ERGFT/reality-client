$ErrorActionPreference = "Stop"

$mingwBin = if ($env:REALITY_MINGW_BIN) {
    $env:REALITY_MINGW_BIN
} else {
    Join-Path $env:LOCALAPPDATA "Programs\msys64\mingw64\bin"
}
$dlltool = Join-Path $mingwBin "dlltool.exe"
if (-not (Test-Path -LiteralPath $dlltool)) {
    throw "MinGW-w64 dlltool.exe was not found at $dlltool. Set REALITY_MINGW_BIN to the MinGW-w64 bin directory."
}

$testLibDir = Join-Path ([System.IO.Path]::GetTempPath()) ("reality-client-test-importlib-" + [guid]::NewGuid().ToString("N"))
$tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$testLibDir = [System.IO.Path]::GetFullPath($testLibDir)
if (-not $testLibDir.StartsWith($tempRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Refusing to create test files outside the temporary directory."
}
New-Item -ItemType Directory -Path $testLibDir | Out-Null

try {
    $importLibrary = Join-Path $testLibDir "libshlwapi.a"
    & $dlltool --machine i386:x86-64 --input-def (Join-Path $PSScriptRoot "windows\shlwapi.def") --dllname shlwapi.dll --output-lib $importLibrary
    if ($LASTEXITCODE -ne 0) { throw "MinGW failed to create the shlwapi import library." }

    $env:PATH = "$mingwBin;$env:PATH"
    if ($env:LIBRARY_PATH) {
        $env:LIBRARY_PATH = "$testLibDir;$env:LIBRARY_PATH"
    } else {
        $env:LIBRARY_PATH = $testLibDir
    }

    Set-Location (Split-Path -Parent $PSScriptRoot)
    $target = "x86_64-pc-windows-gnu"
    cargo +stable-x86_64-pc-windows-gnu test --offline --locked --release --target $target
    if ($LASTEXITCODE -ne 0) { throw "The default Windows GNU test suite failed." }

    cargo +stable-x86_64-pc-windows-gnu test --offline --locked --release --target $target --features android-bridge-check
    if ($LASTEXITCODE -ne 0) { throw "The Android bridge host-check test suite failed." }

    cargo +stable-x86_64-pc-windows-gnu clippy --offline --locked --release --target $target --all-targets -- -D warnings -A dead_code
    if ($LASTEXITCODE -ne 0) { throw "Default Windows GNU Clippy failed." }

    cargo +stable-x86_64-pc-windows-gnu clippy --offline --locked --release --target $target --all-targets --features android-bridge-check -- -D warnings -A dead_code
    if ($LASTEXITCODE -ne 0) { throw "Android bridge host-check Clippy failed." }
} finally {
    if ($testLibDir.StartsWith($tempRoot, [System.StringComparison]::OrdinalIgnoreCase) -and (Test-Path -LiteralPath $testLibDir)) {
        [System.IO.Directory]::Delete($testLibDir, $true)
    }
}
