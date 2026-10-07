//! Content-Security-Policy check and fix for the pixel hosts.
//!
//! The pixel needs:
//!
//! | Directive | Source | Why |
//! |---|---|---|
//! | `script-src` | `'self'` | The loader (a plugin asset). |
//! | `script-src` | origin of `script_url` | `fbevents.js` and its config scripts. |
//! | `img-src` | `https://www.facebook.com` | Hits and the `noscript` image. |
//! | `connect-src` | `https://www.facebook.com` | Hits sent with `fetch` or a beacon. |
//!
//! The check is a subset of CSP Level 3. It never reports a pass for a
//! policy that blocks the pixel. It can report a gap for a policy that is
//! wider than it can parse; the fix is then harmless.

/// Origin of the pixel hits.
pub const HIT_ORIGIN: &str = "https://www.facebook.com";

/// One source that the policy does not allow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CspGap {
    /// The directive that needs the source, for example `script-src`.
    pub directive: &'static str,
    /// The source to add, for example `https://connect.facebook.net`.
    pub source: String,
}

impl std::fmt::Display for CspGap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} does not allow {}", self.directive, self.source)
    }
}

/// The `scheme://host[:port]` part of an `https` URL, lower case.
#[must_use]
pub fn origin_of(url: &str) -> Option<String> {
    let lower = url.trim().to_ascii_lowercase();
    let rest = lower.strip_prefix("https://")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let (host, port) = match authority.split_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (authority, None),
    };
    let host_ok = !host.is_empty()
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    let port_ok = port.is_none_or(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    (host_ok && port_ok).then(|| format!("https://{authority}"))
}

/// One directive: its name as written, and its source tokens.
struct Directive<'a> {
    name: &'a str,
    tokens: Vec<&'a str>,
    text: &'a str,
}

fn parse(csp: &str) -> Vec<Directive<'_>> {
    csp.split(';')
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(|text| {
            let mut parts = text.split_ascii_whitespace();
            let name = parts.next().unwrap_or_default();
            Directive {
                name,
                tokens: parts.collect(),
                text,
            }
        })
        .collect()
}

