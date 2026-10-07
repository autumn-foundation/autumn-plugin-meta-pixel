//! Typed Meta Pixel events.
//!
//! One JSON format serves the three fire paths: a data block, an
//! `HX-Trigger` header, and a `data-meta-pixel` attribute.
//!
//! ```json
//! {"name":"Purchase","custom":false,"params":{"value":9.5,"currency":"EUR"},
//!  "eventId":"order-42","pixelId":"1234567890"}
//! ```

use serde_json::{Map, Value};

use crate::error::MetaPixelError;
use crate::policy::is_valid_event_name;

/// A Meta standard event. The loader sends it with `fbq('track', ...)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum StandardEvent {
    /// Payment details added in checkout.
    AddPaymentInfo,
    /// Item added to the cart.
    AddToCart,
    /// Item added to a wish list.
    AddToWishlist,
    /// Registration form done.
    CompleteRegistration,
    /// Contact made (phone, SMS, e-mail, chat).
    Contact,
    /// Product customized.
    CustomizeProduct,
    /// Donation made.
    Donate,
    /// Location search.
    FindLocation,
    /// Checkout started.
    InitiateCheckout,
    /// Lead sent (form, trial sign-up).
    Lead,
    /// Page view. `head()` sends one by default.
    PageView,
    /// Purchase done. Set `value` and `currency`.
    Purchase,
    /// Appointment booked.
    Schedule,
    /// Search done.
    Search,
    /// Free trial started.
    StartTrial,
    /// Application sent.
    SubmitApplication,
    /// Paid subscription started.
    Subscribe,
    /// Key page viewed (product page, landing page).
    ViewContent,
}

impl StandardEvent {
    /// All standard events.
    pub const ALL: &'static [Self] = &[
        Self::AddPaymentInfo,
        Self::AddToCart,
        Self::AddToWishlist,
        Self::CompleteRegistration,
        Self::Contact,
        Self::CustomizeProduct,
        Self::Donate,
        Self::FindLocation,
        Self::InitiateCheckout,
        Self::Lead,
        Self::PageView,
        Self::Purchase,
        Self::Schedule,
        Self::Search,
        Self::StartTrial,
        Self::SubmitApplication,
        Self::Subscribe,
        Self::ViewContent,
    ];

    /// The name Meta uses, for example `"AddToCart"`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AddPaymentInfo => "AddPaymentInfo",
            Self::AddToCart => "AddToCart",
            Self::AddToWishlist => "AddToWishlist",
            Self::CompleteRegistration => "CompleteRegistration",
            Self::Contact => "Contact",
            Self::CustomizeProduct => "CustomizeProduct",
            Self::Donate => "Donate",
            Self::FindLocation => "FindLocation",
            Self::InitiateCheckout => "InitiateCheckout",
            Self::Lead => "Lead",
            Self::PageView => "PageView",
            Self::Purchase => "Purchase",
            Self::Schedule => "Schedule",
            Self::Search => "Search",
            Self::StartTrial => "StartTrial",
            Self::SubmitApplication => "SubmitApplication",
            Self::Subscribe => "Subscribe",
            Self::ViewContent => "ViewContent",
        }
    }
}

impl std::fmt::Display for StandardEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One item in `contents`.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Content {
    /// Product ID (SKU).
    pub id: String,
    /// Number of items.
    pub quantity: u32,
    /// Price of one item. The JSON does not include it when it is `None` or
    /// not finite.
    pub item_price: Option<f64>,
}

impl Content {
    /// An item with this ID and quantity.
    #[must_use]
    pub fn new(id: impl Into<String>, quantity: u32) -> Self {
        Self {
            id: id.into(),
            quantity,
            item_price: None,
        }
    }

    /// Sets the price of one item.
    #[must_use]
    pub const fn item_price(mut self, price: f64) -> Self {
        self.item_price = Some(price);
        self
    }

    fn to_json(&self) -> Value {
        let mut out = Map::new();
        out.insert("id".to_owned(), Value::String(self.id.clone()));
        out.insert("quantity".to_owned(), Value::from(self.quantity));
        if let Some(n) = self.item_price.and_then(serde_json::Number::from_f64) {
            out.insert("item_price".to_owned(), Value::Number(n));
        }
        Value::Object(out)
    }
}

