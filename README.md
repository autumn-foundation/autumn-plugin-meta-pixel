# autumn-plugin-meta-pixel

Meta Pixel plugin for [autumn-web](https://github.com/autumn-foundation/autumn) 0.8.

- **No inline JavaScript.** The loader is a plugin asset (autumn 0.8 `PluginAssets`): a content-hashed, `immutable`, same-origin URL with SRI.
- **Consent first.** No pixel markup until the visitor consents (`autumn_web::consent`). No pixel for a Global Privacy Control request. A withdrawal stops a pixel that runs in the page.
- **Typed events.** All Meta standard events, checked custom events, typed parameters, `eventID`, and single-pixel targets. Events go only to the pixels in your config.
- **htmx ready.** Events fire from data blocks (also after a swap), from an `HX-Trigger` header, or from a click attribute. Boost, history restore, and morph swaps do not fire an event twice.
- **Startup checks.** Bad config stops the app start. When the pixel is on, a CSP that blocks it also stops the start. The error gives the fix.

```mermaid
flowchart LR
  H[handler] -->|MetaPixel extractor| G{enabled, pixels, consent, GPC}
  G -->|off| N[empty markup]
  G -->|on| M["head(): config JSON + SRI script tag"]
  M --> L["/static/_plugins/meta-pixel/meta-pixel.&lt;hash&gt;.js"]
  L --> F[fbq: init, PageView] --> X[connect.facebook.net/fbevents.js]
  E["track(): event JSON block"] --> L
  T["hx_trigger_value(): HX-Trigger"] --> L
  C["click_attr(): data-meta-pixel"] --> L
  R["REVOKE_HX_TRIGGER"] -->|consent revoke| L
```

## Install

```toml
[dependencies]
autumn-plugin-meta-pixel = "0.1"
```

```rust,ignore
use autumn_plugin_meta_pixel::MetaPixelPlugin;

const POLICY_VERSION: u32 = 1; // Your cookie policy version.

autumn_web::app()
    .routes(routes![index])
    .plugin(MetaPixelPlugin::new().consent_policy_version(POLICY_VERSION))
    .run()
    .await;
```

```toml
# autumn.toml
[meta_pixel]
pixel_ids = ["1234567890123456"]

[profile.dev.meta_pixel]
enabled = false
```

`examples/app.rs` is a full app. Install the plugin one time only: autumn ignores a second install with the same name, and its config too.

## Use

Take `MetaPixel` as a handler argument. It never fails. When the pixel is off, each method gives empty markup or `None`.

```rust,ignore
use autumn_plugin_meta_pixel::{Event, MetaPixel, StandardEvent};

#[get("/thanks")]
async fn thanks(pixel: MetaPixel) -> Markup {
    let purchase = Event::standard(StandardEvent::Purchase)
        .value(25.0)
        .currency("EUR")
        .event_id("order-1001");
    let call = Event::standard(StandardEvent::Contact);
    html! {
        head { (pixel.head()) }
        body {
            (pixel.noscript())
            (pixel.track(&purchase))
            button data-meta-pixel=[pixel.click_attr(&call)] { "Call us" }
        }
    }
}
```

| Method | Gives | Fires |
|---|---|---|
| `head()` | Config data block and loader `<script>` (SRI, `defer`). Put it in `<head>`. | `init` for each pixel, then `PageView`. |
| `noscript()` | Hidden `PageView` image for each pixel. Put it at the start of `<body>`. Empty for an htmx request. | Only with no JavaScript. |
| `track(&event)` | Event data block. Empty for an htmx history restore. | One time, at page load or after an htmx swap. |
| `click_attr(&event)` | Value for `data-meta-pixel`. | On each click on the element or a child. |
| `hx_trigger_value(&[event])` | `HX-Trigger` header value (ASCII). | After the htmx request. |
| `is_active()` | `true` when this page gets the pixel, `false` when it is off. | — |

The loader sends each event with `trackSingle` to each pixel in `pixel_ids`. So an event does not go to a pixel that other code on the page started.

- Custom events: `Event::custom("ShareClick")?`. The name has 1 to 50 characters from `A-Z a-z 0-9 _`.
- Custom parameters: `.param("plan", "pro")`.
- One pixel only: `.for_pixel("123")`. The ID must be in `pixel_ids`. If it is not, the event does not render.
- JavaScript: `window.autumnMetaPixel.track({name: "Lead", custom: false, params: {}})`. It exists only when the pixel is on.

### htmx

- htmx reads one `HX-Trigger` header. Do not add a second one.
- With `htmx.config.allowScriptTags = false`, htmx removes the event data blocks from swapped content. Use `hx_trigger_value` then.
- Content that your JavaScript adds does not get an `htmx:load` event. Call `window.autumnMetaPixel.scan(element)`.
- `hx-boost` does not swap `<head>`. A page that gets consent through a boosted request gets the pixel on the next full load. Send `HX-Refresh: true`, or use `hx-boost="false"` on the consent form.

## Consent

With `require_consent = true` (default), the plugin renders nothing until
`Consent::allows(consent_category, consent_policy_version)` is `true`. Record consent with the same category, and give the plugin your policy version:

```rust,ignore
autumn_web::consent::accept_all_cookie(&["marketing"], POLICY_VERSION)
MetaPixelPlugin::new().consent_policy_version(POLICY_VERSION)
```

When the visitor withdraws consent, send `REVOKE_HX_TRIGGER` as the `HX-Trigger` value (or `HX-Refresh: true`). With `hx-boost`, the page and `fbevents.js` stay in memory. The loader then calls `fbq('consent', 'revoke')`, stops `pushState` page views, and sends no more events.

Pages with the pixel vary per visitor. Do not use `CacheResponseLayer` for them (see autumn's cookie-consent guide).

## Content-Security-Policy

The pixel needs these sources. autumn's default CSP does not have them.

| Directive | Source |
|---|---|
| `script-src` | `'self'` and `https://connect.facebook.net` (the origin of `script_url`) |
| `img-src` | `https://www.facebook.com` |
| `connect-src` | `https://www.facebook.com` |

When the pixel is on, the plugin checks `security.headers.content_security_policy` at startup. When a source is missing, the start fails and the error gives the fixed policy. For autumn's default CSP:

```toml
[security.headers]
content_security_policy = "default-src 'self'; img-src 'self' data: https://www.facebook.com; style-src 'self' 'unsafe-inline'; script-src 'self' https://connect.facebook.net; connect-src 'self' https://www.facebook.com; form-action 'self'; frame-ancestors 'none'; base-uri 'self'"
```

- `csp::with_meta_pixel_sources(csp, origin)` gives the same fix in code. Each policy of a comma-separated list gets its own fix.
- An explicit CSP turns off autumn's automatic nonce. Then inline scripts with a `CspNonce` do not run.
- `'strict-dynamic'` and `require-trusted-types-for` block the loader. The plugin does not support them.

## Config

Sources, in order (later wins): `[meta_pixel]` in `autumn.toml`, `[profile.<name>.meta_pixel]`, `autumn-<profile>.toml`, `.env`, then `AUTUMN_META_PIXEL__<KEY>` env vars. A list is comma-separated. A boolean is `true`, `false`, `1`, or `0`.

| Key | Default | Meaning |
|---|---|---|
| `enabled` | `true` | `false` turns the pixel off. |
| `pixel_ids` | `[]` | Pixel IDs, 1 to 32 digits each, no duplicates. Empty means off. |
| `page_view` | `true` | Send `PageView` at page load. |
| `history_page_views` | `true` | Let the pixel send `PageView` on `pushState` (`hx-push-url`). `false` sets `fbq.disablePushState`. |
| `auto_config` | `true` | Let the pixel collect clicks and page metadata. `false` sends `fbq('set', 'autoConfig', false, id)`. |
| `noscript` | `true` | Render the `noscript` image. Needs `page_view = true`. |
| `require_consent` | `true` | Render nothing before consent. |
| `consent_category` | `"marketing"` | Consent category, from `A-Z a-z 0-9 _ -`. Not `necessary`. |
| `consent_policy_version` | `1` | Your cookie policy version. `MetaPixelPlugin::consent_policy_version` overrides it. |
| `honor_gpc` | `true` | Off for `Sec-GPC: 1` and `navigator.globalPrivacyControl`. |
| `script_url` | `https://connect.facebook.net/en_US/fbevents.js` | `fbevents.js` URL. HTTPS only. |
| `csp_check` | `"error"` | `"error"`, `"warn"`, or `"off"`. |

## Test your templates

```rust,ignore
let pixel = MetaPixel::for_request(&config, &headers);
assert!(pixel.head().into_string().contains("autumn-meta-pixel-config"));
```

`MetaPixelPlugin::new()` reads `autumn.toml` and env vars, also in a `TestApp`. In tests, use `MetaPixelPlugin::with_config(cfg)`. Set a CSP with the pixel sources, or `cfg.csp_check = CspCheck::Off`.

## Security notes

- Config and events are JSON in `<script type="application/json">` blocks. The JSON escapes `<`, `>`, and `&`, so text cannot close the block. `verus/policy.rs` proves this.
- `fbevents.js` is third-party code with full page access. Meta changes it often, so it has no SRI hash. The consent gate, GPC, and CSP host list limit it.
- The loader fires each event block and `data-meta-pixel` attribute in the page. If you show user HTML, your sanitizer must remove `<script>` and the `data-meta-pixel` and `data-autumn-meta-pixel-event` attributes.
- The plugin sends no personal data. Event parameters come from your code. Do not put e-mail addresses or phone numbers in them.

## Out of scope

Conversions API (server events; use `event_id` to match them later), advanced matching, a consent banner (autumn has one).

## Develop

See [CLAUDE.md](CLAUDE.md), [docs/plan.md](docs/plan.md), [docs/adr](docs/adr), and [docs/ac-evidence.md](docs/ac-evidence.md).

License: Apache-2.0.
