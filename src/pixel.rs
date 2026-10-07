//! The `MetaPixel` extractor and its markup.

use std::convert::Infallible;
use std::fmt::Write as _;
use std::sync::{Arc, Once};

use autumn_web::AppState;
use autumn_web::consent::Consent;
use autumn_web::reexports::axum::extract::FromRequestParts;
use autumn_web::reexports::axum::http::HeaderMap;
use autumn_web::reexports::axum::http::request::Parts;
use maud::{Markup, PreEscaped, html};
use serde_json::Value;

use crate::assets::{ASSETS, LOADER};
use crate::config::MetaPixelConfig;
use crate::csp::HIT_ORIGIN;
use crate::event::Event;
use crate::policy::{Gate, active, escape_json_for_html};

/// `id` of the config data block.
pub const CONFIG_ELEMENT_ID: &str = "autumn-meta-pixel-config";
/// Attribute that marks an event data block.
pub const EVENT_ATTR: &str = "data-autumn-meta-pixel-event";
/// Attribute for click events. Its value is the event JSON.
pub const CLICK_ATTR: &str = "data-meta-pixel";
/// Name of the htmx event that `HX-Trigger` fires.
pub const HX_EVENT: &str = "autumn:meta-pixel";
/// `HX-Trigger` value that stops a pixel that runs in the page.
///
/// Send it with the response that records a consent withdrawal. With
/// `hx-boost`, the page and `fbevents.js` stay in memory across requests.
/// The loader then calls `fbq('consent', 'revoke')`, stops `pushState`
/// page views, and sends no more events. A full page load (`HX-Refresh`)
/// also works.
pub const REVOKE_HX_TRIGGER: &str = "autumn:meta-pixel-revoke";

/// Config shared by all requests. The plugin makes it once at startup.
#[derive(Debug)]
pub(crate) struct Runtime {
    config: MetaPixelConfig,
    /// The loader config, as escaped JSON.
    config_json: String,
}

impl Runtime {
    pub(crate) fn new(config: MetaPixelConfig) -> Self {
        let json = serde_json::json!({
            "pixelIds": config.pixel_ids,
            "pageView": config.page_view,
            "historyPageViews": config.history_page_views,
            "autoConfig": config.auto_config,
            "honorGpc": config.honor_gpc,
            "scriptUrl": config.script_url,
        });
        Self {
            config_json: escape_json_for_html(&json.to_string()),
            config,
        }
    }
}

/// The pixel for one request.
///
/// Take it as a handler argument. It never fails. When the pixel is off
/// (config, consent, GPC, or no plugin), each method gives empty markup or
/// `None`.
///
/// ```rust,ignore
/// #[get("/")]
/// async fn index(pixel: MetaPixel) -> Markup {
///     html! {
///         head { (pixel.head()) }
///         body { (pixel.noscript()) h1 { "Hello" } }
///     }
/// }
/// ```
#[derive(Debug, Clone, Default)]
pub struct MetaPixel {
    /// `Some` only when the pixel is on for this request.
    runtime: Option<Arc<Runtime>>,
    /// `HX-Request: true`. htmx parses the response with scripting off.
    htmx: bool,
    /// `HX-History-Restore-Request: true`. The page events fired before.
    history_restore: bool,
}

impl MetaPixel {
    /// A pixel that is off. All markup is empty.
    #[must_use]
    pub const fn off() -> Self {
        Self {
            runtime: None,
            htmx: false,
            history_restore: false,
        }
    }

    /// The pixel for a request with these headers. The extractor does the
    /// same with the config loaded at startup. Use it in template tests.
    ///
    /// ```
    /// use autumn_plugin_meta_pixel::{MetaPixel, MetaPixelConfig};
    ///
    /// let config = MetaPixelConfig::with_pixel_ids(["1234567890"]);
    /// // No consent cookie: the pixel is off.
    /// let pixel = MetaPixel::for_request(&config, &Default::default());
    /// assert!(pixel.head().into_string().is_empty());
    ///
    /// let mut open = config.clone();
    /// open.require_consent = false;
    /// let pixel = MetaPixel::for_request(&open, &Default::default());
    /// assert!(pixel.head().into_string().contains("autumn-meta-pixel-config"));
    /// ```
    #[must_use]
    ///
    /// A config that fails [`MetaPixelConfig::validate`] gives an off pixel.
    pub fn for_request(config: &MetaPixelConfig, headers: &HeaderMap) -> Self {
        if let Err(e) = config.validate() {
            tracing::warn!("meta_pixel: {e}; the pixel is off");
            return Self::off();
        }
        Self::decide(Arc::new(Runtime::new(config.clone())), headers)
    }

