//! `[meta_pixel]` configuration.
//!
//! Sources, in order (later wins):
//! 1. `[meta_pixel]` in `autumn.toml`.
//! 2. `[profile.<name>.meta_pixel]` in `autumn.toml`.
//! 3. `[meta_pixel]` in the profile file, for example `autumn-prod.toml`.
//! 4. `.env` values, then the process environment:
//!    `AUTUMN_META_PIXEL__<KEY>`, for example
//!    `AUTUMN_META_PIXEL__PIXEL_IDS=123,456`. A list value is a
//!    comma-separated string.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::csp::origin_of;
use crate::error::MetaPixelError;
use crate::policy::{MAX_PIXEL_ID_LEN, is_valid_pixel_id};

/// Name of the TOML section.
pub const SECTION: &str = "meta_pixel";
/// Prefix for environment overrides.
pub const ENV_PREFIX: &str = "AUTUMN_META_PIXEL__";
/// Default `fbevents.js` URL.
pub const DEFAULT_SCRIPT_URL: &str = "https://connect.facebook.net/en_US/fbevents.js";

/// What to do when the CSP blocks the pixel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum CspCheck {
    /// Fail the app start. The error gives the fixed CSP.
    #[default]
    Error,
    /// Log a warning with the fixed CSP.
    Warn,
    /// Do not check.
    Off,
}

/// Plugin configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
#[allow(clippy::struct_excessive_bools)] // Each flag is one config key.
pub struct MetaPixelConfig {
    /// Off switch. Use `false` in dev and test profiles.
    pub enabled: bool,
    /// Pixel IDs. Each is 1 to 32 ASCII digits.
    pub pixel_ids: Vec<String>,
    /// Send `PageView` when the page loads.
    pub page_view: bool,
    /// Let the pixel send `PageView` on `pushState` (htmx `hx-push-url`).
    /// `false` sets `fbq.disablePushState`.
    pub history_page_views: bool,
    /// Let the pixel collect button clicks and page metadata. `false` sends
    /// `fbq('set', 'autoConfig', false, id)`.
    pub auto_config: bool,
    /// Render a `<noscript>` `PageView` image.
    pub noscript: bool,
    /// Render nothing until the visitor consents to `consent_category`.
    pub require_consent: bool,
    /// Consent category for the pixel. Not `necessary`.
    pub consent_category: String,
    /// The app's cookie policy version, as given to `Consent::allows`.
    pub consent_policy_version: u32,
    /// Turn the pixel off for a Global Privacy Control request.
    pub honor_gpc: bool,
    /// `fbevents.js` URL. HTTPS only.
    pub script_url: String,
    /// What to do when the CSP blocks the pixel.
    pub csp_check: CspCheck,
}

impl Default for MetaPixelConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            pixel_ids: Vec::new(),
            page_view: true,
            history_page_views: true,
            auto_config: true,
            noscript: true,
            require_consent: true,
            consent_category: "marketing".to_owned(),
            consent_policy_version: 1,
            honor_gpc: true,
            script_url: DEFAULT_SCRIPT_URL.to_owned(),
            csp_check: CspCheck::Error,
        }
    }
}

