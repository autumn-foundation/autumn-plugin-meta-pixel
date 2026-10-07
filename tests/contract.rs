//! The event JSON contract between Rust and the loader.
//!
//! `tests/fixtures/events.json` holds each event and the `fbq` calls that the
//! loader must make. This test checks the Rust side. `tests/js/` checks the
//! loader side with the same file.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // Test helpers fail loudly.

use autumn_plugin_meta_pixel::{Event, MetaPixel, MetaPixelConfig, StandardEvent};
use autumn_web::reexports::axum::http::HeaderMap;
use serde_json::Value;

fn build(id: &str) -> Event {
    match id {
        "purchase" => Event::standard(StandardEvent::Purchase)
            .value(9.5)
            .currency("EUR")
            .event_id("order-1"),
        "custom" => Event::custom("ShareClick")
            .unwrap()
            .param("channel", "mail"),
        "single" => Event::standard(StandardEvent::Lead).for_pixel("222"),
        "single-custom" => Event::custom("Ping")
            .unwrap()
            .event_id("e")
            .for_pixel("111"),
        other => panic!("no builder for fixture case {other}"),
    }
}

#[test]
fn rust_events_match_the_fixture() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/events.json")).unwrap();
    let ids: Vec<String> = fixture["pixelIds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    let mut config = MetaPixelConfig::with_pixel_ids(ids);
    config.require_consent = false;
    let pixel = MetaPixel::for_request(&config, &HeaderMap::new());
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 4);
    for case in cases {
        let event = build(case["id"].as_str().unwrap());
        assert_eq!(event.to_json(), case["event"], "{}", case["id"]);
        // The three fire paths carry the same JSON.
        let attr: Value = serde_json::from_str(&pixel.click_attr(&event).unwrap()).unwrap();
        assert_eq!(attr, case["event"]);
        let hx: Value = serde_json::from_str(
            &pixel
                .hx_trigger_value(std::slice::from_ref(&event))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(hx["autumn:meta-pixel"]["events"][0], case["event"]);
        let block = pixel.track(&event).into_string();
        let body = &block[block.find('{').unwrap()..block.rfind("</script>").unwrap()];
        assert_eq!(serde_json::from_str::<Value>(body).unwrap(), case["event"]);
    }
}
