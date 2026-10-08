# Subscription HTTPS fixtures

The DER certificates and PKCS#8 key are synthetic, local test material.
The test CA is trusted only by test-local ureq agents, never by the OS or
production client. The leaf is valid for `localhost` and `127.0.0.1`, from
October 2026 until October 2036. The CA private key is not checked in.

`subscription-test-key.der` is intentionally public and must never be used
for a real service. Tests bind ephemeral loopback ports and do not start VPN,
change proxy settings, install certificates or access an external subscription.
