//! The plugin in an app, through the full autumn middleware stack.
#![allow(clippy::unwrap_used, clippy::expect_used)] // Test helpers fail loudly.

use autumn_plugin_meta_pixel::assets::{ASSETS, LOADER, NAMESPACE};
use autumn_plugin_meta_pixel::{
    CspCheck, Event, MetaPixel, MetaPixelConfig, MetaPixelPlugin, PLUGIN_NAME, StandardEvent,
};
use autumn_web::config::AutumnConfig;
use autumn_web::plugin::Plugin;
use autumn_web::prelude::*;
use autumn_web::route_listing::{RouteClassification, RouteSource};
use autumn_web::test::{TestApp, TestClient};

const FB: &str = "https://connect.facebook.net";

#[get("/")]
async fn index(pixel: MetaPixel) -> Markup {
    let purchase = Event::standard(StandardEvent::Purchase)
        .value(9.5)
        .currency("EUR")
        .event_id("order-1");
    let call = Event::standard(StandardEvent::Contact);
    html! {
        (maud::DOCTYPE)
        html {
            head { (pixel.head()) }
            body {
                (pixel.noscript())
                (pixel.track(&purchase))
                button data-meta-pixel=[pixel.click_attr(&call)] { "Call" }
            }
        }
    }
}

#[get("/partial")]
async fn partial(pixel: MetaPixel) -> impl IntoResponse {
    let lead = Event::standard(StandardEvent::Lead);
    let mut headers = Vec::new();
    if let Some(v) = pixel.hx_trigger_value(std::slice::from_ref(&lead)) {
        headers.push(("hx-trigger", v));
    }
    let body = html! { div { (pixel.track(&lead)) } };
    (
        autumn_web::reexports::axum::response::AppendHeaders(headers),
        body,
    )
}

/// Autumn defaults, with the CSP fixed for the pixel.
fn app_config() -> AutumnConfig {
    let mut cfg = AutumnConfig::default();
    cfg.security.headers.content_security_policy =
        autumn_plugin_meta_pixel::csp::with_meta_pixel_sources(
            &cfg.security.headers.content_security_policy,
            FB,
        );
    cfg
}

fn pixel_config() -> MetaPixelConfig {
    MetaPixelConfig::with_pixel_ids(["111", "222"])
}

fn app(plugin: MetaPixelPlugin) -> TestClient {
    TestApp::new()
        .config(app_config())
        .routes(routes![index, partial])
        .plugin(plugin)
        .build()
}

fn consent_cookie() -> String {
    let set = autumn_web::consent::accept_all_cookie(&["marketing"], 1);
    set.split(';').next().unwrap().to_owned()
}

// ------------------------------------------------------------ assets (AC2)

#[tokio::test(flavor = "multi_thread")]
async fn loader_is_served_at_hashed_immutable_url() {
    let client = app(MetaPixelPlugin::with_config(pixel_config()));
    let loader = ASSETS.get(LOADER).unwrap();
    assert!(
        loader
            .url()
            .starts_with("/static/_plugins/meta-pixel/meta-pixel.")
    );
    let r = client.get(loader.url()).send().await;
    r.assert_status(200);
    r.assert_header_contains("cache-control", "immutable");
    r.assert_header_contains("content-type", "javascript");
    assert_eq!(r.text(), include_str!("../assets/meta-pixel.js"));

    let r = client
        .get("/static/_plugins/meta-pixel/meta-pixel.js")
        .send()
        .await;
    r.assert_status(200);
    r.assert_header_contains("cache-control", "must-revalidate");
    assert_eq!(
        autumn_web::assets::asset_url("_plugins/meta-pixel/meta-pixel.js"),
        loader.url()
    );
}

#[test]
fn routes_are_public_attributed_and_conformant() {
    let plugin = MetaPixelPlugin::new();
    let contract = plugin.contract().unwrap();
    let builder = autumn_web::app().plugin(plugin);
    let routes = builder.plugin_route_infos().unwrap();
    let mine: Vec<_> = routes
        .iter()
        .filter(|r| {
            r.path
                .starts_with(&format!("/static/_plugins/{NAMESPACE}/"))
        })
        .collect();
    assert_eq!(mine.len(), 2, "{routes:#?}");
    for r in &mine {
        assert_eq!(r.method, "GET");
        assert_eq!(r.classification, RouteClassification::Public);
        assert_eq!(r.source, RouteSource::Plugin(PLUGIN_NAME.to_owned()));
    }
    let report = autumn_web::plugin_conformance::run_conformance(
        &autumn_web::plugin_conformance::ConformanceConfig::new(PLUGIN_NAME)
            .contract(contract)
            .prefix(format!("/static/_plugins/{NAMESPACE}")),
        &routes,
    );
    assert!(report.passed(), "{}", report.to_text_report());
    assert!(builder.has_config_section("meta_pixel"));
}

// ------------------------------------------------------------ page (AC3, AC4, AC6)

