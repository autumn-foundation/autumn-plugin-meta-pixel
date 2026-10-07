//! Errors.

/// A plugin error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum MetaPixelError {
    /// Bad `[meta_pixel]` config, or a config file that cannot be read.
    #[error("meta_pixel config: {0}")]
    Config(String),
    /// The Content-Security-Policy blocks the pixel.
    #[error("meta_pixel CSP: {0}")]
    Csp(String),
    /// A custom event name breaks the rules of
    /// [`is_valid_event_name`](crate::policy::is_valid_event_name).
    #[error("meta_pixel: custom event name {0:?} is not valid; use 1 to 50 of A-Z a-z 0-9 _")]
    EventName(String),
}
