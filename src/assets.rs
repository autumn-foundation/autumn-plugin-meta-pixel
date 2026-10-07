//! The loader, served through the autumn 0.8 plugin assets seam.
//!
//! URLs: `/static/_plugins/meta-pixel/meta-pixel.<hash>.js` (`immutable`)
//! and `/static/_plugins/meta-pixel/meta-pixel.js` (`must-revalidate`).

use autumn_web::assets::PluginAssets;

/// Namespace of the bundle. It is a URL segment.
pub const NAMESPACE: &str = "meta-pixel";
/// Logical path of the loader in the bundle.
pub const LOADER: &str = "meta-pixel.js";

/// The plugin's asset bundle. [`crate::MetaPixelPlugin`] installs it.
pub static ASSETS: PluginAssets = PluginAssets::from_files(
    NAMESPACE,
    &[(LOADER, include_bytes!("../assets/meta-pixel.js"))],
);
