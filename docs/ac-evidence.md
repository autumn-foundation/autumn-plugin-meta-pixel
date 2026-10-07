# Acceptance criteria and evidence

Style: ASD-STE100.

No GitHub issue exists for this work. The acceptance criteria come from
`docs/plan.md` section 8. Each row gives the evidence: a test (Rust unit
tests are `module::tests::name`; `tests/app.rs`, `tests/boot.rs`; JS tests
are in `tests/js/meta-pixel.test.mjs`; browser steps are in
`tests/e2e/browser.mjs`), a proof, or a file.

## Commands that give the evidence

| Command | Result |
|---|---|
| `cargo test` | 64 unit, 13 `TestApp`, 1 boot, 3 doc tests pass. |
| `node --test tests/js/*.test.mjs` | 20 pass. |
| `node tests/e2e/browser.mjs` (Chromium) | `e2e: ok`. |
| `verus verus/policy.rs` | 28 verified, 0 errors. |
| `cargo llvm-cov --all-targets --summary-only` | 97.8% lines. |
| `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, rustdoc `-D warnings`, `cargo +1.88.0 check` | Clean. |

## Criteria

| AC | Criterion | Evidence |
|---|---|---|
| AC1 | `MetaPixelPlugin` implements `Plugin`. One call installs it. Contract for autumn-web 0.8. `[meta_pixel]` config section. | `src/plugin.rs` (`impl Plugin`). `plugin::tests::contract_names_autumn_08`. `routes_are_public_attributed_and_conformant` (autumn `run_conformance` passes; `has_config_section("meta_pixel")`). `app_boots_with_env_config` (real app, one `.plugin(...)` call, strict config accepts `AUTUMN_META_PIXEL__*`). |
| AC2 | Loader through the `PluginAssets` seam: hashed `immutable` URL, plain `must-revalidate` URL, public routes, SRI tag. | `src/assets.rs` (`PluginAssets::from_files`), `AppBuilder::plugin_assets` in `src/plugin.rs`. `loader_is_served_at_hashed_immutable_url` (hashed URL `immutable`, plain URL `must-revalidate`, `asset_url` resolves). `routes_are_public_attributed_and_conformant` (both routes `GET`, `Public`, source is the plugin). `pixel::tests::head_has_config_block_and_sri_loader_tag`. e2e step 2 (`integrity` is `sha384-…`, script runs under CSP). `app_boots_with_env_config` (hashed URL served `immutable` by a real app). |
| AC3 | No inline JavaScript. JSON data blocks. JSON escapes `<`, `>`, `&`. | `pixel::tests::head_has_config_block_and_sri_loader_tag` (each `<script>` is JSON or has `src`). `pixel::tests::track_renders_escaped_event_block` (`</script>` in a parameter cannot close the block). `policy::tests::escape_*` (property tests). Verus: `lemma_escape_safe`, `lemma_escape_identity`, `escape_json_for_html` postcondition. e2e: no CSP violation in the console. |
| AC4 | `head()` inits each pixel and sends `PageView` (configurable). `noscript()` gives a hidden image per pixel. | JS: `base code: init each pixel, PageView to each pixel…`, `pageView off, autoConfig off…`. `pixel::tests::noscript_has_hidden_image_per_pixel`, `pixel::tests::htmx_requests_get_no_noscript_image`. JS: `noscript images of the plugin are removed…`. e2e steps 2, 6, 7 (no `noscript=1` hit on boost or back). |
| AC5 | Typed events: all standard events, checked custom names, typed parameters, extra parameters, `eventID`, single-pixel target. | `event::tests::standard_names_match_meta` (18 names), `purchase_json`, `all_param_setters`, `non_finite_numbers_are_not_sent`, `custom_event_checks_name`. `policy::tests::event_name_*`; Verus `spec_event_name`, `lemma_event_name_safe`. `pixel::tests::unknown_pixel_target_is_not_rendered`. JS: `event blocks fire once, in order, only to configured pixels`. e2e step 6 (`Purchase` with `{eventID: 'order-1001'}`). |
| AC6 | Three fire paths: data block (page or htmx swap), `HX-Trigger`, click attribute. Each fires once. | JS: `event blocks fire once…`, `a morph that drops the done attribute…`, `htmx:load fires events in swapped content…`, `HX-Trigger event fires each event…`, `click on a data-meta-pixel element… capture phase`, `an fbq error does not stop the other events`. `htmx_partial_gets_trigger_header_and_block`. `pixel::tests::hx_trigger_value_is_visible_ascii_and_round_trips`, `history_restore_requests_get_no_event_blocks`. e2e steps 3 (click), 4 (htmx swap + `HX-Trigger`), 5 (`consume` click), 6 (boost), 7 (history back: no second fire). |
| AC7 | Consent gate, GPC (server and browser), `enabled = false`. | `pixel::tests::consent_gate`, `gpc_and_off_switch`, `off_pixel_renders_nothing`. `no_consent_no_pixel` (no markup, no `HX-Trigger`), `gpc_request_gets_no_pixel`, `disabled_config_gets_no_pixel`. JS: `GPC: honored stops the load`, `revoke: …`. Verus: `lemma_no_pixel_without_consent`, `lemma_gpc_wins`, `lemma_disabled_is_off`. e2e steps 1 (no consent: no `fbq`, no request to Meta), 8 (withdrawal revokes), 10 (`Sec-GPC: 1`). |
| AC8 | Startup checks: bad config fails; a CSP that blocks the pixel fails with the fixed CSP, or warns, or is off. | `bad_pixel_id_fails_startup`, `default_csp_fails_startup` (message has the fixed CSP), `csp_check_warn_starts`. `plugin::tests::default_csp_fails_with_the_fix`, `fixed_csp_passes`, `warn_and_off_do_not_fail`, `nonce_note_*`, `trusted_types_fails_with_a_reason`. `csp::tests::*` (15 cases, 3 property tests: the fix closes all gaps, keeps a good policy, keeps other directives). Manual: the example app with a CSP that blocks `connect.facebook.net` does not start. |
| AC9 | Serde config with validation, layered TOML, `AUTUMN_META_PIXEL__*` env vars. | `config::tests::*` (9 cases: defaults, unknown keys, env typing and `1`/`0`, profile order, profile file, `load_from_dir`, validation, no secret in errors). `app_boots_with_env_config`. |
| AC10 | `history_page_views` → `fbq.disablePushState`; `auto_config` → `fbq('set', 'autoConfig', false, id)`. | JS: `pageView off, autoConfig off, history page views off`, `app fbq already present…` (flag set on an existing `fbq` too). e2e step 2 (`disablePushState` false by default). |
| AC11 | fmt, clippy pedantic and nursery, no `unwrap` in `src/`, tests pass, coverage ≥ 85%, CI runs them. | `Cargo.toml` `[lints]`, `clippy.toml`. `.github/workflows/ci.yml`: `fmt`, `clippy` (+ rustdoc), `msrv`, `test` (`--fail-under-lines 85`, doc tests), `js`, `e2e`, `verus`. `.githooks/pre-commit`. Coverage 97.8%. |
| AC12 | Verus specs state the policy invariants. Proofs pass. | `verus/policy.rs`: 28 verified, 0 errors. Two injected bugs (no `>` escape; no consent term in the gate) give 2 errors, so the proofs are not vacuous. CI job `verus`. |
| AC13 | README, CLAUDE.md, ADRs, Mermaid diagram, ASD-STE100. | `README.md` (Mermaid diagram), `CLAUDE.md`, `docs/plan.md` (Mermaid diagram), `docs/adr/0001…0003`, `CHANGELOG.md`. A reviewer agent checked the ASD-STE100 style; its findings are fixed. |

## TDD record

- SPEC and PROOF: `verus/policy.rs` before any code. 28 verified.
- RED: stub bodies gave 39 unit, 8 `TestApp`, 1 boot, and 11 JS failures.
  `proptest-regressions/policy.txt` keeps two seeds from that phase.
- GREEN: all pass. REFACTOR: clippy pedantic and nursery, docs.
- Review fixes went test first too (for example 12 JS tests and 3 unit
  tests failed before the loader and `pixel.rs` changes).

## Review

Five reviewer agents checked the code from these angles: security,
Rust and autumn correctness, JavaScript with `fbq` and htmx, tests and
proofs, and docs with ASD-STE100. The fixed findings are in the second
commit and in `CHANGELOG.md`. Findings that stay open, with the reason:

- The `autumn.consent` cookie has no `__Host-` prefix (autumn upstream).
- The plugin does not set `Vary`/`Cache-Control` on pixel pages. The README
  says not to cache them. autumn's own `CacheResponseLayer` ignores both
  headers.
- An event block in user HTML fires. The README says the sanitizer must
  remove it.
- A profile with the literal name `default` reads no profile layer (same as
  the slack plugin; autumn uses `default` for "no profile").
