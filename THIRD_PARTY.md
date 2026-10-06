# Third-party components

## reality-core / reality-client

- Repository: https://github.com/ERGFT/vpn-core
- Source snapshot: `third_party/vpn-core-source.zip`
- Checkout commit for the snapshot: `ee68039943ebb2aaf3287bf622ae34c18bfa0cae`
- License declared by the upstream Cargo workspace: GPL-3.0-or-later
- Bundled executable version: `reality-client 0.1.0`
- Bundled executable SHA-256: see the `CoreExeSha256` line in `BUILD-MANIFEST.txt`.

The Windows GUI embeds the executable as a resource and extracts it to `%LOCALAPPDATA%\RealityClient` at runtime. The source archive contains the pinned upstream commit listed above. `build_core.ps1` verifies the archive SHA-256 and revision marker, then extracts it to a temporary directory for a native rebuild. The GUI ships with that GPL-licensed core; keep the source archive and license with any redistribution.

The GUI and the combined Windows client are distributed under GPL-3.0-or-later; see `LICENSE.txt`.

## Wintun for Windows x64

- Project and official download: https://www.wintun.net/
- Version: 0.14.1
- Official archive SHA-256: `07C256185D6EE3652E09FA55C0B673E2624B565E02C4B9091C79CA7D2F24EF51`
- Bundled file: `third_party/wintun/wintun.dll`; SHA-256: `E5DA8447DC2C320EDC0FC52FA01885C103DE8C118481F683643CACC3220DAFCE`
- License: `third_party/wintun/LICENSE.txt`; the packaged copy is named `WINTUN-LICENSE.txt`.

The Windows x64 package includes the signed upstream DLL alongside the client. The pinned Reality Core loads it through the Wintun API when a TUN inbound is configured. The build script verifies the DLL hash before packaging it. Redistribution is governed by the included upstream prebuilt-binaries license.
