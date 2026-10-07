//! A small shop page with the Meta Pixel.
//!
//! Run:
//!
//! ```sh
//! AUTUMN_META_PIXEL__PIXEL_IDS=1234567890 \
//! AUTUMN_SECURITY__HEADERS__CONTENT_SECURITY_POLICY="default-src 'self'; \
//!   script-src 'self' https://connect.facebook.net; \
//!   img-src 'self' https://www.facebook.com; connect-src 'self' https://www.facebook.com" \
//! cargo run --example app
//! ```
//!
//! Open `/`, then `/accept` to give consent. `tests/e2e/browser.mjs` drives
//! this app in Chromium.

use autumn_plugin_meta_pixel::{Content, Event, MetaPixel, MetaPixelPlugin, StandardEvent};
use autumn_web::assets::asset_url;
use autumn_web::prelude::*;
use autumn_web::reexports::axum::response::{AppendHeaders, Redirect};

/// The app's cookie policy version. Same value as `consent_policy_version`.
const POLICY_VERSION: u32 = 1;

#[get("/")]
async fn index(pixel: MetaPixel) -> Markup {
    let view = Event::standard(StandardEvent::ViewContent)
        .content_ids(["sku-1"])
        .content_type("product")
        .value(25.0)
        .currency("EUR");
    let contact = Event::standard(StandardEvent::Contact);
    html! {
        (maud::DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                title { "Shop" }
                (pixel.head())
                script src=(asset_url("js/htmx.min.js")) defer {}
            }
            body {
                (pixel.noscript())
                (pixel.track(&view))
                h1 { "Blue mug" }
                button id="contact" data-meta-pixel=[pixel.click_attr(&contact)] { "Contact us" }
                button id="add" hx-get="/cart" hx-target="#cart" { "Add to cart" }
                div id="cart" {}
                a href="/thanks" { "Buy" }
            }
        }
    }
}

/// An htmx fragment. The event fires from the `HX-Trigger` header.
#[get("/cart")]
async fn cart(pixel: MetaPixel) -> impl IntoResponse {
    let add = Event::standard(StandardEvent::AddToCart)
        .contents(&[Content::new("sku-1", 1).item_price(25.0)])
        .value(25.0)
        .currency("EUR");
    let header: Vec<(&str, String)> = pixel
        .hx_trigger_value(std::slice::from_ref(&add))
        .map(|v| ("hx-trigger", v))
        .into_iter()
        .collect();
    let lead = Event::custom("CartOpened").unwrap_or_else(|_| Event::from(StandardEvent::Lead));
    (
        AppendHeaders(header),
        html! { p { "1 item" } (pixel.track(&lead)) },
    )
}

#[get("/thanks")]
async fn thanks(pixel: MetaPixel) -> Markup {
    let purchase = Event::standard(StandardEvent::Purchase)
        .value(25.0)
        .currency("EUR")
        .event_id("order-1001");
    html! {
        (maud::DOCTYPE)
        html lang="en" {
            head { meta charset="utf-8"; title { "Thanks" } (pixel.head()) }
            body { (pixel.noscript()) (pixel.track(&purchase)) h1 { "Thank you" } }
        }
    }
}

/// Demo only: a real app records consent with a CSRF-protected form (see
/// autumn's cookie-consent guide).
#[get("/accept")]
async fn accept() -> impl IntoResponse {
    let cookie = autumn_web::consent::accept_all_cookie(&["marketing"], POLICY_VERSION);
    (AppendHeaders([("set-cookie", cookie)]), Redirect::to("/"))
}

#[autumn_web::main]
async fn main() {
    autumn_web::app()
        .routes(routes![index, cart, thanks, accept])
        .plugin(MetaPixelPlugin::new())
        .run()
        .await;
}
