//! Verified core. Pure functions with no I/O.
//!
//! `verus/policy.rs` holds the spec and the proofs. Keep both files in step.

/// Longest pixel ID.
pub const MAX_PIXEL_ID_LEN: usize = 32;
/// Longest custom event name. Meta limit.
pub const MAX_EVENT_NAME_LEN: usize = 50;

/// Escapes JSON text for a `<script type="application/json">` block.
///
/// Replaces `<`, `>`, and `&` with `\u003c`, `\u003e`, and `\u0026`. Other
/// characters stay. These bytes occur in JSON only inside strings, so the
/// JSON value does not change. The output has no `<`, so it cannot close
/// the block.
#[must_use]
pub fn escape_json_for_html(json: &str) -> String {
    // Per char, not per byte: `<`, `>`, `&` are ASCII, and a UTF-8
    // multi-byte sequence has no ASCII byte. So this function agrees
    // with `spec_escape` in `verus/policy.rs`.
    let mut out = String::with_capacity(json.len());
    for c in json.chars() {
        match c {
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            _ => out.push(c),
        }
    }
    out
}

/// Returns `true` when `id` is 1 to 32 ASCII digits.
#[must_use]
pub const fn is_valid_pixel_id(id: &str) -> bool {
    let b = id.as_bytes();
    if b.is_empty() || b.len() > MAX_PIXEL_ID_LEN {
        return false;
    }
    let mut i = 0;
    while i < b.len() {
        if !b[i].is_ascii_digit() {
            return false;
        }
        i += 1;
    }
    true
}

/// Returns `true` when `name` is 1 to 50 bytes of `A-Z a-z 0-9 _`.
#[must_use]
pub const fn is_valid_event_name(name: &str) -> bool {
    let b = name.as_bytes();
    if b.is_empty() || b.len() > MAX_EVENT_NAME_LEN {
        return false;
    }
    let mut i = 0;
    while i < b.len() {
        if !(b[i].is_ascii_alphanumeric() || b[i] == b'_') {
            return false;
        }
        i += 1;
    }
    true
}

/// Inputs to the load decision for one request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(clippy::struct_excessive_bools)] // Each field is one independent input.
pub(crate) struct Gate {
    /// `enabled` in config.
    pub enabled: bool,
    /// One or more pixel IDs are set.
    pub has_pixels: bool,
    /// `require_consent` in config.
    pub require_consent: bool,
    /// `Consent::allows(category, version)` for this request.
    pub consent_granted: bool,
    /// `honor_gpc` in config.
    pub honor_gpc: bool,
    /// The request sends `Sec-GPC: 1`.
    pub gpc_signal: bool,
}

