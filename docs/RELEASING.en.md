[Русский](RELEASING.md) | [English](RELEASING.en.md)

# Cutting a release

CI builds and publishes the release: pushing a tag is enough.

Contents:

1. [How to release](#1-how-to-release)
2. [What CI does](#2-what-ci-does)
3. [What is still missing](#3-what-is-still-missing)

## 1. How to release

1. Write the notes `docs/releases/<tag>.md` (Russian and English; say plainly what was checked and what was not, following [STATUS.en.md](STATUS.en.md)). Without the file the release gets automatic notes.
2. Make sure `main` is green and the notes are already in `main`.
3. Tag the right `main` commit and push the tag:

```sh
git tag -a v0.1.0-preview.1 -m "Reality Client v0.1.0-preview.1"
git push origin v0.1.0-preview.1
```

A tag with a hyphen (`v0.1.0-preview.1`) becomes a pre-release, one without (`v0.1.0`) a regular release.

## 2. What CI does

A `v*` tag starts the three workflows (Linux, Windows, Android). Each builds its package, computes SHA-256 and publishes the files to the release in a separate job through `.github/scripts/publish-release.sh`: the first of the three creates the release (with the notes from `docs/releases/<tag>.md`), the others add their files. The release files:

| File | Platform |
|---|---|
| `RealityClient-Rust-windows-x64.zip`, `SHA256SUMS-windows-x64.txt` | Windows x64 |
| `RealityClient-Rust-android-arm64-debug.apk`, `SHA256SUMS-android-arm64.txt` | Android arm64 |
| `RealityClient-Rust-linux-x86_64.tar.gz`, `SHA256SUMS-linux-x86_64.txt` | Linux x86_64 |

The Windows build takes about half an hour.

## 3. What is still missing

- Signatures: the Windows build is unsigned (SmartScreen warns), the APK is signed with a debug key. Signing needs the owner's certificate and key; they are not kept in the repository.
- Update instructions between versions (for now: "uninstall and install the new one").
