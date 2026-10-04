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
