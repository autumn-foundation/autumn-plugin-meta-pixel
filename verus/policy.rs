//! Verus spec and proof for `src/policy.rs`.
//!
//! Keep this file in step with `src/policy.rs`.
//! Run: `verus verus/policy.rs`.

use vstd::prelude::*;

verus! {

/// `<`
pub const LT: u8 = 0x3c;
/// `>`
pub const GT: u8 = 0x3e;
/// `&`
pub const AMP: u8 = 0x26;
/// `"`
pub const QUOTE: u8 = 0x22;
/// `\`
pub const BACKSLASH: u8 = 0x5c;

/// Longest pixel ID.
pub const MAX_PIXEL_ID_LEN: usize = 32;
/// Longest custom event name. Meta limit.
pub const MAX_EVENT_NAME_LEN: usize = 50;

// ------------------------------------------------------------- HTML escape

/// Spec: a byte that can end or change a `<script>` data block.
pub open spec fn spec_html_special(b: u8) -> bool {
    b == LT || b == GT || b == AMP
}

/// Spec: a byte that can end a JSON string or start an escape.
pub open spec fn spec_json_special(b: u8) -> bool {
    b == QUOTE || b == BACKSLASH
}

/// Spec: no byte of `s` is HTML-special.
pub open spec fn spec_html_safe(s: Seq<u8>) -> bool {
    forall|i: int| 0 <= i < s.len() ==> !spec_html_special(#[trigger] s[i])
}

/// Spec: `<`, `>`, `&` for the special bytes. Other bytes stay.
pub open spec fn spec_escape_byte(b: u8) -> Seq<u8> {
    if b == LT {
        seq![0x5cu8, 0x75u8, 0x30u8, 0x30u8, 0x33u8, 0x63u8]
    } else if b == GT {
        seq![0x5cu8, 0x75u8, 0x30u8, 0x30u8, 0x33u8, 0x65u8]
    } else if b == AMP {
        seq![0x5cu8, 0x75u8, 0x30u8, 0x30u8, 0x32u8, 0x36u8]
    } else {
        seq![b]
    }
}

/// Spec: escape each byte, in order.
pub open spec fn spec_escape(s: Seq<u8>) -> Seq<u8>
    decreases s.len(),
{
    if s.len() == 0 {
        Seq::empty()
    } else {
        spec_escape(s.drop_last()) + spec_escape_byte(s.last())
    }
}

/// The escape of one byte has no HTML-special byte.
proof fn lemma_escape_byte_safe(b: u8)
    ensures
        spec_html_safe(spec_escape_byte(b)),
{
}

/// The escape of a byte that is not special is that byte.
proof fn lemma_escape_byte_identity(b: u8)
    requires
        !spec_html_special(b),
    ensures
        spec_escape_byte(b) == seq![b],
{
}

/// Main safety property: the escaped output has no `<`, `>`, or `&`.
/// So JSON in a `<script type="application/json">` block cannot close it.
proof fn lemma_escape_safe(s: Seq<u8>)
    ensures
        spec_html_safe(spec_escape(s)),
    decreases s.len(),
{
    if s.len() > 0 {
        lemma_escape_safe(s.drop_last());
        lemma_escape_byte_safe(s.last());
        let a = spec_escape(s.drop_last());
        let b = spec_escape_byte(s.last());
        assert forall|i: int| 0 <= i < (a + b).len() implies !spec_html_special(
            #[trigger] (a + b)[i],
        ) by {
            if i < a.len() {
                assert((a + b)[i] == a[i]);
            } else {
                assert((a + b)[i] == b[i - a.len()]);
            }
        }
    }
}

/// The escape changes nothing in input that is already safe.
proof fn lemma_escape_identity(s: Seq<u8>)
    requires
        spec_html_safe(s),
    ensures
        spec_escape(s) == s,
    decreases s.len(),
{
    if s.len() > 0 {
        let p = s.drop_last();
        assert(spec_html_safe(p)) by {
            assert forall|i: int| 0 <= i < p.len() implies !spec_html_special(#[trigger] p[i]) by {
                assert(p[i] == s[i]);
            }
        }
        lemma_escape_identity(p);
        assert(!spec_html_special(s[s.len() - 1]));
        lemma_escape_byte_identity(s.last());
        assert(p + seq![s.last()] =~= s);
    }
}

/// The escape never makes the text shorter.
proof fn lemma_escape_len(s: Seq<u8>)
    ensures
        spec_escape(s).len() >= s.len(),
    decreases s.len(),
{
    if s.len() > 0 {
        lemma_escape_len(s.drop_last());
    }
}

/// Escapes one byte into `out`.
fn escape_byte_into(b: u8, out: &mut Vec<u8>)
    ensures
        final(out)@ == old(out)@ + spec_escape_byte(b),
{
    if b == LT || b == GT || b == AMP {
        out.push(0x5c);
        out.push(0x75);
        out.push(0x30);
        out.push(0x30);
        if b == LT {
            out.push(0x33);
            out.push(0x63);
        } else if b == GT {
            out.push(0x33);
            out.push(0x65);
        } else {
            out.push(0x32);
            out.push(0x36);
        }
        assert(out@ =~= old(out)@ + spec_escape_byte(b));
    } else {
        out.push(b);
        assert(out@ =~= old(out)@ + spec_escape_byte(b));
    }
}

/// Escapes JSON text for an HTML `<script>` data block.
pub fn escape_json_for_html(input: &[u8]) -> (out: Vec<u8>)
    ensures
        out@ == spec_escape(input@),
        spec_html_safe(out@),
{
    let mut out: Vec<u8> = Vec::new();
    let mut i: usize = 0;
    while i < input.len()
        invariant
            i <= input.len(),
            out@ == spec_escape(input@.subrange(0, i as int)),
        decreases input.len() - i,
    {
        let ghost before = input@.subrange(0, i as int);
        escape_byte_into(input[i], &mut out);
        let ghost after = input@.subrange(0, i as int + 1);
        assert(after.drop_last() =~= before);
        assert(after.last() == input@[i as int]);
        i = i + 1;
    }
    assert(input@.subrange(0, input.len() as int) =~= input@);
    proof {
        lemma_escape_safe(input@);
    }
    out
}

// ------------------------------------------------------------- identifiers

pub open spec fn spec_is_digit(b: u8) -> bool {
    0x30 <= b && b <= 0x39
}

pub open spec fn spec_is_name_byte(b: u8) -> bool {
    spec_is_digit(b) || (0x41 <= b && b <= 0x5a) || (0x61 <= b && b <= 0x7a) || b == 0x5f
}

/// Spec: a pixel ID is 1 to 32 ASCII digits.
pub open spec fn spec_pixel_id(s: Seq<u8>) -> bool {
    1 <= s.len() <= MAX_PIXEL_ID_LEN && forall|i: int| 0 <= i < s.len() ==> spec_is_digit(
        #[trigger] s[i],
    )
}

/// Spec: a custom event name is 1 to 50 bytes of `A-Z a-z 0-9 _`.
pub open spec fn spec_event_name(s: Seq<u8>) -> bool {
    1 <= s.len() <= MAX_EVENT_NAME_LEN && forall|i: int| 0 <= i < s.len() ==> spec_is_name_byte(
        #[trigger] s[i],
    )
}

/// Returns `true` when `s` is a valid pixel ID.
pub fn is_valid_pixel_id(s: &[u8]) -> (r: bool)
    ensures
        r == spec_pixel_id(s@),
{
    if s.len() == 0 || s.len() > MAX_PIXEL_ID_LEN {
        return false;
    }
    let mut i: usize = 0;
    while i < s.len()
        invariant
            i <= s.len(),
            forall|j: int| 0 <= j < i ==> spec_is_digit(#[trigger] s@[j]),
        decreases s.len() - i,
    {
        if !(0x30 <= s[i] && s[i] <= 0x39) {
            return false;
        }
        i = i + 1;
    }
    true
}

/// Returns `true` when `s` is a valid custom event name.
pub fn is_valid_event_name(s: &[u8]) -> (r: bool)
    ensures
        r == spec_event_name(s@),
{
    if s.len() == 0 || s.len() > MAX_EVENT_NAME_LEN {
        return false;
    }
    let mut i: usize = 0;
    while i < s.len()
        invariant
            i <= s.len(),
            forall|j: int| 0 <= j < i ==> spec_is_name_byte(#[trigger] s@[j]),
        decreases s.len() - i,
    {
        let b = s[i];
        let ok = (0x30 <= b && b <= 0x39) || (0x41 <= b && b <= 0x5a) || (0x61 <= b && b <= 0x7a)
            || b == 0x5f;
        if !ok {
            return false;
        }
        i = i + 1;
    }
    true
}

/// A valid pixel ID is safe in HTML, in a JSON string, and in a URL query.
proof fn lemma_pixel_id_safe(s: Seq<u8>)
    requires
        spec_pixel_id(s),
    ensures
        spec_html_safe(s),
        forall|i: int| 0 <= i < s.len() ==> !spec_json_special(#[trigger] s[i]),
{
}

/// A valid event name is safe in HTML and in a JSON string.
proof fn lemma_event_name_safe(s: Seq<u8>)
    requires
        spec_event_name(s),
    ensures
        spec_html_safe(s),
        forall|i: int| 0 <= i < s.len() ==> !spec_json_special(#[trigger] s[i]),
{
}

// ------------------------------------------------------------- load gate

/// Inputs to the load decision.
pub struct Gate {
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

/// Spec: the pixel is on for this request.
pub open spec fn spec_active(g: Gate) -> bool {
    g.enabled && g.has_pixels && (!g.require_consent || g.consent_granted) && !(g.honor_gpc
        && g.gpc_signal)
}

/// Returns `true` when the page gets the pixel.
pub fn active(g: &Gate) -> (r: bool)
    ensures
        r == spec_active(*g),
{
    g.enabled && g.has_pixels && (!g.require_consent || g.consent_granted) && !(g.honor_gpc
        && g.gpc_signal)
}

/// No pixel without consent, when consent is required.
proof fn lemma_no_pixel_without_consent(g: Gate)
    requires
        g.require_consent,
        !g.consent_granted,
    ensures
        !spec_active(g),
{
}

/// No pixel for a GPC request, when GPC is honored.
proof fn lemma_gpc_wins(g: Gate)
    requires
        g.honor_gpc,
        g.gpc_signal,
    ensures
        !spec_active(g),
{
}

/// The off switch wins over all other inputs.
proof fn lemma_disabled_is_off(g: Gate)
    requires
        !g.enabled || !g.has_pixels,
    ensures
        !spec_active(g),
{
}

/// More consent never turns the pixel off. Less consent never turns it on.
proof fn lemma_consent_monotonic(g: Gate)
    requires
        spec_active(g),
    ensures
        spec_active(Gate { consent_granted: true, ..g }),
{
}

fn main() {
}

} // verus!