/// A pixel event: a standard or a custom event, with parameters.
///
/// ```
/// use autumn_plugin_meta_pixel::{Content, Event, StandardEvent};
///
/// let purchase = Event::standard(StandardEvent::Purchase)
///     .value(25.0)
///     .currency("EUR")
///     .contents(&[Content::new("sku-1", 1)])
///     .event_id("order-1001");
/// assert_eq!(purchase.to_json()["params"]["currency"], "EUR");
///
/// let share = Event::custom("ShareClick")?.param("channel", "mail");
/// assert!(share.is_custom());
/// assert!(Event::custom("no spaces").is_err());
/// # Ok::<(), autumn_plugin_meta_pixel::MetaPixelError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    name: String,
    custom: bool,
    params: Map<String, Value>,
    dedup_id: Option<String>,
    pixel_id: Option<String>,
}

impl From<StandardEvent> for Event {
    fn from(event: StandardEvent) -> Self {
        Self::standard(event)
    }
}

impl Event {
    /// A standard event.
    #[must_use]
    pub fn standard(event: StandardEvent) -> Self {
        Self {
            name: event.as_str().to_owned(),
            custom: false,
            params: Map::new(),
            dedup_id: None,
            pixel_id: None,
        }
    }

    /// A custom event. The loader sends it with `fbq('trackCustom', ...)`.
    ///
    /// # Errors
    /// Returns [`MetaPixelError::EventName`] when `name` is not 1 to 50 characters from
    /// `A-Z a-z 0-9 _`.
    pub fn custom(name: &str) -> Result<Self, MetaPixelError> {
        if !is_valid_event_name(name) {
            return Err(MetaPixelError::EventName(name.to_owned()));
        }
        Ok(Self {
            name: name.to_owned(),
            custom: true,
            params: Map::new(),
            dedup_id: None,
            pixel_id: None,
        })
    }

    /// The event name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// `true` for a custom event.
    #[must_use]
    pub const fn is_custom(&self) -> bool {
        self.custom
    }

    /// The single pixel target, if set.
    #[must_use]
    pub fn pixel_id(&self) -> Option<&str> {
        self.pixel_id.as_deref()
    }

    /// The parameters.
    #[must_use]
    pub const fn params(&self) -> &Map<String, Value> {
        &self.params
    }

    /// Sets `value`. A value that is not finite is not sent.
    #[must_use]
    pub fn value(self, value: f64) -> Self {
        self.number("value", value)
    }

    /// Sets `currency`, an ISO 4217 code such as `"EUR"`.
    #[must_use]
    pub fn currency(self, code: &str) -> Self {
        self.param("currency", code)
    }

    /// Sets `content_ids`.
    #[must_use]
    pub fn content_ids<I, S>(self, ids: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let ids: Vec<Value> = ids.into_iter().map(|s| Value::String(s.into())).collect();
        self.param("content_ids", ids)
    }

    /// Sets `content_type`: `"product"` or `"product_group"`.
    #[must_use]
    pub fn content_type(self, kind: &str) -> Self {
        self.param("content_type", kind)
    }

    /// Sets `content_name`.
    #[must_use]
    pub fn content_name(self, name: &str) -> Self {
        self.param("content_name", name)
    }

    /// Sets `content_category`.
    #[must_use]
    pub fn content_category(self, category: &str) -> Self {
        self.param("content_category", category)
    }

    /// Sets `contents`.
    #[must_use]
    pub fn contents(self, items: &[Content]) -> Self {
        let items: Vec<Value> = items.iter().map(Content::to_json).collect();
        self.param("contents", items)
    }

    /// Sets `num_items`.
    #[must_use]
    pub fn num_items(self, count: u32) -> Self {
        self.param("num_items", count)
    }

    /// Sets `predicted_ltv`. A value that is not finite is not sent.
    #[must_use]
    pub fn predicted_ltv(self, value: f64) -> Self {
        self.number("predicted_ltv", value)
    }

    /// Sets `search_string`.
    #[must_use]
    pub fn search_string(self, query: &str) -> Self {
        self.param("search_string", query)
    }

    /// Sets `status`, for `CompleteRegistration`.
    #[must_use]
    pub fn status(self, done: bool) -> Self {
        self.param("status", done)
    }

    /// Sets `delivery_category`: `"in_store"`, `"curbside"`, or
    /// `"home_delivery"`.
    #[must_use]
    pub fn delivery_category(self, category: &str) -> Self {
        self.param("delivery_category", category)
    }

    /// Sets one parameter. Use it for custom properties. A later call with
    /// the same key replaces the value.
    #[must_use]
    pub fn param(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.params.insert(key.to_owned(), value.into());
        self
    }

    /// Sets a number parameter. Removes it when `value` is not finite.
    fn number(mut self, key: &str, value: f64) -> Self {
        match serde_json::Number::from_f64(value) {
            Some(n) => {
                self.params.insert(key.to_owned(), Value::Number(n));
            }
            None => {
                self.params.remove(key);
            }
        }
        self
    }

