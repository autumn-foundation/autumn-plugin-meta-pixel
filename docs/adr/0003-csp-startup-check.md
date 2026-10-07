# ADR 0003: Check the CSP at startup and fail with the fix

Status: accepted

## Context

autumn-web 0.8 has no seam for a plugin to add CSP sources. The default CSP
blocks `connect.facebook.net` and `www.facebook.com`. Then the pixel fails, and
only the browser console shows the error.

## Decision

- At startup, read `security.headers.content_security_policy`. Check that
  `script-src` (or `script-src-elem`, or `default-src`) allows the host of
  `script_url`, and that `img-src` and `connect-src` allow
  `https://www.facebook.com`.
- `csp_check = "error"` (default): fail the start. The message gives the
  fixed CSP string (`csp::with_meta_pixel_sources`).
- `csp_check = "warn"`: log the same text. `"off"`: skip.
- Also check `'self'` in `script-src`. Report `'strict-dynamic'` and
  `require-trusted-types-for`: they block the loader, and no source fixes them.
- Check each policy of a comma-separated list.
- Skip the check when the pixel is off.
- An empty CSP (no header) passes.

## Consequences

- A broken setup fails in development, not in production analytics.
- The check parses a subset of CSP: directive names and source
  expressions. An unknown token never changes a fail into a pass.
- Upstream idea: a plugin seam to add CSP sources. Then this check becomes a fallback.