    fn decide(runtime: Arc<Runtime>, headers: &HeaderMap) -> Self {
        let c = &runtime.config;
        let gate = Gate {
            enabled: c.enabled,
            has_pixels: !c.pixel_ids.is_empty(),
            require_consent: c.require_consent,
            consent_granted: c.require_consent
                && Consent::from_headers(headers)
                    .allows(&c.consent_category, c.consent_policy_version),
            honor_gpc: c.honor_gpc,
            gpc_signal: headers
                .get("sec-gpc")
                .is_some_and(|v| v.as_bytes().trim_ascii() == b"1"),
        };
        Self {
            runtime: active(gate).then_some(runtime),
            htmx: is_true(headers, "hx-request"),
            history_restore: is_true(headers, "hx-history-restore-request"),
        }
    }

    /// `true` when this page gets the pixel.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.runtime.is_some()
    }

    /// Put this in `<head>`: the config data block and the loader tag.
    #[must_use]
    pub fn head(&self) -> Markup {
        let Some(rt) = &self.runtime else {
            return html! {};
        };
        html! {
            script type="application/json" id=(CONFIG_ELEMENT_ID) { (PreEscaped(&rt.config_json)) }
            (ASSETS.deferred_script_tag(LOADER))
        }
    }

    /// Put this at the start of `<body>`: a hidden `PageView` image for
    /// each pixel, for browsers with no JavaScript.
    ///
    /// Empty for an htmx request (`HX-Request: true`): htmx parses the
    /// response with scripting off, so the image would load.
    #[must_use]
    pub fn noscript(&self) -> Markup {
        let Some(rt) = &self.runtime else {
            return html! {};
        };
        let c = &rt.config;
        // An htmx response is parsed with scripting off: the image would load.
        if !(c.noscript && c.page_view) || self.htmx {
            return html! {};
        }
        html! {
            noscript data-autumn-meta-pixel {
                @for id in &c.pixel_ids {
                    img height="1" width="1" alt="" hidden
                        src=(format!("{HIT_ORIGIN}/tr?id={id}&ev=PageView&noscript=1"));
                }
            }
        }
    }

    /// An event data block. The loader fires it once, at page load or
    /// after an htmx swap.
    ///
    /// Empty for an htmx history restore (`HX-History-Restore-Request`):
    /// the events fired on the first visit.
    #[must_use]
    pub fn track(&self, event: &Event) -> Markup {
        if self.history_restore {
            return html! {};
        }
        let Some(json) = self.event_json(event) else {
            return html! {};
        };
        html! {
            script type="application/json" data-autumn-meta-pixel-event {
                (PreEscaped(escape_json_for_html(&json.to_string())))
            }
        }
    }

    /// The value for a `data-meta-pixel` attribute. A click on the element
    /// (or on a child) fires the event.
    ///
    /// ```rust,ignore
    /// html! { button data-meta-pixel=[pixel.click_attr(&lead)] { "Call us" } }
    /// ```
    #[must_use]
    pub fn click_attr(&self, event: &Event) -> Option<String> {
        self.event_json(event).map(|json| json.to_string())
    }

    /// An `HX-Trigger` value that fires `events` after an htmx request.
    ///
    /// Use it with `HtmxResponseExt::hx_trigger`. htmx reads one
    /// `HX-Trigger` header, so do not add a second one. The value is
    /// visible ASCII: other characters are `\uXXXX` escapes.
    #[must_use]
    pub fn hx_trigger_value(&self, events: &[Event]) -> Option<String> {
        let events: Vec<Value> = events.iter().filter_map(|e| self.event_json(e)).collect();
        if events.is_empty() {
            return None;
        }
        let value = serde_json::json!({ HX_EVENT: { "events": events } });
        Some(ascii_json(&value.to_string()))
    }

    /// The event JSON, or `None` when the pixel is off or the event targets
    /// a pixel that is not in `pixel_ids`.
    fn event_json(&self, event: &Event) -> Option<Value> {
        let rt = self.runtime.as_ref()?;
        if let Some(id) = event.pixel_id()
            && !rt.config.pixel_ids.iter().any(|p| p == id)
        {
            // Once: a bad target in a template must not fill the log.
            static WARNED: Once = Once::new();
            WARNED.call_once(|| {
                tracing::warn!(
                    pixel_id = id,
                    event = event.name(),
                    "meta_pixel: an event targets a pixel that is not in pixel_ids; not sent"
                );
            });
            return None;
        }
        Some(event.to_json())
    }
}