impl MetaPixelConfig {
    /// Default config with these pixel IDs.
    #[must_use]
    pub fn with_pixel_ids<I, S>(ids: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            pixel_ids: ids.into_iter().map(Into::into).collect(),
            ..Self::default()
        }
    }

    /// The origin of `script_url`, for example `https://connect.facebook.net`.
    #[must_use]
    pub fn script_origin(&self) -> Option<String> {
        origin_of(&self.script_url)
    }

    /// Reads `[meta_pixel]` from the text of an `autumn.toml` file.
    ///
    /// # Errors
    /// Returns [`MetaPixelError::Config`] for bad TOML, unknown keys, or
    /// failed validation.
    pub fn from_toml_str(autumn_toml: &str) -> Result<Self, MetaPixelError> {
        Self::from_layers(Some(autumn_toml), &[], None, std::iter::empty())
    }

    /// Merges, in order: `[meta_pixel]` in `base`, `[profile.<name>.meta_pixel]`
    /// in `base` for each of `profile_names`, `[meta_pixel]` in
    /// `profile_file`, and env vars. Then validates.
    fn from_layers(
        base: Option<&str>,
        profile_names: &[String],
        profile_file: Option<&str>,
        env: impl IntoIterator<Item = (String, String)>,
    ) -> Result<Self, MetaPixelError> {
        let parse = |text: &str| -> Result<toml::Table, MetaPixelError> {
            toml::from_str(text).map_err(|e| MetaPixelError::Config(format!("toml: {e}")))
        };
        let mut table = toml::Table::new();
        if let Some(text) = base {
            let file = parse(text)?;
            if let Some(toml::Value::Table(section)) = file.get(SECTION) {
                merge(&mut table, section.clone());
            }
            for name in inline_profile_order(profile_names) {
                if let Some(toml::Value::Table(section)) = file
                    .get("profile")
                    .and_then(|p| p.get(&name))
                    .and_then(|p| p.get(SECTION))
                {
                    merge(&mut table, section.clone());
                }
            }
        }
        if let Some(text) = profile_file
            && let Some(toml::Value::Table(section)) = parse(text)?.get(SECTION)
        {
            merge(&mut table, section.clone());
        }
        let schema = toml::Table::try_from(Self::default())
            .map_err(|e| MetaPixelError::Config(e.to_string()))?;
        for (key, value) in env {
            let Some(path) = key.strip_prefix(ENV_PREFIX) else {
                continue;
            };
            let path: Vec<String> = path.split("__").map(str::to_ascii_lowercase).collect();
            // A bad typed value: the error names the variable, not the value.
            let typed = typed_env_value(&schema, &path, &value)
                .ok_or_else(|| MetaPixelError::Config(format!("{key}: value is not valid")))?;
            insert_path(&mut table, &path, typed);
        }
        let cfg: Self = toml::Value::Table(table)
            .try_into()
            .map_err(|e| MetaPixelError::Config(e.to_string()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Loads config from files in `manifest_dir` (else the current
    /// directory) and from `env`. `profile_names` selects
    /// `[profile.<name>]` and the first `autumn-<name>.toml` found.
    ///
    /// # Errors
    /// Returns [`MetaPixelError::Config`] for a file that cannot be read or
    /// parsed, or for failed validation.
    pub fn load_from_dir(
        manifest_dir: &Path,
        profile_names: &[String],
        env: impl IntoIterator<Item = (String, String)>,
    ) -> Result<Self, MetaPixelError> {
        let find = |file: &str| {
            let candidate = manifest_dir.join(file);
            if candidate.exists() {
                candidate
            } else {
                PathBuf::from(file)
            }
        };
        let base = read_optional(&find("autumn.toml"))?;
        let mut profile = None;
        for name in profile_names {
            if let Some(text) = read_optional(&find(&format!("autumn-{name}.toml")))? {
                profile = Some(text);
                break;
            }
        }
        Self::from_layers(base.as_deref(), profile_names, profile.as_deref(), env)
    }

    /// Loads config like autumn-web does: `$AUTUMN_MANIFEST_DIR` (else `.`),
    /// the app profile, `.env`, and the process environment.
    ///
    /// `profile` is the app profile. `None` reads no profile layers.
    ///
    /// # Errors
    /// Returns [`MetaPixelError::Config`] when loading or validation fails.
    pub fn load(profile: Option<&str>) -> Result<Self, MetaPixelError> {
        let dotenv = autumn_web::dotenv::resolve_process_dotenv()
            .map_err(|e| MetaPixelError::Config(format!(".env: {e}")))?;
        let os: Vec<(String, String)> = std::env::vars_os()
            .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
            .collect();
        let args: Vec<String> = std::env::args_os()
            .filter_map(|a| a.into_string().ok())
            .collect();
        // autumn's `OsEnv` also gives the crate dir that
        // `#[autumn_web::main]` records at compile time.
        let manifest_dir = {
            use autumn_web::config::Env as _;
            autumn_web::config::OsEnv
                .var("AUTUMN_MANIFEST_DIR")
                .ok()
                .map(PathBuf::from)
        };
        Self::load_from(profile, &os, dotenv, &args, manifest_dir)
    }

    /// [`load`](Self::load) with the process environment (`os`), the
    /// `.env` values, the process args, and the manifest dir as inputs.
    ///
    /// The profile selector (`AUTUMN_ENV`, `AUTUMN_PROFILE`, `--profile`)
    /// comes from `os` only, like autumn-web. The overlay is `.env`, then
    /// `os` (later wins). No `manifest_dir` means the current directory.
    fn load_from(
        profile: Option<&str>,
        os: &[(String, String)],
        dotenv: Vec<(String, String)>,
        args: &[String],
        manifest_dir: Option<PathBuf>,
    ) -> Result<Self, MetaPixelError> {
        let var = |key: &str| {
            os.iter()
                .rev()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
        };
        let names = profile
            .map(|p| {
                // Same selector order as autumn-web: env vars, then `--profile`.
                let selector = ["AUTUMN_ENV", "AUTUMN_PROFILE"]
                    .iter()
                    .find_map(|k| var(k).filter(|v| !v.trim().is_empty()))
                    .or_else(|| profile_flag(args))
                    .map_or_else(|| p.to_owned(), |v| v.trim().to_owned());
                autumn_web::config::profile_override_file_lookup_names(p, &selector)
            })
            .unwrap_or_default();
        let dir = manifest_dir.unwrap_or_else(|| PathBuf::from("."));
        let mut env = dotenv;
        env.extend(os.iter().cloned());
        Self::load_from_dir(&dir, &names, env)
    }

    /// Checks the values.
    ///
    /// # Errors
    /// Returns [`MetaPixelError::Config`] with the first problem found.
    pub fn validate(&self) -> Result<(), MetaPixelError> {
        let bad = |m: String| Err(MetaPixelError::Config(m));
        for (i, id) in self.pixel_ids.iter().enumerate() {
            if !is_valid_pixel_id(id) {
                return bad(format!(
                    "pixel_ids: {id:?} is not valid; use 1 to {MAX_PIXEL_ID_LEN} ASCII digits"
                ));
            }
            if self.pixel_ids[..i].contains(id) {
                return bad(format!("pixel_ids: {id} occurs twice"));
            }
        }
        if self.require_consent {
            let c = &self.consent_category;
            if c.is_empty()
                || !c
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            {
                return bad(format!(
                    "consent_category {c:?} is not valid; use A-Z a-z 0-9 _ -"
                ));
            }
            if c == autumn_web::consent::NECESSARY {
                return bad(
                    "consent_category must not be \"necessary\": that gate is always open"
                        .to_owned(),
                );
            }
        }
        let url = &self.script_url;
        let safe = url
            .bytes()
            .all(|b| b.is_ascii_graphic() && !matches!(b, b'"' | b'\'' | b'<' | b'>' | b'\\'));
        if !safe || origin_of(url).is_none() {
            // Do not show the value: a URL can hold a password.
            return bad("script_url must be an https URL with a host and no quotes".to_owned());
        }
        Ok(())
    }
}

/// The value of `--profile <name>` or `--profile=<name>` in `args`.
fn profile_flag(args: &[String]) -> Option<String> {
    args.iter()
        .enumerate()
        .find_map(|(i, a)| {
            a.strip_prefix("--profile=").map(str::to_owned).or_else(|| {
                (a == "--profile")
                    .then(|| args.get(i + 1).cloned())
                    .flatten()
            })
        })
        // Like autumn-web: an empty value selects nothing.
        .filter(|p| !p.trim().is_empty())
}

/// Inline `[profile.<name>]` merge order of autumn-web: the long alias first,
/// so the short name wins.
fn inline_profile_order(profile_names: &[String]) -> Vec<String> {
    let mut names = profile_names.to_vec();
    names.sort_by_key(|n| match n.as_str() {
        "production" | "development" => 0,
        _ => 1,
    });
    names
}

/// Deep merge: tables merge by key; other values replace.
fn merge(into: &mut toml::Table, from: toml::Table) {
    for (key, value) in from {
        match (into.get_mut(&key), value) {
            (Some(toml::Value::Table(dst)), toml::Value::Table(src)) => merge(dst, src),
            (_, value) => {
                into.insert(key, value);
            }
        }
    }
}

fn insert_path(table: &mut toml::Table, path: &[String], value: toml::Value) {
    let Some((last, parents)) = path.split_last() else {
        return;
    };
    let mut cur = table;
    for key in parents {
        let entry = cur
            .entry(key.clone())
            .or_insert_with(|| toml::Value::Table(toml::Table::new()));
        if !entry.is_table() {
            *entry = toml::Value::Table(toml::Table::new());
        }
        let toml::Value::Table(next) = entry else {
            return;
        };
        cur = next;
    }
    cur.insert(last.clone(), value);
}

/// Converts an env value to the type of the default value at the same
/// path. A list is a comma-separated string.
fn typed_env_value(schema: &toml::Table, path: &[String], raw: &str) -> Option<toml::Value> {
    let mut node: Option<&toml::Value> = None;
    let mut table = Some(schema);
    for key in path {
        node = table.and_then(|t| t.get(key));
        table = node.and_then(toml::Value::as_table);
    }
    match node {
        Some(toml::Value::Integer(_)) => raw.trim().parse().ok().map(toml::Value::Integer),
        // Like autumn: `true`/`1` and `false`/`0`, any case.
        Some(toml::Value::Boolean(_)) => match raw.trim().to_ascii_lowercase().as_str() {
            "true" | "1" => Some(toml::Value::Boolean(true)),
            "false" | "0" => Some(toml::Value::Boolean(false)),
            _ => None,
        },
        Some(toml::Value::Array(_)) => Some(toml::Value::Array(
            raw.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| toml::Value::String(s.to_owned()))
                .collect(),
        )),
        _ => Some(toml::Value::String(raw.to_owned())),
    }
}

fn read_optional(path: &Path) -> Result<Option<String>, MetaPixelError> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(MetaPixelError::Config(format!("{}: {e}", path.display()))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn defaults_are_valid() {
        MetaPixelConfig::default().validate().unwrap();
        assert_eq!(
            MetaPixelConfig::from_toml_str("").unwrap(),
            MetaPixelConfig::default()
        );
        assert_eq!(
            MetaPixelConfig::default().script_origin().as_deref(),
            Some("https://connect.facebook.net")
        );
    }

    #[test]
    fn reads_section_and_rejects_unknown_keys() {
        let c = MetaPixelConfig::from_toml_str(
            "[meta_pixel]\npixel_ids = [\"123\", \"456\"]\nauto_config = false\ncsp_check = \"warn\"\n",
        )
        .unwrap();
        assert_eq!(c.pixel_ids, ["123", "456"]);
        assert!(!c.auto_config);
        assert_eq!(c.csp_check, CspCheck::Warn);
        assert!(MetaPixelConfig::from_toml_str("[meta_pixel]\npixel_id = \"1\"\n").is_err());
        assert!(MetaPixelConfig::from_toml_str("[meta_pixel\n").is_err());
        assert!(MetaPixelConfig::from_toml_str("[meta_pixel]\ncsp_check = \"loud\"\n").is_err());
        // Other sections are not ours.
        MetaPixelConfig::from_toml_str("[server]\nport = 1\n").unwrap();
    }

    #[test]
    fn env_overlay_types_values() {
        let c = MetaPixelConfig::from_layers(
            Some("[meta_pixel]\npixel_ids = [\"1\"]\n"),
            &[],
            None,
            env(&[
                ("AUTUMN_META_PIXEL__PIXEL_IDS", "123, 456"),
                ("AUTUMN_META_PIXEL__ENABLED", "false"),
                ("AUTUMN_META_PIXEL__CONSENT_POLICY_VERSION", "3"),
                ("AUTUMN_META_PIXEL__CSP_CHECK", "off"),
                ("OTHER", "x"),
            ]),
        )
        .unwrap();
        assert_eq!(c.pixel_ids, ["123", "456"]);
        assert!(!c.enabled);
        assert_eq!(c.consent_policy_version, 3);
        assert_eq!(c.csp_check, CspCheck::Off);
    }

    #[test]
    fn env_bools_accept_one_and_zero_like_autumn() {
        let c = MetaPixelConfig::from_layers(
            None,
            &[],
            None,
            env(&[
                ("AUTUMN_META_PIXEL__ENABLED", "0"),
                ("AUTUMN_META_PIXEL__NOSCRIPT", " FALSE "),
                ("AUTUMN_META_PIXEL__HONOR_GPC", "1"),
            ]),
        )
        .unwrap();
        assert!(!c.enabled);
        assert!(!c.noscript);
        assert!(c.honor_gpc);
        let c = MetaPixelConfig::from_layers(
            None,
            &[],
            None,
            env(&[("AUTUMN_META_PIXEL__ENABLED", "True")]),
        )
        .unwrap();
        assert!(c.enabled);
    }

    #[test]
    fn script_url_error_does_not_echo_the_value() {
        let mut c = MetaPixelConfig::with_pixel_ids(["1"]);
        c.script_url = "https://user:hunter2@cdn.example.com/f.js".into();
        let err = c.validate().unwrap_err().to_string();
        assert!(err.contains("script_url"), "{err}");
        assert!(!err.contains("hunter2"), "{err}");
    }

    #[test]
    fn bad_env_value_names_the_variable_not_the_value() {
        let err = MetaPixelConfig::from_layers(
            None,
            &[],
            None,
            env(&[("AUTUMN_META_PIXEL__ENABLED", "s3cret-maybe")]),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("AUTUMN_META_PIXEL__ENABLED"), "{err}");
        assert!(!err.contains("s3cret-maybe"), "{err}");
    }

    #[test]
    fn profile_layers_merge_in_order() {
        let base = "[meta_pixel]\npixel_ids = [\"1\"]\nnoscript = false\n\
                    [profile.production.meta_pixel]\npixel_ids = [\"2\"]\n\
                    [profile.prod.meta_pixel]\npixel_ids = [\"3\"]\n";
        let names = ["prod".to_owned(), "production".to_owned()];
        let c = MetaPixelConfig::from_layers(Some(base), &names, None, std::iter::empty()).unwrap();
        assert_eq!(c.pixel_ids, ["3"]);
        assert!(!c.noscript);
        let c = MetaPixelConfig::from_layers(
            Some(base),
            &names,
            Some("[meta_pixel]\npixel_ids = [\"4\"]\n"),
            std::iter::empty(),
        )
        .unwrap();
        assert_eq!(c.pixel_ids, ["4"]);
        assert!(!c.noscript);
    }

    /// A temp dir that goes away also when a test panics.
    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("meta-pixel-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn write(&self, file: &str, text: &str) {
            std::fs::write(self.0.join(file), text).unwrap();
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn env_wins_over_the_profile_file() {
        let c = MetaPixelConfig::from_layers(
            Some("[meta_pixel]\npixel_ids = [\"1\"]\n"),
            &["prod".to_owned()],
            Some("[meta_pixel]\npixel_ids = [\"2\"]\nnoscript = false\n"),
            env(&[("AUTUMN_META_PIXEL__PIXEL_IDS", "3")]),
        )
        .unwrap();
        assert_eq!(c.pixel_ids, ["3"]);
        assert!(!c.noscript);
    }

    #[test]
    fn first_profile_file_found_wins() {
        let dir = TempDir::new("first");
        dir.write("autumn-prod.toml", "[meta_pixel]\npixel_ids = [\"1\"]\n");
        dir.write(
            "autumn-production.toml",
            "[meta_pixel]\npixel_ids = [\"2\"]\n",
        );
        let names = strings(&["prod", "production"]);
        let c = MetaPixelConfig::load_from_dir(&dir.0, &names, std::iter::empty()).unwrap();
        assert_eq!(c.pixel_ids, ["1"]);
    }

    #[test]
    fn load_uses_manifest_dir_selector_flag_and_dotenv() {
        let dir = TempDir::new("load");
        dir.write("autumn.toml", "[meta_pixel]\npixel_ids = [\"1\"]\n");
        dir.write("autumn-prod.toml", "[meta_pixel]\npixel_ids = [\"2\"]\n");
        dir.write(
            "autumn-production.toml",
            "[meta_pixel]\npixel_ids = [\"3\"]\n",
        );
        let path = Some(dir.0.clone());
        let os = env;
        let load = |os: &[(String, String)], args: &[String]| {
            MetaPixelConfig::load_from(Some("prod"), os, Vec::new(), args, path.clone())
                .unwrap()
                .pixel_ids
        };
        // AUTUMN_MANIFEST_DIR and the app profile: `prod` spelling first.
        assert_eq!(load(&os(&[]), &[]), ["2"]);
        // No profile: no profile layer.
        let c = MetaPixelConfig::load_from(None, &os(&[]), Vec::new(), &[], path.clone()).unwrap();
        assert_eq!(c.pixel_ids, ["1"]);
        // The selector spelling `production` (env or `--profile`) wins.
        assert_eq!(load(&os(&[("AUTUMN_ENV", "production")]), &[]), ["3"]);
        assert_eq!(load(&os(&[("AUTUMN_PROFILE", "production")]), &[]), ["3"]);
        assert_eq!(load(&os(&[("AUTUMN_ENV", " ")]), &[]), ["2"]);
        for args in [
            strings(&["app", "--profile", "production"]),
            strings(&["app", "--profile=production"]),
        ] {
            assert_eq!(load(&os(&[]), &args), ["3"]);
        }
        assert_eq!(load(&os(&[]), &strings(&["app", "--profile="])), ["2"]);
        // `.env` overlays the files; the process env wins over `.env`.
        let dotenv = env(&[
            ("AUTUMN_META_PIXEL__PIXEL_IDS", "4"),
            ("AUTUMN_META_PIXEL__NOSCRIPT", "false"),
        ]);
        let c =
            MetaPixelConfig::load_from(None, &os(&[]), dotenv.clone(), &[], path.clone()).unwrap();
        assert_eq!(c.pixel_ids, ["4"]);
        assert!(!c.noscript);
        let c = MetaPixelConfig::load_from(
            None,
            &os(&[("AUTUMN_META_PIXEL__PIXEL_IDS", "5")]),
            dotenv,
            &[],
            path.clone(),
        )
        .unwrap();
        assert_eq!(c.pixel_ids, ["5"]);
        // No manifest dir: the current directory (the crate root has no
        // autumn.toml).
        let c = MetaPixelConfig::load_from(None, &[], Vec::new(), &[], None).unwrap();
        assert!(c.pixel_ids.is_empty());
    }

    #[test]
    fn unreadable_config_file_is_an_error() {
        let dir = TempDir::new("unreadable");
        // A directory with the file name cannot be read as text.
        std::fs::create_dir_all(dir.0.join("autumn.toml")).unwrap();
        let err = MetaPixelConfig::load_from_dir(&dir.0, &[], std::iter::empty()).unwrap_err();
        assert!(err.to_string().contains("autumn.toml"), "{err}");
    }

    #[test]
    fn load_from_dir_reads_files() {
        let guard = TempDir::new("cfg");
        let dir = guard.0.clone();
        std::fs::write(
            dir.join("autumn.toml"),
            "[meta_pixel]\npixel_ids = [\"10\"]\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("autumn-dev.toml"),
            "[meta_pixel]\nenabled = false\n",
        )
        .unwrap();
        let c =
            MetaPixelConfig::load_from_dir(&dir, &["dev".to_owned()], std::iter::empty()).unwrap();
        assert_eq!(c.pixel_ids, ["10"]);
        assert!(!c.enabled);
        std::fs::write(
            dir.join("autumn.toml"),
            "[meta_pixel]\npixel_ids = [\"x\"]\n",
        )
        .unwrap();
        assert!(MetaPixelConfig::load_from_dir(&dir, &[], std::iter::empty()).is_err());
    }

    #[test]
    fn validation_rules() {
        let check = |f: fn(&mut MetaPixelConfig)| {
            let mut c = MetaPixelConfig::with_pixel_ids(["123"]);
            f(&mut c);
            c.validate().unwrap_err().to_string()
        };
        assert!(check(|c| c.pixel_ids = vec!["12a".into()]).contains("pixel_ids"));
        assert!(check(|c| c.pixel_ids = vec![String::new()]).contains("pixel_ids"));
        assert!(check(|c| c.pixel_ids = vec!["1".into(), "1".into()]).contains("twice"));
        assert!(check(|c| c.consent_category = "necessary".into()).contains("necessary"));
        assert!(check(|c| c.consent_category = String::new()).contains("consent_category"));
        assert!(check(|c| c.consent_category = "a,b".into()).contains("consent_category"));
        assert!(check(|c| c.script_url = "http://x.com/f.js".into()).contains("script_url"));
        assert!(check(|c| c.script_url = "https://x.com/f\".js".into()).contains("script_url"));
        assert!(check(|c| c.script_url = "https://x.com/a b.js".into()).contains("script_url"));
        assert!(check(|c| c.script_url = "https://x.com/a'.js".into()).contains("script_url"));
        assert!(check(|c| c.script_url = "https://x.com/a\\.js".into()).contains("script_url"));
        assert!(check(|c| c.script_url = "https://x.com/<a>.js".into()).contains("script_url"));
        // With consent off, the category is not used.
        let mut c = MetaPixelConfig::with_pixel_ids(["123"]);
        c.require_consent = false;
        c.consent_category = "necessary".into();
        c.validate().unwrap();
    }
}