    /// Sets the `eventID`. Meta uses it to drop a duplicate of a server
    /// (Conversions API) event.
    #[must_use]
    pub fn event_id(mut self, id: impl Into<String>) -> Self {
        self.dedup_id = Some(id.into());
        self
    }

    /// Sends the event to this pixel only. The pixel must be in
    /// `pixel_ids`. If it is not, the event does not render.
    #[must_use]
    pub fn for_pixel(mut self, pixel_id: impl Into<String>) -> Self {
        self.pixel_id = Some(pixel_id.into());
        self
    }

    /// The JSON form that the loader reads.
    #[must_use]
    pub fn to_json(&self) -> Value {
        let mut out = Map::new();
        out.insert("name".to_owned(), Value::String(self.name.clone()));
        out.insert("custom".to_owned(), Value::Bool(self.custom));
        out.insert("params".to_owned(), Value::Object(self.params.clone()));
        if let Some(id) = &self.dedup_id {
            out.insert("eventId".to_owned(), Value::String(id.clone()));
        }
        if let Some(id) = &self.pixel_id {
            out.insert("pixelId".to_owned(), Value::String(id.clone()));
        }
        Value::Object(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn standard_names_match_meta() {
        let names: Vec<&str> = StandardEvent::ALL.iter().map(|e| e.as_str()).collect();
        assert_eq!(
            names,
            [
                "AddPaymentInfo",
                "AddToCart",
                "AddToWishlist",
                "CompleteRegistration",
                "Contact",
                "CustomizeProduct",
                "Donate",
                "FindLocation",
                "InitiateCheckout",
                "Lead",
                "PageView",
                "Purchase",
                "Schedule",
                "Search",
                "StartTrial",
                "SubmitApplication",
                "Subscribe",
                "ViewContent",
            ]
        );
        assert_eq!(StandardEvent::Lead.to_string(), "Lead");
    }

    #[test]
    fn purchase_json() {
        let ev = Event::standard(StandardEvent::Purchase)
            .value(19.5)
            .currency("EUR")
            .content_ids(["sku-1", "sku-2"])
            .content_type("product")
            .contents(&[
                Content::new("sku-1", 2).item_price(4.5),
                Content::new("sku-2", 1),
            ])
            .num_items(3)
            .event_id("order-42");
        assert_eq!(
            ev.to_json(),
            json!({
                "name": "Purchase",
                "custom": false,
                "params": {
                    "value": 19.5,
                    "currency": "EUR",
                    "content_ids": ["sku-1", "sku-2"],
                    "content_type": "product",
                    "contents": [
                        {"id": "sku-1", "quantity": 2, "item_price": 4.5},
                        {"id": "sku-2", "quantity": 1}
                    ],
                    "num_items": 3
                },
                "eventId": "order-42"
            })
        );
    }

    #[test]
    fn all_param_setters() {
        let ev = Event::from(StandardEvent::CompleteRegistration)
            .content_name("Pro")
            .content_category("plans")
            .predicted_ltv(120.0)
            .search_string("shoes")
            .status(true)
            .delivery_category("curbside")
            .param("plan", "pro")
            .param("plan", "team");
        assert_eq!(
            ev.to_json()["params"],
            json!({
                "content_name": "Pro",
                "content_category": "plans",
                "predicted_ltv": 120.0,
                "search_string": "shoes",
                "status": true,
                "delivery_category": "curbside",
                "plan": "team"
            })
        );
    }

    #[test]
    fn non_finite_numbers_are_not_sent() {
        let ev = Event::standard(StandardEvent::Purchase)
            .value(f64::NAN)
            .predicted_ltv(f64::INFINITY)
            .contents(&[Content::new("a", 1).item_price(f64::NEG_INFINITY)]);
        assert_eq!(
            ev.to_json()["params"],
            json!({"contents": [{"id": "a", "quantity": 1}]})
        );
    }

    #[test]
    fn custom_event_checks_name() {
        let ev = Event::custom("ShareClick").unwrap().for_pixel("123");
        assert!(ev.is_custom());
        assert_eq!(ev.name(), "ShareClick");
        assert_eq!(ev.pixel_id(), Some("123"));
        assert_eq!(
            ev.to_json(),
            json!({"name": "ShareClick", "custom": true, "params": {}, "pixelId": "123"})
        );
        assert_eq!(
            Event::custom("bad name").unwrap_err(),
            MetaPixelError::EventName("bad name".to_owned())
        );
        assert!(Event::custom("</script>").is_err());
        assert!(Event::custom("").is_err());
    }
}
