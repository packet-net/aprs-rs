//! Telemetry reports, `T#sss,111,222,333,444,555,xxxxxxxx` (APRS12c ch. 13).

use alloc::string::String;
use alloc::vec::Vec;

use crate::context::Context;
use crate::message::is_decimal;
use crate::{Code, Data, EncodeError, Telemetry, text};

pub(crate) fn report(ctx: &mut Context, info: &[u8]) -> Option<Data> {
    let Some(body) = info.strip_prefix(b"T#") else {
        ctx.error(Code::InvalidTelemetry, "a telemetry report starts T# (APRS12c ch. 13)", Some(0));
        return None;
    };
    // The structure is ASCII and is checked first; only the comment after the bits is text.
    let (sequence, mut at) = if body.starts_with(b"MIC") {
        (&body[..3], if body.get(3) == Some(&b',') { 4 } else { 3 })
    } else {
        let end = body.iter().position(|&b| b == b',').unwrap_or(body.len());
        (&body[..end], (end + 1).min(body.len()))
    };
    let mut t = Telemetry { sequence: String::from_utf8_lossy(sequence).into_owned(), ..Telemetry::default() };
    let mut well_formed = sequence == b"MIC" || (!sequence.is_empty() && sequence.len() <= 3);
    for _ in 0..5 {
        if at >= body.len() && t.analog.len() < 5 {
            well_formed = false;
            break;
        }
        let end = body[at..].iter().position(|&b| b == b',').map_or(body.len(), |i| at + i);
        let field = &body[at..end];
        match core::str::from_utf8(field) {
            Ok("") => t.analog.push(None),
            Ok(v) if is_decimal(v) => t.analog.push(Some(String::from(v))),
            _ => {
                ctx.error(Code::InvalidTelemetry, "a telemetry value is not a number (APRS12c ch. 13)", Some(2 + at));
                return None;
            }
        }
        at = if end < body.len() { end + 1 } else { body.len() };
        if end == body.len() {
            if t.analog.len() < 5 {
                well_formed = false;
            }
            break;
        }
    }
    let rest = &body[at..];
    let mut comment_from = None;
    if t.analog.len() == 5 && end_reached(body, at) {
        well_formed = false;
    } else if !rest.is_empty() {
        let bits = rest.iter().take_while(|b| **b == b'0' || **b == b'1').count();
        if bits >= 8 {
            t.bits = Some(String::from_utf8_lossy(&rest[..8]).into_owned());
            comment_from = Some(at + 8);
        } else {
            well_formed = false;
            comment_from = Some(at);
        }
    }
    if !well_formed
        && !ctx.tolerate(
            Code::InvalidTelemetry,
            "a telemetry report carries a sequence, 5 analog values and the 8 digital bits (APRS12c ch. 13)",
            Some(2),
        )
    {
        return None;
    }
    if let Some(from) = comment_from {
        t.comment = text::decode(ctx, &body[from..], 2 + from)?;
    }
    Some(Data::Telemetry(t))
}

/// Whether nothing follows the five analog values.
fn end_reached(body: &[u8], at: usize) -> bool {
    at >= body.len()
}

pub(crate) fn encode(t: &Telemetry) -> Result<Vec<u8>, EncodeError> {
    let seq_ok =
        t.sequence == "MIC" || (!t.sequence.is_empty() && t.sequence.len() <= 3 && t.sequence.bytes().all(|b| b.is_ascii_alphanumeric()));
    let bits_ok = t.bits.as_ref().is_some_and(|b| b.len() == 8 && b.bytes().all(|x| x == b'0' || x == b'1'));
    if !seq_ok || t.analog.len() != 5 || !bits_ok || !t.analog.iter().flatten().all(|v| is_decimal(v)) {
        return Err(EncodeError::new("a telemetry report carries 5 analog values and the 8 digital bits (APRS12c ch. 13)"));
    }
    if crate::text::has_line_break(t.comment.as_bytes()) {
        return Err(EncodeError::new("telemetry comment text cannot contain a line break"));
    }
    let mut out = Vec::from(&b"T#"[..]);
    out.extend_from_slice(t.sequence.as_bytes());
    // MIC may or may not be followed by a comma; it is written without (APRS12c ch. 13).
    if t.sequence != "MIC" {
        out.push(b',');
    }
    for v in &t.analog {
        if let Some(v) = v {
            out.extend_from_slice(v.as_bytes());
        }
        out.push(b',');
    }
    out.extend_from_slice(t.bits.as_deref().unwrap_or_default().as_bytes());
    out.extend_from_slice(t.comment.as_bytes());
    Ok(out)
}
