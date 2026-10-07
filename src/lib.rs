//! Meta Pixel plugin for autumn-web 0.8.
//!
//! - **Loader:** a plugin asset (`PluginAssets`) with a content-hashed URL
//!   and SRI. No inline JavaScript.
//! - **Gate:** no pixel before consent (`autumn_web::consent`), none for a
//!   Global Privacy Control request, none when off in config.
//! - **Events:** typed standard and custom events. Fire them from a data
//!   block, an `HX-Trigger` header, or a click attribute.
//! - **Startup checks:** config and Content-Security-Policy.
//!
//! See the README for setup.

pub mod assets;
pub mod config;
pub mod csp;
mod error;
pub mod event;
mod pixel;
mod plugin;
pub mod policy;

pub use config::{CspCheck, MetaPixelConfig};
pub use error::MetaPixelError;
pub use event::{Content, Event, StandardEvent};
pub use pixel::{
    CLICK_ATTR, CONFIG_ELEMENT_ID, EVENT_ATTR, HX_EVENT, MetaPixel, REVOKE_HX_TRIGGER,
};
pub use plugin::{MetaPixelPlugin, PLUGIN_NAME};