/// The directive that applies: the first present name in `order`. The
/// browser uses the first of two directives with the same name.
fn effective(dirs: &[Directive<'_>], order: &[&str]) -> Option<usize> {
    order
        .iter()
        .find_map(|want| dirs.iter().position(|d| d.name.eq_ignore_ascii_case(want)))
}

const SCRIPT: [&str; 3] = ["script-src-elem", "script-src", "default-src"];
const IMG: [&str; 2] = ["img-src", "default-src"];
const CONNECT: [&str; 2] = ["connect-src", "default-src"];

/// The needs of the pixel: lookup order, directive to fix, source.
fn needs(script_origin: &str) -> [(&'static [&'static str], &'static str, String); 4] {
    [
        (&SCRIPT, "script-src", "'self'".to_owned()),
        (&SCRIPT, "script-src", script_origin.to_ascii_lowercase()),
        (&IMG, "img-src", HIT_ORIGIN.to_owned()),
        (&CONNECT, "connect-src", HIT_ORIGIN.to_owned()),
    ]
}

/// `true` when one of `tokens` allows `source` (`'self'` or an origin).
fn allows(tokens: &[&str], source: &str) -> bool {
    tokens.iter().any(|t| {
        let t = t.to_ascii_lowercase();
        if t == "*" {
            return true;
        }
        if source == "'self'" {
            return t == "'self'";
        }
        t == "https:" || host_source_matches(&t, source)
    })
}

/// Host-source match for an `https://host[:port]` origin. A source with a
/// path other than `/`, or with a scheme other than `https`, does not match.
fn host_source_matches(token: &str, origin: &str) -> bool {
    let Some(target) = origin.strip_prefix("https://") else {
        return false;
    };
    let (target_host, target_port) = target.split_once(':').unwrap_or((target, "443"));
    if token.starts_with('\'') {
        return false;
    }
    let rest = match token.split_once("://") {
        Some(("https", rest)) => rest,
        Some(_) => return false,
        None if token.ends_with(':') => return false,
        None => token,
    };
    let (authority, path) = rest.find('/').map_or((rest, ""), |i| rest.split_at(i));
    if !(path.is_empty() || path == "/") {
        return false;
    }
    let (host, port) = authority.split_once(':').unwrap_or((authority, "443"));
    let host_ok = host
        .strip_prefix("*.")
        .map_or(host == target_host, |suffix| {
            target_host
                .strip_suffix(suffix)
                .is_some_and(|head| head.ends_with('.') && head.len() > 1)
        });
    host_ok && (port == "*" || port == target_port)
}

/// The sources that `csp` does not allow. Empty means the pixel works.
///
/// `script_origin` is the origin of `script_url`. An empty `csp` sends no
/// header, so it allows all.
#[must_use]
pub fn missing_sources(csp: &str, script_origin: &str) -> Vec<CspGap> {
    let dirs = parse(csp);
    needs(script_origin)
        .into_iter()
        .filter_map(|(order, directive, source)| {
            let i = effective(&dirs, order)?;
            if allows(&dirs[i].tokens, &source) {
                return None;
            }
            let directive = if dirs[i].name.eq_ignore_ascii_case("script-src-elem") {
                "script-src-elem"
            } else {
                directive
            };
            Some(CspGap { directive, source })
        })
        .collect()
}

/// `true` when the effective `script-src` has `'strict-dynamic'`.
///
/// Then the browser ignores `'self'` and host sources. The loader needs a
/// nonce, which this plugin does not give. [`with_meta_pixel_sources`]
/// cannot fix that.
#[must_use]
pub fn has_strict_dynamic(csp: &str) -> bool {
    let dirs = parse(csp);
    effective(&dirs, &SCRIPT).is_some_and(|i| {
        dirs[i]
            .tokens
            .iter()
            .any(|t| t.eq_ignore_ascii_case("'strict-dynamic'"))
    })
}

/// `csp` with the missing sources added.
///
/// ```
/// use autumn_plugin_meta_pixel::csp::{missing_sources, with_meta_pixel_sources};
///
/// let origin = "https://connect.facebook.net";
/// let fixed = with_meta_pixel_sources("default-src 'self'", origin);
/// assert!(missing_sources(&fixed, origin).is_empty());
/// assert!(fixed.starts_with("default-src 'self'; script-src 'self' https://connect.facebook.net"));
/// ```
///
/// A source goes into the directive that applies. When only `default-src`
/// applies, the fix adds the specific directive with the `default-src`
/// sources plus the new source, so other resource types do not change.
/// `'none'` goes away from a directive that gets a source.
#[must_use]
pub fn with_meta_pixel_sources(csp: &str, script_origin: &str) -> String {
    if missing_sources(csp, script_origin).is_empty() {
        return csp.to_owned();
    }
    let dirs = parse(csp);
    // Tokens per existing directive, set when the fix changes it.
    let mut changed: Vec<Option<Vec<String>>> = vec![None; dirs.len()];
    // New directives, in the order the fix adds them.
    let mut added: Vec<(&'static str, Vec<String>)> = Vec::new();
    let keep = |tokens: &[&str]| -> Vec<String> {
        tokens
            .iter()
            .filter(|t| !t.eq_ignore_ascii_case("'none'"))
            .map(|t| (*t).to_owned())
            .collect()
    };
    for (order, directive, source) in needs(script_origin) {
        let Some(i) = effective(&dirs, order) else {
            continue;
        };
        if allows(&dirs[i].tokens, &source) {
            continue;
        }
        if dirs[i].name.eq_ignore_ascii_case("default-src") {
            if let Some((_, tokens)) = added.iter_mut().find(|(n, _)| *n == directive) {
                tokens.push(source);
            } else {
                let mut tokens = keep(&dirs[i].tokens);
                tokens.push(source);
                added.push((directive, tokens));
            }
        } else {
            changed[i]
                .get_or_insert_with(|| keep(&dirs[i].tokens))
                .push(source);
        }
    }
    let mut out: Vec<String> = dirs
        .iter()
        .zip(changed)
        .map(|(d, tokens)| {
            tokens.map_or_else(
                || d.text.to_owned(),
                |tokens| format!("{} {}", d.name, tokens.join(" ")),
            )
        })
        .collect();
    out.extend(
        added
            .into_iter()
            .map(|(name, tokens)| format!("{name} {}", tokens.join(" "))),
    );
    out.join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const FB: &str = "https://connect.facebook.net";
    const AUTUMN_DEFAULT: &str = "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; script-src 'self'; connect-src 'self'; form-action 'self'; frame-ancestors 'none'; base-uri 'self'";

    fn gaps(csp: &str) -> Vec<String> {
        missing_sources(csp, FB)
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn origin_parse() {
        assert_eq!(
            origin_of("https://connect.facebook.net/en_US/fbevents.js").as_deref(),
            Some(FB)
        );
        assert_eq!(
            origin_of("HTTPS://CDN.Example.com:8443/x.js").as_deref(),
            Some("https://cdn.example.com:8443")
        );
        assert_eq!(origin_of("https://a.b").as_deref(), Some("https://a.b"));
        assert_eq!(origin_of("http://connect.facebook.net/x.js"), None);
        assert_eq!(origin_of("https:///x.js"), None);
        assert_eq!(origin_of("https://a b/x.js"), None);
        assert_eq!(origin_of("https://user@host/x.js"), None);
        assert_eq!(origin_of("//connect.facebook.net/x.js"), None);
    }

    #[test]
    fn empty_policy_allows_all() {
        assert!(gaps("").is_empty());
        assert!(gaps("   ").is_empty());
        // No fetch directive at all.
        assert!(gaps("frame-ancestors 'none'").is_empty());
    }

    #[test]
    fn autumn_default_policy_has_three_gaps() {
        assert_eq!(
            gaps(AUTUMN_DEFAULT),
            [
                "script-src does not allow https://connect.facebook.net",
                "img-src does not allow https://www.facebook.com",
                "connect-src does not allow https://www.facebook.com",
            ]
        );
    }

    #[test]
    fn fixed_policy_passes_and_keeps_other_directives() {
        let fixed = with_meta_pixel_sources(AUTUMN_DEFAULT, FB);
        assert!(gaps(&fixed).is_empty(), "{fixed}");
        assert!(fixed.contains("script-src 'self' https://connect.facebook.net"));
        assert!(fixed.contains("img-src 'self' data: https://www.facebook.com"));
        assert!(fixed.contains("connect-src 'self' https://www.facebook.com"));
        assert!(fixed.contains("frame-ancestors 'none'"));
        assert!(fixed.contains("style-src 'self' 'unsafe-inline'"));
    }

    #[test]
    fn default_src_fallback_is_copied_into_a_new_directive() {
        let csp = "default-src 'self' https://cdn.example.com";
        assert_eq!(gaps(csp).len(), 3);
        let fixed = with_meta_pixel_sources(csp, FB);
        assert!(gaps(&fixed).is_empty(), "{fixed}");
        assert!(fixed.starts_with("default-src 'self' https://cdn.example.com"));
        assert!(
            fixed
                .contains("script-src 'self' https://cdn.example.com https://connect.facebook.net")
        );
    }

    #[test]
    fn script_src_elem_wins_over_script_src() {
        let csp =
            format!("script-src 'self' {FB}; script-src-elem 'self'; img-src *; connect-src *");
        assert_eq!(
            gaps(&csp),
            ["script-src-elem does not allow https://connect.facebook.net"]
        );
    }

    #[test]
    fn loader_needs_self() {
        let csp = format!("script-src {FB}; img-src *; connect-src *");
        assert_eq!(gaps(&csp), ["script-src does not allow 'self'"]);
        let fixed = with_meta_pixel_sources(&csp, FB);
        assert!(gaps(&fixed).is_empty(), "{fixed}");
    }

    #[test]
    fn wide_sources_match() {
        assert!(gaps("default-src *").is_empty());
        assert!(gaps("default-src 'self' https:").is_empty());
        assert!(gaps("default-src 'self' *.facebook.net *.facebook.com").is_empty());
        assert!(
            gaps("default-src 'self' https://*.facebook.net https://*.facebook.com").is_empty()
        );
        assert!(gaps("default-src 'self' connect.facebook.net www.facebook.com").is_empty());
        assert!(
            gaps("default-src 'self' https://connect.facebook.net:443 https://www.facebook.com:*")
                .is_empty()
        );
        assert!(
            gaps("DEFAULT-SRC 'SELF' HTTPS://CONNECT.FACEBOOK.NET HTTPS://WWW.FACEBOOK.COM")
                .is_empty()
        );
    }

    #[test]
    fn narrow_or_wrong_sources_do_not_match() {
        // Path-limited sources: the pixel loads more than one path.
        assert_eq!(
            gaps("default-src 'self' https://connect.facebook.net/en_US/ https://www.facebook.com")
                .len(),
            1
        );
        // `http:` scheme, wrong port, a look-alike host.
        assert_eq!(
            gaps("default-src 'self' http://connect.facebook.net https://www.facebook.com").len(),
            1
        );
        assert_eq!(
            gaps("default-src 'self' https://connect.facebook.net:8443 https://www.facebook.com")
                .len(),
            1
        );
        assert_eq!(
            gaps("default-src 'self' https://evilfacebook.net https://www.facebook.com").len(),
            1
        );
        // `*.facebook.net` does not match `facebook.net` itself.
        assert_eq!(
            missing_sources(
                "default-src 'self' *.facebook.net *.facebook.com",
                "https://facebook.net"
            )
            .len(),
            1
        );
    }

    #[test]
    fn none_is_replaced_by_the_fix() {
        let csp = "default-src 'none'";
        assert_eq!(gaps(csp).len(), 4);
        let fixed = with_meta_pixel_sources(csp, FB);
        assert!(gaps(&fixed).is_empty(), "{fixed}");
        assert!(!fixed.contains("script-src 'none'"), "{fixed}");
        assert!(fixed.starts_with("default-src 'none'"));
    }

    #[test]
    fn first_duplicate_directive_wins() {
        let csp = format!("script-src 'self'; script-src 'self' {FB}; img-src *; connect-src *");
        assert_eq!(gaps(&csp).len(), 1);
    }

    #[test]
    fn strict_dynamic_is_reported() {
        assert!(has_strict_dynamic("script-src 'nonce-x' 'strict-dynamic'"));
        assert!(has_strict_dynamic("default-src 'self' 'STRICT-DYNAMIC'"));
        assert!(!has_strict_dynamic(
            "script-src 'self'; default-src 'strict-dynamic'"
        ));
        assert!(!has_strict_dynamic(AUTUMN_DEFAULT));
    }

    fn token() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("'self'".to_owned()),
            Just("'none'".to_owned()),
            Just("'unsafe-inline'".to_owned()),
            Just("data:".to_owned()),
            Just("https:".to_owned()),
            Just("*".to_owned()),
            Just("https://cdn.example.com".to_owned()),
            Just("https://connect.facebook.net/x/".to_owned()),
            Just("*.facebook.com".to_owned()),
            "[a-z]{1,8}\\.(com|net)".prop_map(|h| h),
        ]
    }

    fn directive() -> impl Strategy<Value = String> {
        (
            prop_oneof![
                Just("default-src"),
                Just("script-src"),
                Just("script-src-elem"),
                Just("img-src"),
                Just("connect-src"),
                Just("style-src"),
                Just("frame-ancestors"),
            ],
            prop::collection::vec(token(), 0..4),
        )
            .prop_map(|(name, tokens)| format!("{name} {}", tokens.join(" ")))
    }

    proptest! {
        /// The fix always closes all gaps.
        #[test]
        fn fix_closes_all_gaps(dirs in prop::collection::vec(directive(), 0..6)) {
            let csp = dirs.join("; ");
            let fixed = with_meta_pixel_sources(&csp, FB);
            prop_assert!(missing_sources(&fixed, FB).is_empty(), "{} -> {}", csp, fixed);
        }

        /// The fix does not change a policy with no gaps.
        #[test]
        fn fix_is_identity_without_gaps(dirs in prop::collection::vec(directive(), 0..6)) {
            let csp = dirs.join("; ");
            if missing_sources(&csp, FB).is_empty() {
                prop_assert_eq!(with_meta_pixel_sources(&csp, FB), csp);
            }
        }

        /// The fix keeps each directive that needs no change.
        #[test]
        fn fix_keeps_style_and_frame(dirs in prop::collection::vec(directive(), 0..6)) {
            let csp = dirs.join("; ");
            let fixed = with_meta_pixel_sources(&csp, FB);
            for d in csp.split(';').map(str::trim) {
                if d.starts_with("style-src") || d.starts_with("frame-ancestors") {
                    prop_assert!(fixed.contains(d), "{} lost {}", fixed, d);
                }
            }
        }
    }
}
