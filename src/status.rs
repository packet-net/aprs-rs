//! Status reports (APRS12c ch. 16): text, optionally a DHM zulu timestamp, a Maidenhead grid
//! locator with a symbol, and a meteor-scatter beam heading and power.

use alloc::string::String;
use alloc::vec::Vec;

use crate::context::Context;
use crate::{BeamHeading, Code, Data, EncodeError, ParseOptions, Status, Symbol, Timestamp, position, text};

pub(crate) fn decode(ctx: &mut Context, info: &[u8]) -> Option<Data> {
    let mut rest = &info[1..];
    let mut at = 1;
    let mut status = Status::default();

    if let Some(t) = rest.get(..7).and_then(Timestamp::parse).filter(|t| matches!(t, Timestamp::DayHourMinute { utc: true, .. })) {
        if !t.is_valid() && !ctx.tolerate(Code::InvalidTimestamp, "the timestamp is out of range (APRS12c ch. 6)", Some(at)) {
            return None;
        }
        status.timestamp = Some(t);
        rest = &rest[7..];
        at += 7;
    } else if let Some((len, symbol)) = grid(rest) {
        status.locator = Some(String::from_utf8_lossy(&rest[..len]).to_ascii_uppercase());
        status.symbol = Some(symbol);
        rest = &rest[len + 2..];
        at += len + 2;
        if let Some((&first, after)) = rest.split_first() {
            if first == b' ' {
                rest = after;
                at += 1;
            } else if !ctx.tolerate(
                Code::MissingSpaceAfterLocator,
                "status text straight after the grid locator and symbol; a space comes first (UAP 5.17)",
                Some(at),
            ) {
                return None;
            }
        }
    }

    let mut body = rest;
    if let [head @ .., b'^', h, p] = rest {
        if (h.is_ascii_digit() || h.is_ascii_uppercase()) && (b'1'..=b'K').contains(p) && !(b'L'..=b'Z').contains(p) {
            status.beam = Some(BeamHeading { heading_code: *h as char, power_code: *p as char });
            body = head;
        }
    }
    status.text = text::decode(ctx, body, at)?;
    Some(Data::Status(status))
}

/// A 6- or 4-character locator followed by a symbol. A bare 6-character locator followed by
/// text is plain text; the 4-character reading is tried only when the first six characters are
/// not a locator (interpretations.md).
fn grid(rest: &[u8]) -> Option<(usize, Symbol)> {
    let symbol_at = |i: usize| -> Option<Symbol> {
        let (t, c) = (*rest.get(i)?, *rest.get(i + 1)?);
        let mut scratch = Context::new(ParseOptions::STRICT);
        position::symbol(&mut scratch, t, c, 0, 0, false)
    };
    if is_locator(rest.get(..6)?) {
        return symbol_at(6).map(|s| (6, s));
    }
    if is_locator(rest.get(..4)?) {
        return symbol_at(4).map(|s| (4, s));
    }
    None
}

pub(crate) fn is_locator(b: &[u8]) -> bool {
    let field = |c: u8| (b'A'..=b'R').contains(&c.to_ascii_uppercase());
    let sub = |c: u8| (b'A'..=b'X').contains(&c.to_ascii_uppercase());
    match b {
        [a, b2, c, d] => field(*a) && field(*b2) && c.is_ascii_digit() && d.is_ascii_digit(),
        [a, b2, c, d, e, f] => field(*a) && field(*b2) && c.is_ascii_digit() && d.is_ascii_digit() && sub(*e) && sub(*f),
        _ => false,
    }
}

pub(crate) fn encode(s: &Status) -> Result<Vec<u8>, EncodeError> {
    let mut out = Vec::from(&b">"[..]);
    if let Some(t) = &s.timestamp {
        if !matches!(t, Timestamp::DayHourMinute { utc: true, .. }) {
            return Err(EncodeError::new("a status timestamp is DHM zulu, DDHHMMz (APRS12c ch. 16)"));
        }
        if s.locator.is_some() {
            return Err(EncodeError::new("a status report with a grid locator cannot have a timestamp (APRS12c ch. 16)"));
        }
        crate::encode::dhm_or_hms(&mut out, t)?;
    }
    match (&s.locator, s.symbol) {
        (Some(loc), Some(symbol)) => {
            if !is_locator(loc.as_bytes()) {
                return Err(EncodeError::new("a grid locator is 4 or 6 characters, e.g. IO91 or IO91SX"));
            }
            position::check_symbol(symbol)?;
            out.extend_from_slice(loc.to_ascii_uppercase().as_bytes());
            out.push(symbol.table as u8);
            out.push(symbol.code as u8);
            if !s.text.is_empty() || s.beam.is_some() {
                out.push(b' ');
            }
        }
        (None, None) => {}
        _ => return Err(EncodeError::new("a status grid locator and its symbol go together")),
    }
    if crate::text::has_line_break(s.text.as_bytes()) {
        return Err(EncodeError::new("status text cannot contain a line break"));
    }
    out.extend_from_slice(s.text.as_bytes());
    if let Some(b) = s.beam {
        let (h, p) = (b.heading_code as u8, b.power_code as u8);
        if !(h.is_ascii_digit() || h.is_ascii_uppercase()) || !(b'1'..=b'K').contains(&p) || (b'L'..=b'Z').contains(&p) {
            return Err(EncodeError::new("a beam heading is 0-9 or A-Z, and the power code 1-9 or : to K (APRS12c ch. 16)"));
        }
        out.extend_from_slice(&[b'^', h, p]);
    }
    // The text must read back as itself: status text that looks like a timestamp or a grid
    // locator would not.
    let mut ctx = Context::new(ParseOptions::LENIENT);
    match decode(&mut ctx, &out) {
        Some(Data::Status(back))
            if back == *s
                || (back.locator.as_deref().map(str::to_ascii_uppercase) == s.locator.as_deref().map(str::to_ascii_uppercase)
                    && back.text == s.text
                    && back.beam == s.beam
                    && back.timestamp == s.timestamp) =>
        {
            Ok(out)
        }
        _ => Err(EncodeError::new("status text that would read back as a timestamp, grid locator or beam heading")),
    }
}
