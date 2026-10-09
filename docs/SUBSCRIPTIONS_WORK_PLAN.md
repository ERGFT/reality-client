# HTTPS subscriptions implementation plan

Status: in progress. Target: Reality Client (Rust + Slint), Windows and Android.

Initial delivery: [PR #19](https://github.com/ERGFT/reality-client/pull/19),
implementation commit d89dba2. Windows and Android runtime checks passed;
PR #19 is now merged as 38fbabb. Follow-up usability/connection changes are in progress; [PR #20](https://github.com/ERGFT/reality-client/pull/20) is open; its latest CI remains pending.

Final storage review found the 16 KiB plaintext limit was also applied to DPAPI
ciphertext, which has additional headers. Protected subscription blobs now allow
32 KiB while input and legacy vault limits remain unchanged. A regression saves
and reopens maximum-length URL and VLESS inputs. All 15 subscription tests and
Clippy passed after this correction.

Final privacy/clipboard review: dependency TRACE logging is disabled at compile time
to prevent HTTP request path/query leakage even with RUST_LOG=trace. The shared
clipboard sanitizer accepts HTTPS and rejects multiline input without echoing
secrets. All 17 subscription tests, fmt and all-target android-bridge-check
Clippy passed. Native clipboard input remains unverified. Final CI is pending.

## Usability revision requested 2026-10-09

PR #19 is merged as 38fbabb. The revised import UX and connection fixes will be delivered in a new PR on the existing rust-rewrite branch.
The previous two-form layout confused a selected server with a new import draft.
Reference: Hiddify's add-profile flow (clipboard or manual link), and v2rayN's
subscription groups. Keep the existing theme; simplify the user flow.

1. One Add action and independent draft for VLESS or HTTPS; detect the format.
2. Preserve selected profiles while typing, clear secrets after successful import,
   show errors beside the draft; leave subscription management separate.
3. Review desktop/mobile render with the user before expanding the redesign.
4. Diagnose reported unchanged/Russian exit IP using the actual platform and
   proxy/TUN mode. Country alone is not proof of routing failure. Separate core
   outbound success from browser/system routing and from configured direct rules.
5. Verify changes in isolated tests, then open a new PR and wait for CI.

Local draft: 18 subscription/import tests passed, the real Slint import callback
added a server without replacing the manual profile or breaking a running
loopback TCP stream. Desktop/mobile software renders inspected; all-target Clippy
with android-bridge-check passed. This revision is now in PR #20 and has not been merged.
Native clipboard/keyboard and this revised screen on Android remain unverified.
Read-only host check: Happ is running; Windows proxy is enabled but is not the
client's 127.0.0.1:1080 endpoint. This is current state, not evidence of the state
when 2ip.io was tested. No host network setting was changed.
Connection audit: editor changes were ignored by file-based startup, selected
profiles were ignored in full-config mode, and profile runtime files could
replace a JSON file chosen for advanced editing. Startup now uses the editor
snapshot, binds the selected VLESS server explicitly to the proxy outlet, keeps
runtime files separate, and preserves routes/DNS/TUN settings. A user can disable
binding for an independent raw JSON configuration. Same snapshot handling is
implemented for Android. UI shows the active server/mode and detects foreign
Windows proxy changes without restoring network settings automatically.

Verification: two synthetic VLESS servers received real HTTP CONNECT and SOCKS
requests through the linked core; switching saved profile changed the response.
A separate opt-in probe of an encrypted local vault copy completed an HTTPS
IP-check request with an exit different from the ordinary OS route. No URL or
IP address was printed. The copied vault and runtime secret were removed. This
proves core/server forwarding; native browser routing and Windows TUN remain
unverified while Happ is running. Never claim they passed from a listener test.

Final local follow-up checks (2026-10-09): 114 standard Windows unit tests
passed; 117 with android-bridge-check passed (one opt-in vault probe ignored
in both normal suites, and executed separately successfully). All-target Clippy
and fmt passed. Android x86_64 cargo-ndk check passed. Native browser/TUN and
updated Android runtime interaction are still separate verification gates.
Default manual server names now use the VLESS remark or endpoint, rather than
an uninformative profile number.

A final clipboard callback regression rejects results from an older, closed, or busy import draft. Manual edit/create remain separate; saving clears secrets and invalidates pending clipboard reads.

## Requirements

- Import an HTTPS subscription as a group of servers; support UTF-8 URI lists
  and standard/URL-safe Base64 lists with or without padding.
- Validate imported VLESS parameters against the pinned core, before saving.
  Report unsupported protocols and malformed entries without exposing secrets.
- Bound download duration, redirects, body size, server count and URI length.
  Validate TLS certificates and prevent HTTPS-to-HTTP redirects.
- Protect the subscription URL using existing platform secret storage. Keep
  manual profiles intact and use atomic persistence for group replacements.
- Provide add, paste, refresh, rename, delete and server selection controls.
  Show last successful update and last failure. Group servers by subscription.
- Preserve selected server identity across updates. Updating a subscription
  must not stop or reload an active VPN session. Failed/empty updates retain
  the previous working list.
- Verify parser, persistence, refresh and UI on Windows and Android. Document
  formats and limitations. Deliver a separate PR and wait for green CI.

## Implementation sequence

1. Inspect profile storage, connection selection, clipboard and Slint layout.
2. Implement bounded downloader and URI/Base64 parser with synthetic fixtures.
3. Extend storage with stable subscription and server identities, protected
   URLs, atomic replacement and backward compatibility for RCLIENT1 profiles.
4. Wire background workers and subscription controls to the server page.
5. Handle selection changes and concurrent operations without disturbing the
   session; display sanitized failure reasons and import summary.
6. Add parser, storage and update regression tests; run platform CI, verify
   Windows interaction and Android layout/runtime where available.
7. Update user and architecture documentation; create a separate PR, inspect
   all checks, fix failures and verify main after the authorized merge.

## Evidence and decisions

- Current main: e50b344. Existing ProfileStore saves only individual VLESS
  links in RCLIENT1 format; there is no client subscription downloader or UI.
- The pinned core already exposes VlessConfig parsing/validation. Use that
  parser rather than duplicating its transport and REALITY rules.
- No real subscription URL or credentials are required for synthetic tests.
  Provider-specific response compatibility remains to be verified from a
  sanitized example if the provider returns another format.
- Initial refresh is manual. Automatic refresh is a separate follow-up setting.
- Work, tools, temporary files, caches and builds stay on D:. Do not change
  Happ, host VPN/proxy/DNS/routes. Do not claim device runtime from compilation.

## Completion checklist

- [x] Inspect current source and locate integration points.
- [x] Downloader/parser and regression tests (13 subscription tests pass on Windows).
- [x] Protected persistence and compatibility with legacy manual profiles.
- [x] UI, grouping and operation coordination in Windows Slint integration test; native clipboard and Android remain part of platform verification.
- [x] Selection preservation and active-session update behavior (same live loopback TCP stream through the real core).
- [x] Windows and Android verification (14 host tests, rendered UI, actual Android callback/Keystore runtime; native clipboard and provider-specific responses not verified).
- [ ] Documentation, separate PR, green CI and main verification.

### Current verification

- Windows GNU `cargo check --all-targets` passed after Slint callback correction.
- Windows GNU `cargo test --lib subscription`: 9 passed, including DPAPI
  protection, restart, identity preservation and failed atomic commit rollback.
- Windows GNU clippy with `android-bridge-check --all-targets -D warnings
  -A dead_code` passed. Warnings in the existing vendored core dependency remain.
- Client controls and storage are implemented; interaction, concurrency cases,
  Android runtime and complete CI are still pending. No PR is published yet.
- Build environment: existing MinGW must precede Rust DLL directories in PATH;
  AWS-LC CMake builder used. All output and caches remain on D:.

### Additional verification and fixes

- 13 Windows tests pass: parser, DPAPI/restart/atomic rollback, mixed manual and
  subscription profiles, reorder/remove identities, unchanged manual vault,
  corrupt subscription file isolation, real loopback HTTPS with certificate
  verification, untrusted CA rejection, redirect limit and HTTP downgrade,
  oversize body, timeout while reading the body and HTTP 403.
- Fixture trust is scoped to the test agent; no OS certificates, VPN, proxy,
  DNS or routes are modified. DER test key is explicitly synthetic/public.
- Manual saves now acquire the operation guard. Subscription commits acquire
  it on the UI thread, avoiding a race with connection check/start handlers.
- Secret reads during selection reconciliation run in the existing background
  profile worker. Stale profile reads are detected with the store revision.
- Windows GUI binary builds. Live input verification is pending; the active
  desktop includes a game, so avoid focus/input interference.
- Android target installation started on D:; existing SDK/NDK/JDK/Gradle can
  be reused. Build/device verification and PR/CI remain outstanding.

### Next actions / continuation checkpoint

- Both Android Rust targets are installed in the D: Rustup home. Current
  `cargo ndk -t x86_64 --platform 26 check --locked --lib` is running, with
  two build jobs, log `D:\reality-client-work\tmp\subscription-android-check.log`.
  The observed exec session is 27965; re-poll that handle before restarting.
- Reuse `D:\reality-client-work\tmp\subscription-env.ps1` for host checks.
  Android uses existing SDK/NDK on C: (read only), JDK/Gradle on D:, all new
  Cargo outputs and downloads on D:.
- Finish Android compilation, prepare emulator APK and inspect mobile UI.
  Add offscreen/render or runtime interaction evidence for both layouts.
- Add operation/selection regression coverage and verify full authorized
  non-network-altering tests. Update architecture, root docs and checklist.
- Fetch main before the PR, preserve changes on an existing suitable branch
  or detached checkout; do not create a new branch. Create a new PR rather
  than adding work to an already closed PR. Wait for all checks before merge.

### October 9 verification

- Android x86_64 `cargo ndk ... check --locked --lib` passed in 5m08s. Windows
  AWS-LC CMake override is inappropriate for Android; use the CC builder via
  `D:\reality-client-work\tmp\subscription-android-env.ps1` instead.
- A Windows Slint integration test passed: add, select, refresh/reorder, rename,
  delete, unchanged manual vault, selection/cache reconciliation and desktop/
  mobile software-rendered screenshots in `D:\reality-client-work\subscription-proof`.
- A real isolated core listener and loopback echo server retained the same TCP
  connection through all subscription mutations. No TUN, system proxy, external
  service or existing user vault was used.
- Screenshots inspected. Long group/server names were truncated on mobile;
  adjusted ProfileRow to wrap and hide the redundant selected badge in mobile
  layout. The width-dependent badge caused a Slint binding loop; replaced it
  with an explicit compact property and verified compilation and fresh renders.
- Emulator acceleration is available (WHPX); API35 image and build-tools35
  already installed. Native x86_64 debug library builds passed, including the
  final layout revision (`subscription-android-final.log`).

### Latest checkpoint

- Windows: all 14 subscription tests passed; final `android-bridge-check`
  Clippy for all targets passed. Mobile controls and long server names inspected
  in fresh software-rendered screenshots; all subscription controls fit.
- Gradle: Kotlin unit tests and APK packaging passed. Existing emulator app had
  a different signing certificate; preserved it and used an external Gradle
  init script to package a separate test application ID. Test APK installed and
  launched on API35 emulator-5580, startup screenshot inspected, no AndroidRuntime
  errors observed. This confirms startup, not Android subscription interaction.
- Repackaging with the final native library passed in 19 seconds;
  log `D:\reality-client-work\tmp\subscription-apk-final.log`.
- Outstanding: reinstall final isolated APK, exercise Android subscription
  controls/storage, update RU/EN status docs, review diff, commit, separate PR,
  all CI green and merge verification. No PR or subscription commit published.
- Remote main remains e50b344; no open client PRs observed. Do not use closed
  PR18 for new work. Select an existing suitable remote branch for a new PR;
  preserve its existing history and do not create a new branch.

### Android runtime verification completed

- Opt-in `subscription-device-check` compiled into an isolated emulator APK;
  the normal app ID and default builds cannot activate the driver.
- API35 x86_64 PASS: real Android Slint callbacks add/select/refresh/reorder/
  rename/delete, stable server ID, URL/server reads from Android Keystore after
  reopening storage and unchanged manual profile bytes. No core/VPN session.
- The first driver run exposed a test wait race: rename starts its worker
  before the common connection flag is set. The driver now waits on both the
  subscription busy state and operation flag; the repeat passed.
- Host desktop has a game running; no focus/input automation was performed.
  Native clipboard/input and an actual provider remain unverified.
- Windows pagefile grew automatically to about 19 GiB on C: during builds;
  direct artifacts/caches are on D:. Limit subsequent Cargo jobs to two and
  do not change host paging/security/VPN settings.
