# Third-party components

## reality-core / reality-client

- Repository: https://github.com/ERGFT/vpn-core
- Pinned source: commit recorded in `third_party/vpn-core.rev`; `scripts/fetch-core.sh` and `scripts/fetch-core.ps1` fetch exactly that commit from the repository above (a commit hash fixes the content, so no separate checksum is kept).
- License declared by the upstream Cargo workspace: GPL-3.0-or-later
- Built from that commit: `libreality` (`reality.dll`, `libreality.so`) and the `reality-client` command-line program that the GUI starts for config checks, system-proxy recovery and TUN cleanup.

Corresponding source for the GPL-licensed core in any binary package is the commit above; each Windows package carries `CORE-SOURCE.txt` with the repository URL, the commit and a direct archive link.

The GUI and the combined Windows client are distributed under GPL-3.0-or-later; see `LICENSE.txt`.

## Wintun for Windows x64

- Project and official download: https://www.wintun.net/
- Version: 0.14.1
- Official archive SHA-256: `07C256185D6EE3652E09FA55C0B673E2624B565E02C4B9091C79CA7D2F24EF51`
- Bundled file: `third_party/wintun/wintun.dll`; SHA-256: `E5DA8447DC2C320EDC0FC52FA01885C103DE8C118481F683643CACC3220DAFCE`
- License: `third_party/wintun/LICENSE.txt`; the packaged copy is named `WINTUN-LICENSE.txt`.

The Windows x64 package includes the signed upstream DLL alongside the client. The pinned Reality Core loads it through the Wintun API when a TUN inbound is configured. The build script verifies the DLL hash before packaging it. Redistribution is governed by the included upstream prebuilt-binaries license.
