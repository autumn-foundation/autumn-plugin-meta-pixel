# CLAUDE.md

Meta Pixel plugin for autumn-web 0.8. Style for all docs and comments: ASD-STE100.

## Layout

| Path | Contents |
|---|---|
| `src/policy.rs` | Verified core: JSON escape for HTML, pixel ID and event name checks, load gate. Pure. |
| `verus/policy.rs` | Verus spec and proofs for `src/policy.rs`. Keep both in step. |
| `src/config.rs` | `[meta_pixel]` config, profile file, `AUTUMN_META_PIXEL__*` env overlay. |
| `src/csp.rs` | CSP parse, check, and fix. |
| `src/event.rs` | `StandardEvent`, `Event`, `Content`. One JSON format for all fire paths. |
| `src/assets.rs` | The `PluginAssets` bundle (autumn 0.8 plugin assets seam). |
| `assets/meta-pixel.js` | The loader. No inline script, no `eval`. |
| `src/pixel.rs` | `MetaPixel` extractor and markup. |
| `src/plugin.rs` | `MetaPixelPlugin`, startup checks. |
| `tests/app.rs` | `TestApp` tests and plugin conformance. |
| `tests/boot.rs` | Real app boot, env config. |
| `tests/js/` | Loader tests (`node --test`, fake DOM). |
| `tests/e2e/browser.mjs` | Chromium test against `examples/app.rs`. |
| `docs/` | `plan.md`, `ac-evidence.md`, ADRs. |

## Commands

```bash
git config core.hooksPath .githooks   # pre-commit: fmt, clippy, test, JS test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
node --test tests/js/*.test.mjs
cargo llvm-cov --all-targets --summary-only
verus verus/policy.rs
cargo build --example app && NODE_PATH=<playwright dir> node tests/e2e/browser.mjs
```

## Rules

- Write the test first. See it fail. Then write the code.
- A change to `src/policy.rs` needs the same change in `verus/policy.rs`. Run Verus.
- No inline JavaScript. Put data in `application/json` blocks. Escape all JSON with `escape_json_for_html`.
- The server is the gate. No pixel markup and no `HX-Trigger` value when the pixel is off.
- Never send an event to a pixel that is not in `pixel_ids`.
- The extractor never fails. Analytics must not break a page.
- No `unwrap` or `expect` in `src/` outside tests. No `unsafe`.
- Tests that need env vars run a child process (`tests/boot.rs`).
- Do not log event parameters. They can hold user data.