/// `true` when header `name` is `true`.
fn is_true(headers: &HeaderMap, name: &str) -> bool {
    headers
        .get(name)
        .is_some_and(|v| v.as_bytes().trim_ascii().eq_ignore_ascii_case(b"true"))
}

/// JSON text with each character outside `0x20..0x7f` as a `\uXXXX`
/// escape. JSON text has such characters only inside strings, where the
/// escape keeps the value.
fn ascii_json(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    let mut units = [0u16; 2];
    for c in json.chars() {
        if (' '..'\u{7f}').contains(&c) {
            out.push(c);
        } else {
            for unit in c.encode_utf16(&mut units) {
                // A write to a `String` cannot fail.
                let _ = write!(out, "\\u{unit:04x}");
            }
        }
    }
    out
}

impl FromRequestParts<AppState> for MetaPixel {
    type Rejection = Infallible;

    fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        std::future::ready(Ok(state.extension::<Runtime>().map_or_else(
            || {
                static WARNED: Once = Once::new();
                WARNED.call_once(|| {
                    tracing::warn!(
                        "meta_pixel: MetaPixelPlugin is not installed; the pixel is off"
                    );
                });
                Self::off()
            },
            |rt| Self::decide(rt, &parts.headers),
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::ASSETS;
    use crate::event::StandardEvent;
    use autumn_web::reexports::axum::http::HeaderValue;
    use autumn_web::reexports::axum::http::header::COOKIE;

    fn config() -> MetaPixelConfig {
        MetaPixelConfig::with_pixel_ids(["111", "222"])
    }

    fn consent_headers(category: &str, version: u32) -> HeaderMap {
        let set = autumn_web::consent::accept_all_cookie(&[category], version);
        let pair = set.split(';').next().unwrap().to_owned();
        let mut h = HeaderMap::new();
        h.insert(COOKIE, HeaderValue::from_str(&pair).unwrap());
        h
    }

    fn granted() -> HeaderMap {
        consent_headers("marketing", 1)
    }

    fn on() -> MetaPixel {
        MetaPixel::for_request(&config(), &granted())
    }

    #[test]
    fn consent_gate() {
        assert!(on().is_active());
        assert!(!MetaPixel::for_request(&config(), &HeaderMap::new()).is_active());
        assert!(!MetaPixel::for_request(&config(), &consent_headers("analytics", 1)).is_active());
        assert!(!MetaPixel::for_request(&config(), &consent_headers("marketing", 2)).is_active());
        let mut c = config();
        c.require_consent = false;
        assert!(MetaPixel::for_request(&c, &HeaderMap::new()).is_active());
        c.consent_category = "ads".into();
        c.require_consent = true;
        assert!(MetaPixel::for_request(&c, &consent_headers("ads", 1)).is_active());
    }

    #[test]
    fn gpc_and_off_switch() {
        let mut h = granted();
        h.insert("sec-gpc", HeaderValue::from_static("1"));
        assert!(!MetaPixel::for_request(&config(), &h).is_active());
        let mut c = config();
        c.honor_gpc = false;
        assert!(MetaPixel::for_request(&c, &h).is_active());
        h.insert("sec-gpc", HeaderValue::from_static("0"));
        assert!(MetaPixel::for_request(&config(), &h).is_active());
        let mut c = config();
        c.enabled = false;
        assert!(!MetaPixel::for_request(&c, &granted()).is_active());
        assert!(!MetaPixel::for_request(&MetaPixelConfig::default(), &granted()).is_active());
        assert!(!MetaPixel::off().is_active());
    }

    #[test]
    fn head_has_config_block_and_sri_loader_tag() {
        let html = on().head().into_string();
        let loader = ASSETS.get(crate::assets::LOADER).unwrap();
        assert!(
            html.contains(r#"<script type="application/json" id="autumn-meta-pixel-config">"#),
            "{html}"
        );
        assert!(
            html.contains(&format!(r#"src="{}""#, loader.url())),
            "{html}"
        );
        assert!(
            html.contains(&format!(r#"integrity="{}""#, loader.integrity())),
            "{html}"
        );
        assert!(html.contains(r#"crossorigin="anonymous""#), "{html}");
        assert!(html.contains(" defer"), "{html}");
        let start = html.find('{').unwrap();
        let end = html.find("</script>").unwrap();
        let cfg: serde_json::Value = serde_json::from_str(&html[start..end]).unwrap();
        assert_eq!(
            cfg,
            serde_json::json!({
                "pixelIds": ["111", "222"],
                "pageView": true,
                "historyPageViews": true,
                "autoConfig": true,
                "honorGpc": true,
                "scriptUrl": "https://connect.facebook.net/en_US/fbevents.js"
            })
        );
        // No inline JavaScript: each script tag is a JSON block or has a src.
        assert_eq!(html.matches("<script").count(), 2);
    }

    #[test]
    fn noscript_has_hidden_image_per_pixel() {
        let html = on().noscript().into_string();
        assert_eq!(
            html,
            "<noscript data-autumn-meta-pixel>\
             <img height=\"1\" width=\"1\" alt=\"\" hidden src=\"https://www.facebook.com/tr?id=111&amp;ev=PageView&amp;noscript=1\">\
             <img height=\"1\" width=\"1\" alt=\"\" hidden src=\"https://www.facebook.com/tr?id=222&amp;ev=PageView&amp;noscript=1\">\
             </noscript>"
        );
        let mut c = config();
        c.noscript = false;
        assert!(
            MetaPixel::for_request(&c, &granted())
                .noscript()
                .into_string()
                .is_empty()
        );
        let mut c = config();
        c.page_view = false;
        assert!(
            MetaPixel::for_request(&c, &granted())
                .noscript()
                .into_string()
                .is_empty()
        );
    }

    #[test]
    fn htmx_requests_get_no_noscript_image() {
        // htmx parses a response with scripting off, so the image would load.
        let mut h = granted();
        h.insert("hx-request", HeaderValue::from_static("true"));
        let p = MetaPixel::for_request(&config(), &h);
        assert!(p.is_active());
        assert!(p.noscript().into_string().is_empty());
        assert!(!p.head().into_string().is_empty());
        let ev = Event::standard(StandardEvent::Lead);
        assert!(!p.track(&ev).into_string().is_empty());
    }

    #[test]
    fn history_restore_requests_get_no_event_blocks() {
        // htmx re-fetches the page on a history cache miss. The events
        // fired on the first visit.
        let mut h = granted();
        h.insert("hx-request", HeaderValue::from_static("true"));
        h.insert(
            "hx-history-restore-request",
            HeaderValue::from_static("true"),
        );
        let p = MetaPixel::for_request(&config(), &h);
        let ev = Event::standard(StandardEvent::Lead);
        assert!(p.track(&ev).into_string().is_empty());
        assert!(p.noscript().into_string().is_empty());
        assert!(p.click_attr(&ev).is_some());
    }

    #[test]
    fn for_request_with_bad_config_is_off() {
        // Not validated, it would inject hit parameters and a `data:` script.
        let mut c = config();
        c.require_consent = false;
        c.pixel_ids = vec!["1&ev=Purchase".into()];
        assert!(!MetaPixel::for_request(&c, &HeaderMap::new()).is_active());
        let mut c = config();
        c.require_consent = false;
        c.script_url = "data:text/javascript,alert(1)".into();
        assert!(!MetaPixel::for_request(&c, &HeaderMap::new()).is_active());
    }

    #[test]
    fn revoke_trigger_is_an_htmx_event_name() {
        assert_eq!(REVOKE_HX_TRIGGER, "autumn:meta-pixel-revoke");
        assert!(HeaderValue::from_str(REVOKE_HX_TRIGGER).is_ok());
    }

    #[test]
    fn off_pixel_renders_nothing() {
        let off = MetaPixel::for_request(&config(), &HeaderMap::new());
        let ev = Event::standard(StandardEvent::Lead);
        assert!(off.head().into_string().is_empty());
        assert!(off.noscript().into_string().is_empty());
        assert!(off.track(&ev).into_string().is_empty());
        assert_eq!(off.click_attr(&ev), None);
        assert_eq!(off.hx_trigger_value(&[ev]), None);
    }

    #[test]
    fn track_renders_escaped_event_block() {
        let ev = Event::custom("Note")
            .unwrap()
            .param("text", "</script><script>alert(1)</script>&");
        let html = on().track(&ev).into_string();
        assert!(
            html.starts_with(r#"<script type="application/json" data-autumn-meta-pixel-event>"#),
            "{html}"
        );
        assert_eq!(html.matches("</script>").count(), 1, "{html}");
        assert!(!html.contains("<script>alert"), "{html}");
        let body = &html[html.find('{').unwrap()..html.rfind("</script>").unwrap()];
        let back: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(back, ev.to_json());
    }

    #[test]
    fn unknown_pixel_target_is_not_rendered() {
        let stranger = Event::standard(StandardEvent::Lead).for_pixel("999");
        let mine = Event::standard(StandardEvent::Lead).for_pixel("222");
        let p = on();
        assert!(p.track(&stranger).into_string().is_empty());
        assert!(!p.track(&mine).into_string().is_empty());
        assert_eq!(p.click_attr(&stranger), None);
        assert!(p.click_attr(&mine).is_some());
        assert_eq!(p.hx_trigger_value(std::slice::from_ref(&stranger)), None);
        let v: serde_json::Value =
            serde_json::from_str(&p.hx_trigger_value(&[stranger, mine.clone()]).unwrap()).unwrap();
        assert_eq!(v[HX_EVENT]["events"], serde_json::json!([mine.to_json()]));
    }

    #[test]
    fn click_attr_and_hx_trigger_use_event_json() {
        let p = on();
        let ev = Event::standard(StandardEvent::Contact).param("via", "<phone>");
        let attr = p.click_attr(&ev).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&attr).unwrap(),
            ev.to_json()
        );
        let markup = html! { button data-meta-pixel=[p.click_attr(&ev)] { "Call" } }.into_string();
        assert!(markup.contains("data-meta-pixel=\"{&quot;"), "{markup}");
        let hx = p.hx_trigger_value(std::slice::from_ref(&ev)).unwrap();
        let v: serde_json::Value = serde_json::from_str(&hx).unwrap();
        assert_eq!(
            v,
            serde_json::json!({ HX_EVENT: { "events": [ev.to_json()] } })
        );
        assert_eq!(p.hx_trigger_value(&[]), None);
    }

    #[test]
    fn hx_trigger_value_is_visible_ascii_and_round_trips() {
        let ev = Event::custom("Note")
            .unwrap()
            .param("t", "ü 😀 \u{7f} \u{2028} \n");
        let hx = on().hx_trigger_value(std::slice::from_ref(&ev)).unwrap();
        assert!(hx.bytes().all(|b| (0x20..0x7f).contains(&b)), "{hx}");
        assert!(HeaderValue::from_str(&hx).is_ok());
        let v: serde_json::Value = serde_json::from_str(&hx).unwrap();
        assert_eq!(v[HX_EVENT]["events"][0], ev.to_json());
    }
}