/// Returns `true` when the page gets the pixel.
#[must_use]
pub(crate) const fn active(gate: Gate) -> bool {
    gate.enabled
        && gate.has_pixels
        && (!gate.require_consent || gate.consent_granted)
        && !(gate.honor_gpc && gate.gpc_signal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn escape_replaces_html_specials() {
        assert_eq!(
            escape_json_for_html(r#"{"a":"</script><!--&"}"#),
            r#"{"a":"\u003c/script\u003e\u003c!--\u0026"}"#
        );
    }

    #[test]
    fn escape_keeps_json_value() {
        let raw = serde_json::json!({"t": "<b>&amp;</b> ü \u{2028}"}).to_string();
        let escaped = escape_json_for_html(&raw);
        let back: serde_json::Value = serde_json::from_str(&escaped).unwrap();
        assert_eq!(back["t"], "<b>&amp;</b> ü \u{2028}");
    }

    #[test]
    fn pixel_id_rules() {
        assert!(is_valid_pixel_id("1234567890123456"));
        assert!(is_valid_pixel_id("1"));
        assert!(is_valid_pixel_id(&"9".repeat(MAX_PIXEL_ID_LEN)));
        assert!(!is_valid_pixel_id(""));
        assert!(!is_valid_pixel_id(&"9".repeat(MAX_PIXEL_ID_LEN + 1)));
        assert!(!is_valid_pixel_id("12a4"));
        assert!(!is_valid_pixel_id(" 123"));
        assert!(!is_valid_pixel_id("１２３")); // Full-width digits.
    }

    #[test]
    fn event_name_rules() {
        assert!(is_valid_event_name("ShareClick"));
        assert!(is_valid_event_name("plan_upgrade_2"));
        assert!(is_valid_event_name(&"a".repeat(MAX_EVENT_NAME_LEN)));
        assert!(!is_valid_event_name(""));
        assert!(!is_valid_event_name(&"a".repeat(MAX_EVENT_NAME_LEN + 1)));
        assert!(!is_valid_event_name("has space"));
        assert!(!is_valid_event_name("dash-name"));
        assert!(!is_valid_event_name("</script>"));
        assert!(!is_valid_event_name("é"));
    }

    fn on() -> Gate {
        Gate {
            enabled: true,
            has_pixels: true,
            require_consent: true,
            consent_granted: true,
            honor_gpc: true,
            gpc_signal: false,
        }
    }

    #[test]
    fn gate_cases() {
        assert!(active(on()));
        assert!(!active(Gate {
            enabled: false,
            ..on()
        }));
        assert!(!active(Gate {
            has_pixels: false,
            ..on()
        }));
        assert!(!active(Gate {
            consent_granted: false,
            ..on()
        }));
        assert!(active(Gate {
            require_consent: false,
            consent_granted: false,
            ..on()
        }));
        assert!(!active(Gate {
            gpc_signal: true,
            ..on()
        }));
        assert!(active(Gate {
            honor_gpc: false,
            gpc_signal: true,
            ..on()
        }));
    }

    fn any_gate() -> impl Strategy<Value = Gate> {
        any::<[bool; 6]>().prop_map(|b| Gate {
            enabled: b[0],
            has_pixels: b[1],
            require_consent: b[2],
            consent_granted: b[3],
            honor_gpc: b[4],
            gpc_signal: b[5],
        })
    }

    proptest! {
        /// Same as `lemma_escape_safe`.
        #[test]
        fn escape_output_is_html_safe(s in any::<String>()) {
            let out = escape_json_for_html(&s);
            prop_assert!(!out.contains(['<', '>', '&']));
        }

        /// Same as `lemma_escape_identity`.
        #[test]
        fn escape_is_identity_on_safe_text(s in "[^<>&]*") {
            prop_assert_eq!(escape_json_for_html(&s), s);
        }

        /// Any JSON string value round-trips through the escape.
        #[test]
        fn escape_round_trips_json(s in any::<String>()) {
            let raw = serde_json::Value::String(s.clone()).to_string();
            let back: serde_json::Value = serde_json::from_str(&escape_json_for_html(&raw)).unwrap();
            prop_assert_eq!(back, serde_json::Value::String(s));
        }

        /// Same as `spec_pixel_id`.
        #[test]
        fn pixel_id_matches_spec(s in ".{0,40}") {
            let spec = (1..=MAX_PIXEL_ID_LEN).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit());
            prop_assert_eq!(is_valid_pixel_id(&s), spec);
        }

        /// Same as `spec_event_name`.
        #[test]
        fn event_name_matches_spec(s in "[A-Za-z0-9_ <>-]{0,60}") {
            let spec = (1..=MAX_EVENT_NAME_LEN).contains(&s.len())
                && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
            prop_assert_eq!(is_valid_event_name(&s), spec);
        }

        /// Same as the gate lemmas.
        #[test]
        fn gate_invariants(g in any_gate()) {
            let a = active(g);
            if a {
                prop_assert!(g.enabled && g.has_pixels);
            }
            if g.require_consent && !g.consent_granted {
                prop_assert!(!a);
            }
            if g.honor_gpc && g.gpc_signal {
                prop_assert!(!a);
            }
            if a {
                let more = Gate { consent_granted: true, ..g };
                prop_assert!(active(more));
            }
        }
    }
}
