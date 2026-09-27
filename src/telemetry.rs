//! Telemetry reports, `T#sss,111,222,333,444,555,xxxxxxxx` (APRS12c ch. 13).

use alloc::string::String;
use alloc::vec::Vec;

use crate::context::Context;
use crate::message::is_decimal;
use crate::{Code, Data, EncodeError, ParseOptions, Telemetry, text};

pub(crate) fn report(ctx: &mut Context, info: &[u8]) -> Option<Data> {
    let Some(body) = info.strip_prefix(b"T#") else {
        ctx.error(Code::InvalidTelemetry, "a telemetry report starts T# (APRS12c ch. 13)", Some(0));
        return None;
    };
    // The structure is ASCII and is checked first; only the comment after the bits is text.
    // "MIC may or may not be followed by a comma"; any other sequence is letters or digits, of any
    // length (real reports count past 999), and ends at a comma.
    let (sequence, mut at) = if body.starts_with(b"MIC") {
        (&body[..3], if body.get(3) == Some(&b',') { 4 } else { 3 })
    } else {
        match body.iter().position(|&b| b == b',') {
            Some(end) if end > 0 => {
                if !body[..end].iter().all(u8::is_ascii_alphanumeric) {
                    ctx.error(Code::InvalidTelemetry, "the telemetry sequence number is letters or digits", Some(2));
                    return None;
                }
                (&body[..end], end + 1)
            }
            _ => {
                ctx.error(Code::InvalidTelemetry, "the telemetry sequence number is not followed by ','", Some(2));
                return None;
            }
        }
    };
    let mut t = Telemetry { sequence: String::from_utf8_lossy(sequence).into_owned(), ..Telemetry::default() };
    // Up to five values, each ending at a comma or the end; an empty value is missing, and one
    // left empty by a trailing comma still counts.
    while t.analog.len() < 5 {
        let end = body[at..].iter().position(|&b| b == b',').map_or(body.len(), |i| at + i);
        match core::str::from_utf8(&body[at..end]) {
            Ok("") => t.analog.push(None),
            Ok(v) if is_decimal(v) => t.analog.push(Some(String::from(v))),
            _ => {
                ctx.error(Code::InvalidTelemetry, "a telemetry value is not a number (APRS12c ch. 13)", Some(2 + at));
                return None;
            }
        }
        at = end;
        if at >= body.len() {
            break;
        }
        at += 1;
    }
    // The 8 bits only after all five values; anything after them is the comment.
    if t.analog.len() == 5 && body.len() >= at + 8 && body[at..at + 8].iter().all(|b| *b == b'0' || *b == b'1') {
        t.bits = Some(String::from_utf8_lossy(&body[at..at + 8]).into_owned());
        at += 8;
    }
    if (t.analog.len() < 5 || t.bits.is_none())
        && !ctx.tolerate(
            Code::InvalidTelemetry,
            "a telemetry report carries a sequence, 5 analog values and the 8 digital bits (APRS12c ch. 13)",
            Some(2),
        )
    {
        return None;
    }
    t.comment = text::decode(ctx, &body[at..], 2 + at)?;
    Some(Data::Telemetry(t))
}

pub(crate) fn encode(t: &Telemetry) -> Result<Vec<u8>, EncodeError> {
    let seq_ok = !t.sequence.is_empty() && t.sequence.bytes().all(|b| b.is_ascii_alphanumeric());
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
    // A sequence that starts MIC but is not MIC would not read back, for one.
    let mut ctx = Context::new(ParseOptions::LENIENT);
    match report(&mut ctx, &out) {
        Some(Data::Telemetry(back)) if back == *t => Ok(out),
        _ => Err(EncodeError::new("the telemetry report would not read back as given (a sequence starting MIC is MIC itself)")),
    }
}
