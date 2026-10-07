//! `MetaPixelPlugin`.

use std::borrow::Cow;
use std::fmt::Write as _;

use autumn_web::app::AppBuilder;
use autumn_web::config::AutumnConfig;
use autumn_web::plugin::Plugin;
use autumn_web::plugin_contract::PluginContract;
use autumn_web::{AppState, AutumnError};

use crate::assets::ASSETS;
use crate::config::{CspCheck, MetaPixelConfig, SECTION};
use crate::csp::{has_strict_dynamic, missing_sources, with_meta_pixel_sources};
use crate::error::MetaPixelError;
use crate::pixel::Runtime;

/// Plugin name for duplicate detection.
pub const PLUGIN_NAME: &str = "autumn-plugin-meta-pixel";

/// Meta Pixel plugin.
///
/// ```rust,ignore
/// autumn_web::app()
///     .routes(routes![index])
///     .plugin(MetaPixelPlugin::new())
///     .run()
///     .await;
/// ```
#[derive(Debug, Clone, Default)]
pub struct MetaPixelPlugin {
    config: Option<MetaPixelConfig>,
}

impl MetaPixelPlugin {
    /// Makes the plugin. It reads `[meta_pixel]` at startup.
    #[must_use]
    pub const fn new() -> Self {
        Self { config: None }
    }

    /// Makes the plugin with this config. It reads no files.
    #[must_use]
    pub const fn with_config(config: MetaPixelConfig) -> Self {
        Self {
            config: Some(config),
        }
    }

    /// Loads and checks the config, then shares it with the
    /// [`MetaPixel`](crate::MetaPixel) extractor. [`Plugin::build`] calls it
    /// at startup.
    ///
    /// # Errors
    /// Returns [`MetaPixelError::Config`] for bad config, and
    /// [`MetaPixelError::Csp`] when the CSP blocks the pixel and
    /// `csp_check = "error"`.
    pub fn start(self, state: &AppState) -> Result<(), MetaPixelError> {
        let config = match self.config {
            Some(c) => c,
            // autumn sets the state profile from the config; "default" means none.
            None => MetaPixelConfig::load(Some(state.profile()).filter(|p| *p != "default"))?,
        };
        config.validate()?;
        let on = config.enabled && !config.pixel_ids.is_empty();
        if config.enabled && config.pixel_ids.is_empty() {
            tracing::warn!("meta_pixel: enabled, but pixel_ids is empty; the pixel is off");
        }
        if on {
            check_csp(&config, &state.config())?;
        }
        tracing::info!(
            on,
            pixels = config.pixel_ids.len(),
            require_consent = config.require_consent,
            "meta_pixel started"
        );
        state.insert_extension(Runtime::new(config));
        Ok(())
    }
}

/// Checks the app CSP against the pixel hosts.
fn check_csp(config: &MetaPixelConfig, app: &AutumnConfig) -> Result<(), MetaPixelError> {
    if config.csp_check == CspCheck::Off {
        return Ok(());
    }
    let headers = &app.security.headers;
    let csp = &headers.content_security_policy;
    let origin = config
        .script_origin()
        .ok_or_else(|| MetaPixelError::Config("script_url has no https origin".to_owned()))?;
    let gaps = missing_sources(csp, &origin);
    let strict = has_strict_dynamic(csp);
    if gaps.is_empty() && !strict {
        return Ok(());
    }
    let mut problems: Vec<String> = gaps.iter().map(ToString::to_string).collect();
    if strict {
        problems.push(
            "script-src has 'strict-dynamic', so the browser blocks the loader; \
             remove 'strict-dynamic', or set csp_check = \"off\" and load the pixel yourself"
                .to_owned(),
        );
    }
    let mut message = format!(
        "the Content-Security-Policy blocks the pixel: {}.",
        problems.join("; ")
    );
    if !gaps.is_empty() {
        let fixed = toml::Value::String(with_meta_pixel_sources(csp, &origin));
        // Writing to a `String` cannot fail.
        let _ = write!(
            message,
            " Set this in autumn.toml:\n[security.headers]\ncontent_security_policy = {fixed}"
        );
        if headers.csp_nonce.enabled {
            message.push_str(
                "\nNote: an explicit content_security_policy turns off the automatic nonce.",
            );
        }
    }
    match config.csp_check {
        CspCheck::Warn => {
            tracing::warn!("meta_pixel: {message}");
            Ok(())
        }
        _ => Err(MetaPixelError::Csp(message)),
    }
}

impl Plugin for MetaPixelPlugin {
    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed(PLUGIN_NAME)
    }

    fn contract(&self) -> Option<PluginContract> {
        Some(
            PluginContract::new(env!("CARGO_PKG_NAME"))
                .plugin_version(env!("CARGO_PKG_VERSION"))
                .autumn_web("0.8"),
        )
    }

    fn build(self, app: AppBuilder) -> AppBuilder {
        app.config_section(SECTION)
            .plugin_assets(&ASSETS)
            .on_startup(move |state| {
                let plugin = self.clone();
                async move {
                    plugin
                        .start(&state)
                        .map_err(AutumnError::internal_server_error)
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CspCheck;

    fn cfg() -> MetaPixelConfig {
        MetaPixelConfig::with_pixel_ids(["111"])
    }

    #[test]
    fn default_csp_fails_with_the_fix() {
        let err = check_csp(&cfg(), &AutumnConfig::default())
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("script-src does not allow https://connect.facebook.net"),
            "{err}"
        );
        assert!(err.contains("[security.headers]"), "{err}");
        assert!(err.contains("content_security_policy = \""), "{err}");
        assert!(err.contains("https://www.facebook.com"), "{err}");
    }

    #[test]
    fn fixed_csp_passes() {
        let mut app = AutumnConfig::default();
        app.security.headers.content_security_policy = crate::csp::with_meta_pixel_sources(
            &app.security.headers.content_security_policy,
            "https://connect.facebook.net",
        );
        check_csp(&cfg(), &app).unwrap();
        app.security.headers.content_security_policy = String::new();
        check_csp(&cfg(), &app).unwrap();
    }

    #[test]
    fn warn_and_off_do_not_fail() {
        let mut c = cfg();
        c.csp_check = CspCheck::Warn;
        check_csp(&c, &AutumnConfig::default()).unwrap();
        c.csp_check = CspCheck::Off;
        check_csp(&c, &AutumnConfig::default()).unwrap();
    }

    #[test]
    fn nonce_note_and_strict_dynamic() {
        let mut app = AutumnConfig::default();
        app.security.headers.csp_nonce.enabled = true;
        let err = check_csp(&cfg(), &app).unwrap_err().to_string();
        assert!(err.contains("nonce"), "{err}");
        app.security.headers.content_security_policy =
            "script-src 'self' 'strict-dynamic' https://connect.facebook.net; img-src *; connect-src *".into();
        let err = check_csp(&cfg(), &app).unwrap_err().to_string();
        assert!(err.contains("strict-dynamic"), "{err}");
    }

    #[test]
    fn contract_names_autumn_08() {
        let c = MetaPixelPlugin::new().contract().unwrap();
        let text = format!("{c:?}");
        assert!(text.contains("autumn-plugin-meta-pixel"), "{text}");
        assert!(text.contains("0.8"), "{text}");
        assert_eq!(MetaPixelPlugin::new().name(), PLUGIN_NAME);
    }
}
