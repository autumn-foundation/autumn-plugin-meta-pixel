# ADR 0002: Gate the pixel on the server with autumn consent

Status: accepted

## Context

The Meta Pixel sets cookies and sends visitor data to a third party.
ePrivacy and GDPR need consent first. autumn-web has
`autumn_web::consent::Consent`. Its guide says: "the gate is the
enforcement, not just the banner".

A client-side gate (load the stub, wait for a JavaScript call) still sends
the loader and config to the visitor. Also, a template error can skip it.

## Decision

- The `MetaPixel` extractor reads `Consent` from the request. With
  `require_consent = true` (default), it renders nothing until
  `consent.allows(consent_category, consent_policy_version)`.
- The default category is `marketing`. Config rejects `necessary`.
- `Sec-GPC: 1` turns the pixel off when `honor_gpc = true` (default). The
  loader also checks `navigator.globalPrivacyControl`.
- `hx_trigger_value` gives `None` when the pixel is off.
- `REVOKE_HX_TRIGGER` stops a pixel that runs in the page after a withdrawal
  (`hx-boost` keeps the page in memory). The loader calls
  `fbq('consent', 'revoke')` and sends no more events.
- `MetaPixelPlugin::consent_policy_version` takes the app's policy version
  constant, so a policy change closes the gate.

## Consequences

- No third-party request before consent. A visitor who accepts gets the
  pixel on the next page load.
- Pages vary per visitor. Do not put them behind `CacheResponseLayer`
  (same rule as autumn's consent guide).
- The app must set the consent cookie with the same category
  (`accept_all_cookie(&["marketing"], VERSION)`).
