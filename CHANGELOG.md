# Changelog

## 0.2.0 — 2026-10-31

### Changed

- **Breaking:** Support `autumn-web` 0.9. The plugin now needs `autumn-web`
  0.9. Apps on `autumn-web` 0.8 stay on 0.1.

## 0.1.0 — 2026-10-07

- First release for autumn-web 0.8.
- `MetaPixelPlugin`: loader served through the `PluginAssets` seam (hashed URL, SRI).
- `MetaPixel` extractor: `head`, `noscript`, `track`, `click_attr`, `hx_trigger_value`.
- Consent gate (`autumn_web::consent`), Global Privacy Control, off switch.
- Typed standard and custom events, `eventID`, single-pixel target.
- Startup checks for config and Content-Security-Policy.
- `REVOKE_HX_TRIGGER` stops a running pixel after a consent withdrawal.
- The loader sends `trackSingle` to each configured pixel only.
