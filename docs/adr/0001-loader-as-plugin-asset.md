# ADR 0001: Ship the loader as a plugin asset, not an inline snippet

Status: accepted

## Context

Meta gives the pixel as an inline `<script>`. autumn-web sends
`script-src 'self'` by default. That CSP blocks inline scripts. A nonce CSP
works, but each template must pass the nonce. autumn-web 0.8 adds
`PluginAssets`: a plugin can serve compiled-in files at content-hashed,
same-origin URLs with SRI.

## Decision

- Put the loader in `assets/meta-pixel.js`. Serve it with
  `PluginAssets::from_files("meta-pixel", ...)` and `AppBuilder::plugin_assets`.
- Render it with `ASSETS.deferred_script_tag`, so the tag has the hashed URL,
  `integrity`, and `crossorigin`.
- Put config and events in `<script type="application/json">` blocks. The
  browser does not run them, so CSP does not apply. The loader parses them.
- Use `from_files`, not `plugin_assets!`. The app does not need the
  `embed-assets` feature.

## Consequences

- No inline JavaScript. No nonce work in templates.
- A crate upgrade that changes the loader changes its URL. Caches stay correct.
- `fbevents.js` still comes from Meta. The CSP must allow its hosts (ADR 0003).
- JSON in HTML must not close the block. We escape `<`, `>`, and `&`, and prove it (`verus/policy.rs`).