#[tokio::test(flavor = "multi_thread")]
async fn page_with_consent_gets_pixel_and_events() {
    let client = app(MetaPixelPlugin::with_config(pixel_config()));
    let r = client
        .get("/")
        .header("cookie", &consent_cookie())
        .send()
        .await;
    r.assert_status(200);
    let body = r.text();
    let loader = ASSETS.get(LOADER).unwrap();
    assert!(body.contains(r#"id="autumn-meta-pixel-config""#), "{body}");
    assert!(
        body.contains(&format!(r#"src="{}""#, loader.url())),
        "{body}"
    );
    assert!(
        body.contains(&format!(r#"integrity="{}""#, loader.integrity())),
        "{body}"
    );
    assert!(body.contains("data-autumn-meta-pixel-event"), "{body}");
    assert!(body.contains(r#""eventId":"order-1""#), "{body}");
    assert!(
        body.contains("tr?id=111&amp;ev=PageView&amp;noscript=1"),
        "{body}"
    );
    assert!(body.contains("data-meta-pixel=\"{&quot;"), "{body}");
    // The CSP the app sends allows the pixel.
    let csp = r.header("content-security-policy").unwrap().to_owned();
    assert!(
        autumn_plugin_meta_pixel::csp::missing_sources(&csp, FB).is_empty(),
        "{csp}"
    );
}

// ------------------------------------------------------------ gate (AC7)

#[tokio::test(flavor = "multi_thread")]
async fn no_consent_no_pixel() {
    let client = app(MetaPixelPlugin::with_config(pixel_config()));
    let body = client.get("/").send().await.text();
    assert!(!body.contains("meta-pixel"), "{body}");
    assert!(!body.contains("facebook"), "{body}");
    let r = client.get("/partial").send().await;
    assert_eq!(r.header("hx-trigger"), None);
    assert!(!r.text().contains("meta-pixel"));
}

#[tokio::test(flavor = "multi_thread")]
async fn gpc_request_gets_no_pixel() {
    let client = app(MetaPixelPlugin::with_config(pixel_config()));
    let body = client
        .get("/")
        .header("cookie", &consent_cookie())
        .header("sec-gpc", "1")
        .send()
        .await
        .text();
    assert!(!body.contains("meta-pixel"), "{body}");
}

#[tokio::test(flavor = "multi_thread")]
async fn disabled_config_gets_no_pixel() {
    let mut cfg = pixel_config();
    cfg.enabled = false;
    // Off: the CSP check does not run, so the default CSP is fine.
    let client = TestApp::new()
        .routes(routes![index])
        .plugin(MetaPixelPlugin::with_config(cfg))
        .build();
    let body = client
        .get("/")
        .header("cookie", &consent_cookie())
        .send()
        .await
        .text();
    assert!(!body.contains("meta-pixel"), "{body}");
}

#[tokio::test(flavor = "multi_thread")]
async fn htmx_partial_gets_trigger_header_and_block() {
    let client = app(MetaPixelPlugin::with_config(pixel_config()));
    let r = client
        .get("/partial")
        .header("cookie", &consent_cookie())
        .header("hx-request", "true")
        .send()
        .await;
    r.assert_status(200);
    let trigger: serde_json::Value = serde_json::from_str(r.header("hx-trigger").unwrap()).unwrap();
    assert_eq!(trigger["autumn:meta-pixel"]["events"][0]["name"], "Lead");
    assert!(r.text().contains("data-autumn-meta-pixel-event"));
}

// ------------------------------------------------------------ startup (AC1, AC8)

#[tokio::test(flavor = "multi_thread")]
async fn no_plugin_extractor_gives_off_pixel() {
    let client = TestApp::new().routes(routes![index]).build();
    let r = client
        .get("/")
        .header("cookie", &consent_cookie())
        .send()
        .await;
    r.assert_status(200);
    assert!(!r.text().contains("meta-pixel"));
}

#[tokio::test(flavor = "multi_thread")]
async fn new_with_no_config_file_starts_off() {
    // No autumn.toml in the crate root: no pixel IDs, so the pixel is off.
    let client = TestApp::new()
        .routes(routes![index])
        .plugin(MetaPixelPlugin::new())
        .build();
    let body = client
        .get("/")
        .header("cookie", &consent_cookie())
        .send()
        .await
        .text();
    assert!(!body.contains("meta-pixel"), "{body}");
}

#[tokio::test(flavor = "multi_thread")]
#[should_panic(expected = "the Content-Security-Policy blocks the pixel")]
async fn default_csp_fails_startup() {
    let _ = TestApp::new()
        .routes(routes![index])
        .plugin(MetaPixelPlugin::with_config(pixel_config()))
        .build();
}

#[tokio::test(flavor = "multi_thread")]
async fn csp_check_warn_starts() {
    let mut cfg = pixel_config();
    cfg.csp_check = CspCheck::Warn;
    let client = TestApp::new()
        .routes(routes![index])
        .plugin(MetaPixelPlugin::with_config(cfg))
        .build();
    let body = client
        .get("/")
        .header("cookie", &consent_cookie())
        .send()
        .await
        .text();
    assert!(body.contains("autumn-meta-pixel-config"), "{body}");
}

#[tokio::test(flavor = "multi_thread")]
#[should_panic(expected = "pixel_ids: \\\"12-34\\\" is not valid")]
async fn bad_pixel_id_fails_startup() {
    let _ = app(MetaPixelPlugin::with_config(
        MetaPixelConfig::with_pixel_ids(["12-34"]),
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn plugin_installed_twice_is_harmless() {
    let client = TestApp::new()
        .config(app_config())
        .routes(routes![index])
        .plugin(MetaPixelPlugin::with_config(pixel_config()))
        .plugin(MetaPixelPlugin::with_config(pixel_config()))
        .build();
    let body = client
        .get("/")
        .header("cookie", &consent_cookie())
        .send()
        .await
        .text();
    assert_eq!(
        body.matches("autumn-meta-pixel-config").count(),
        1,
        "{body}"
    );
}
