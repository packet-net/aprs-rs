//! Messages, acks and rejects, bulletins, telemetry metadata and directed queries: everything
//! sent as `:ADDRESSEE:text` (APRS12c ch. 13-15).

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::context::Context;
use crate::{
    Ack, Bulletin, Code, Data, DirectedQuery, EncodeError, Message, Reject, TelemetryBits, TelemetryCoefficients, TelemetryLabels, text,
};

/// Message text senders may write (APRS12c ch. 14); receivers accept more.
const MAX_TEXT: usize = 67;

const QUERY_TYPES: [&str; 8] = ["APRSD", "APRSH", "APRSM", "APRSO", "APRSP", "APRSS", "APRST", "PING?"];

pub(crate) fn decode(ctx: &mut Context, info: &[u8]) -> Option<Data> {
    let colon = if info.len() > 10 && info[10] == b':' {
        10
    } else {
        // An addressee that was not padded: the second colon comes early, but not straight away.
        match info.iter().take(11).skip(1).position(|&b| b == b':') {
            Some(i) if i >= 1 => {
                if !ctx.tolerate(Code::UnpaddedAddressee, "the addressee is not padded to 9 characters", Some(1)) {
                    return None;
                }
                i + 1
            }
            _ => {
                ctx.error(
                    Code::InvalidMessage,
                    "a message is :ADDRESSEE:text, the addressee padded to 9 characters (APRS12c ch. 14)",
                    Some(0),
                );
                return None;
            }
        }
    };
    let raw = &info[1..colon];
    if !text::is_printable_ascii(raw) {
        ctx.error(Code::InvalidMessage, "the addressee is not printable ASCII", Some(1));
        return None;
    }
    let mut end = raw.len();
    while end > 0 && raw[end - 1] == b' ' {
        end -= 1;
    }
    let addressee_bytes = &raw[..end];
    if addressee_bytes.is_empty() {
        ctx.error(Code::InvalidMessage, "the addressee is empty", Some(1));
        return None;
    }
    if addressee_bytes.iter().any(|&b| b == b' ' || b == b':')
        && !ctx.tolerate(Code::InvalidAddresseeCharacters, "the addressee contains a space or : (APRS12c ch. 14)", Some(1))
    {
        return None;
    }
    let addressee = String::from_utf8_lossy(addressee_bytes).into_owned();
    let body = &info[colon + 1..];
    let text_offset = colon + 1;

    // Acks and rejects: ackNNNNN / rejNNNNN, optionally }RR (a reply-ack).
    if body.starts_with(b"ack") || body.starts_with(b"rej") {
        match ack_id(ctx, &body[3..], text_offset + 3) {
            Some(Err(())) => return None,
            Some(Ok((id, reply_ack))) => {
                return Some(if body[0] == b'r' {
                    Data::Reject(Reject { addressee, rejected_id: id, message_id: None, reply_ack })
                } else {
                    Data::Ack(Ack { addressee, acked_id: id, message_id: None, reply_ack })
                });
            }
            None => {}
        }
    }

    // The structure is checked before the text's encoding, so a strict decoder names the first.
    if let Some(nws) = bulletin_kind(ctx, &addressee)? {
        let (text_bytes, message_id, _) = split_message_id(body, false);
        if !brace_ok(ctx, text_bytes, text_offset) {
            return None;
        }
        let text_value = text::decode(ctx, text_bytes, text_offset)?;
        let b = Bulletin { addressee, text: text_value, message_id };
        return Some(if nws { Data::NwsBulletin(b) } else { Data::Bulletin(b) });
    }
    if body.len() >= 5 && body[4] == b'.' {
        if let Some(data) = telemetry_metadata(ctx, &addressee, body, text_offset) {
            return data;
        }
    }
    if body.len() > 1 && body[0] == b'?' {
        if let Some(data) = directed_query(ctx, &addressee, body, text_offset) {
            return Some(data);
        }
    }
    let (text_bytes, message_id, reply_ack) = split_message_id(body, true);
    if !brace_ok(ctx, text_bytes, text_offset) {
        return None;
    }
    let text_value = text::decode(ctx, text_bytes, text_offset)?;
    Some(Data::Message(Message { addressee, text: text_value, message_id, reply_ack }))
}

/// Message text never contains `{`, which starts the message ID (APRS12c ch. 14); one left in the
/// text is not followed by a valid ID, a tolerated defect.
fn brace_ok(ctx: &mut Context, text: &[u8], offset: usize) -> bool {
    match text.iter().position(|&b| b == b'{') {
        None => true,
        Some(i) => ctx.tolerate(
            Code::BraceInMessageText,
            "message text contains a { that does not start a message ID (APRS12c ch. 14)",
            Some(offset + i),
        ),
    }
}

fn is_id(b: &[u8]) -> bool {
    (1..=5).contains(&b.len()) && b.iter().all(u8::is_ascii_alphanumeric)
}

/// The ID after ack or rej: 1-5 letters or digits, optionally `}` and a reply-ack, optionally a
/// (tolerated) message ID of its own. `Some(Ok)` with the ID and reply-ack, `Some(Err)` when a
/// defect was not tolerated, `None` when this is not an ack at all.
#[allow(clippy::type_complexity)]
fn ack_id(ctx: &mut Context, rest: &[u8], offset: usize) -> Option<Result<(String, Option<String>), ()>> {
    let brace = rest.iter().position(|&b| b == b'{');
    let core = &rest[..brace.unwrap_or(rest.len())];
    let close = core.iter().position(|&b| b == b'}');
    let id = &core[..close.unwrap_or(core.len())];
    let reply = close.map(|c| &core[c + 1..]);
    if !is_id(id) || reply.is_some_and(|r| !r.is_empty() && !is_id(r)) {
        return None;
    }
    if let Some(b) = brace {
        if !is_id(&rest[b + 1..]) {
            return None;
        }
        if !ctx.tolerate(Code::MessageIdOnAck, "an ack or rej carries a message ID of its own (UAP 5.32)", Some(offset + b)) {
            return Some(Err(()));
        }
    }
    let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    Some(Ok((text(id), reply.map(text))))
}

/// `text{MM`, and for a message also `text{MM}` (reply-ack capable) or `text{MM}AA` (and acking
/// AA): the text, the ID and the reply-ack.
fn split_message_id(body: &[u8], reply_ack: bool) -> (&[u8], Option<String>, Option<String>) {
    let Some(brace) = body.iter().rposition(|&b| b == b'{') else {
        return (body, None, None);
    };
    let tail = &body[brace + 1..];
    let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    match tail.iter().position(|&b| b == b'}') {
        None if is_id(tail) => (&body[..brace], Some(text(tail)), None),
        Some(i) if reply_ack && is_id(&tail[..i]) && (i + 1 == tail.len() || is_id(&tail[i + 1..])) => {
            (&body[..brace], Some(text(&tail[..i])), Some(text(&tail[i + 1..])))
        }
        _ => (body, None, None),
    }
}

/// Whether the addressee makes this a bulletin: `Some(Some(true))` for an NWS bulletin,
/// `Some(Some(false))` for `BLNn`, `BLNx` or `BLNnGROUP`, `Some(None)` for neither, and `None`
/// when a defect was not tolerated.
fn bulletin_kind(ctx: &mut Context, addressee: &str) -> Option<Option<bool>> {
    if addressee.starts_with("NWS-") || addressee.starts_with("NWS_") {
        return Some(Some(true));
    }
    let Some(rest) = addressee.strip_prefix("BLN") else {
        return Some(None);
    };
    match rest.as_bytes() {
        [d] if d.is_ascii_digit() || d.is_ascii_uppercase() => Some(Some(false)),
        [d, group @ ..] if d.is_ascii_digit() && !group.is_empty() && group.len() <= 5 => Some(Some(false)),
        [d, group @ ..] if d.is_ascii_uppercase() && !group.is_empty() && group.len() <= 5 => {
            if !ctx.tolerate(
                Code::LetterGroupBulletin,
                "a group bulletin has a digit before the group name, not a letter (APRS12c ch. 14)",
                Some(1),
            ) {
                return None;
            }
            Some(Some(false))
        }
        _ => Some(None),
    }
}

/// `PARM.`, `UNIT.`, `EQNS.` and `BITS.` (APRS12c ch. 13). `None` when this is not metadata (and a
/// malformed list is not: it stays a plain message); `Some(None)` when it is, but its text has a
/// defect that was not tolerated.
fn telemetry_metadata(ctx: &mut Context, addressee: &str, body: &[u8], offset: usize) -> Option<Option<Data>> {
    let kind = &body[..5];
    if !matches!(kind, b"PARM." | b"UNIT." | b"EQNS." | b"BITS.") {
        return None;
    }
    let (list, message_id, _) = split_message_id(&body[5..], false);
    // Checked quietly first: if this turns out not to be metadata, the text is reported once, as
    // a plain message.
    let s = text::for_display(list);
    let addressee = addressee.to_string();
    let data = match kind {
        b"PARM." | b"UNIT." => {
            let labels: Vec<String> = s.split(',').map(str::to_string).collect();
            if labels.len() > 13 {
                ctx.info(Code::InvalidTelemetryMetadata, "telemetry metadata names at most 13 channels (APRS12c ch. 13)", Some(offset));
                return None;
            }
            let t = TelemetryLabels { addressee, labels, message_id };
            if kind == b"PARM." { Data::TelemetryNames(t) } else { Data::TelemetryUnits(t) }
        }
        b"EQNS." => {
            // "The list may stop at any field" (APRS12c ch. 13): trailing empty entries are the list stopping.
            let mut coefficients = Vec::new();
            for part in s.trim_end_matches(' ').trim_end_matches(',').split(',') {
                let part = part.trim();
                if !is_coefficient(part) {
                    ctx.info(
                        Code::InvalidTelemetryMetadata,
                        "a telemetry equation coefficient is not a number (APRS12c ch. 13)",
                        Some(offset + 5),
                    );
                    return None;
                }
                coefficients.push(part.to_string());
            }
            if coefficients.len() > 15 {
                ctx.info(
                    Code::InvalidTelemetryMetadata,
                    "telemetry equations are up to 15 numbers, a, b and c for each analog channel (APRS12c ch. 13)",
                    Some(offset + 5),
                );
                return None;
            }
            Data::TelemetryCoefficients(TelemetryCoefficients { addressee, coefficients, message_id })
        }
        _ => {
            let (bits, project) = s.split_once(',').unwrap_or((&s, ""));
            if bits.len() != 8 || !bits.bytes().all(|b| b == b'0' || b == b'1') {
                ctx.info(
                    Code::InvalidTelemetryMetadata,
                    "telemetry bit sense is eight 0 or 1 characters (APRS12c ch. 13)",
                    Some(offset + 5),
                );
                return None;
            }
            Data::TelemetryBits(TelemetryBits { addressee, bits: bits.to_string(), project: project.to_string(), message_id })
        }
    };
    // It is metadata: now its text's defects are reported.
    if !brace_ok(ctx, list, offset + 5) || text::decode(ctx, list, offset + 5).is_none() {
        return Some(None);
    }
    Some(Some(data))
}

/// A telemetry equation coefficient: a decimal number with an optional sign and exponent.
pub(crate) fn is_coefficient(t: &str) -> bool {
    let (mantissa, exponent) = match t.find(['e', 'E']) {
        Some(i) => (&t[..i], Some(&t[i + 1..])),
        None => (t, None),
    };
    let unsigned = |t: &str| t.strip_prefix(['-', '+']).unwrap_or(t).to_string();
    let m = unsigned(mantissa);
    let (whole, frac) = m.split_once('.').unwrap_or((&m, ""));
    let digits = |t: &str| t.bytes().all(|b| b.is_ascii_digit());
    !(whole.is_empty() && frac.is_empty())
        && digits(whole)
        && digits(frac)
        && exponent.is_none_or(|e| {
            let e = unsigned(e);
            !e.is_empty() && digits(&e)
        })
}

pub(crate) fn is_decimal(t: &str) -> bool {
    let t = t.strip_prefix(['-', '+']).unwrap_or(t);
    let (whole, frac) = t.split_once('.').unwrap_or((t, ""));
    !(whole.is_empty() && frac.is_empty()) && whole.bytes().all(|b| b.is_ascii_digit()) && frac.bytes().all(|b| b.is_ascii_digit())
}

/// A directed query: `?` and a query type, optionally the one callsign some types ask about
/// (APRS12c ch. 15). An upper-case type the spec does not define is still a query, which the
/// recipient ignores. `None` (with an `Info` for what looked like a query) when it is a plain message.
fn directed_query(ctx: &mut Context, addressee: &str, body: &[u8], offset: usize) -> Option<Data> {
    let s: String = body[1..].iter().map(|&b| b as char).collect();
    let not_a_query = |ctx: &mut Context, why: &str| {
        ctx.info(Code::InvalidQuery, why, Some(offset));
        None
    };
    if s.contains('{') {
        return not_a_query(ctx, "a directed query never has a message ID; read as a message (UAP 5.18)");
    }
    let target_ok = |t: &str| t.chars().count() <= 9 && t.chars().all(|c| ('!'..='~').contains(&c));
    let query = |query_type: &str, target: &str| {
        Some(Data::DirectedQuery(DirectedQuery {
            addressee: addressee.to_string(),
            query_type: query_type.to_string(),
            target: (!target.is_empty()).then(|| target.to_string()),
        }))
    };
    for t in QUERY_TYPES {
        if let Some(rest) = s.strip_prefix(t) {
            let target = rest.trim();
            if !target_ok(target) {
                return not_a_query(ctx, "a query target is one callsign of up to 9 characters; read as a message");
            }
            return query(t, target);
        }
        if s.len() >= t.len() && s.as_bytes()[..t.len()].eq_ignore_ascii_case(t.as_bytes()) {
            return not_a_query(ctx, "query types are upper case; read as a message (UAP 5.18)");
        }
    }
    let end = s.bytes().take_while(|b| b.is_ascii_uppercase() || b.is_ascii_digit()).count();
    if end == 0 || (end < s.len() && s.as_bytes()[end] != b' ') {
        return None;
    }
    let target = s[end..].trim();
    if !target_ok(target) {
        return not_a_query(ctx, "a query target is one callsign of up to 9 characters; read as a message");
    }
    query(&s[..end], target)
}

// ------------------------------------------------------------------ encoding

pub(crate) fn encode(data: &Data) -> Result<Vec<u8>, EncodeError> {
    let mut out = Vec::new();
    match data {
        Data::Message(m) => {
            addressee(&mut out, &m.addressee)?;
            message_text(&mut out, &m.text)?;
            id_and_reply(&mut out, &m.message_id, &m.reply_ack)?;
        }
        Data::Ack(a) => {
            addressee(&mut out, &a.addressee)?;
            ack_or_reject(&mut out, b"ack", &a.acked_id, &a.message_id, &a.reply_ack)?;
        }
        Data::Reject(r) => {
            addressee(&mut out, &r.addressee)?;
            ack_or_reject(&mut out, b"rej", &r.rejected_id, &r.message_id, &r.reply_ack)?;
        }
        Data::Bulletin(b) | Data::NwsBulletin(b) => {
            let nws = b.addressee.starts_with("NWS-") || b.addressee.starts_with("NWS_");
            if matches!(data, Data::NwsBulletin(_)) != nws {
                return Err(EncodeError::new("an NWS bulletin is addressed NWS-xxxxx, and other bulletins are not"));
            }
            if !nws {
                let rest = b.addressee.strip_prefix("BLN").unwrap_or("");
                let r = rest.as_bytes();
                let ok = match r {
                    [d] => d.is_ascii_digit() || d.is_ascii_uppercase(),
                    [d, group @ ..] => d.is_ascii_digit() && group.len() <= 5,
                    _ => false,
                };
                if !ok {
                    return Err(EncodeError::new(
                        "a bulletin is addressed BLN and a digit or letter, or BLN, a digit and a group name of up to 5 (APRS12c ch. 14)",
                    ));
                }
            }
            addressee(&mut out, &b.addressee)?;
            message_text(&mut out, &b.text)?;
            id_and_reply(&mut out, &b.message_id, &None)?;
        }
        Data::TelemetryNames(t) | Data::TelemetryUnits(t) => {
            if t.labels.len() > 13
                || t.labels.iter().any(|l| l.contains(',') || l.contains('{') || crate::text::has_line_break(l.as_bytes()))
            {
                return Err(EncodeError::new("telemetry metadata names at most 13 channels, without commas or braces (APRS12c ch. 13)"));
            }
            addressee(&mut out, &t.addressee)?;
            out.extend_from_slice(if matches!(data, Data::TelemetryNames(_)) { b"PARM." } else { b"UNIT." });
            out.extend_from_slice(t.labels.join(",").as_bytes());
            id_and_reply(&mut out, &t.message_id, &None)?;
        }
        Data::TelemetryCoefficients(t) => {
            if t.coefficients.len() > 15 || !t.coefficients.iter().all(|c| is_coefficient(c)) {
                return Err(EncodeError::new("telemetry equations are up to 15 numbers (APRS12c ch. 13)"));
            }
            addressee(&mut out, &t.addressee)?;
            out.extend_from_slice(b"EQNS.");
            out.extend_from_slice(t.coefficients.join(",").as_bytes());
            id_and_reply(&mut out, &t.message_id, &None)?;
        }
        Data::TelemetryBits(t) => {
            if t.bits.len() != 8
                || !t.bits.bytes().all(|b| b == b'0' || b == b'1')
                || t.project.chars().count() > 23
                || crate::text::has_line_break(t.project.as_bytes())
            {
                return Err(EncodeError::new(
                    "telemetry bit sense is eight 0 or 1 characters, and the project title up to 23 (APRS12c ch. 13)",
                ));
            }
            addressee(&mut out, &t.addressee)?;
            out.extend_from_slice(b"BITS.");
            out.extend_from_slice(t.bits.as_bytes());
            if !t.project.is_empty() {
                out.push(b',');
                out.extend_from_slice(t.project.as_bytes());
            }
            id_and_reply(&mut out, &t.message_id, &None)?;
        }
        Data::DirectedQuery(q) => {
            if q.query_type.is_empty() || !q.query_type.bytes().all(|b| b.is_ascii_graphic() && b != b'{' && !b.is_ascii_lowercase()) {
                return Err(EncodeError::new(
                    "a directed query type is upper-case printable ASCII: ?APRSD, ?APRSH, ?APRSM, ?APRSO, ?APRSP, ?APRSS, ?APRST, ?PING? (APRS12c ch. 15)",
                ));
            }
            addressee(&mut out, &q.addressee)?;
            out.push(b'?');
            out.extend_from_slice(q.query_type.as_bytes());
            if let Some(t) = &q.target {
                if t.is_empty() || t.len() > 9 || !t.bytes().all(|b| b.is_ascii_graphic()) {
                    return Err(EncodeError::new("a directed query's target is one callsign"));
                }
                out.extend_from_slice(t.as_bytes());
            }
        }
        _ => return Err(EncodeError::new("not a message")),
    }
    Ok(out)
}

fn addressee(out: &mut Vec<u8>, a: &str) -> Result<(), EncodeError> {
    let b = a.as_bytes();
    if b.is_empty() || b.len() > 9 || !text::is_printable_ascii(b) || b.contains(&b' ') || b.contains(&b':') {
        return Err(EncodeError::new("an addressee is 1-9 printable characters without spaces or : (APRS12c ch. 14)"));
    }
    out.push(b':');
    out.extend_from_slice(b);
    out.extend(core::iter::repeat_n(b' ', 9 - b.len()));
    out.push(b':');
    Ok(())
}

fn message_text(out: &mut Vec<u8>, t: &str) -> Result<(), EncodeError> {
    if t.chars().count() > MAX_TEXT {
        return Err(EncodeError::new("message text is limited to 67 characters (APRS12c ch. 14)"));
    }
    if t.contains('{') {
        return Err(EncodeError::new("message text must not contain '{', which starts the message ID (APRS12c ch. 14)"));
    }
    if crate::text::has_line_break(t.as_bytes()) {
        return Err(EncodeError::new("message text cannot contain a line break"));
    }
    out.extend_from_slice(t.as_bytes());
    Ok(())
}

fn id_ok(id: &str) -> bool {
    (1..=5).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

fn id_and_reply(out: &mut Vec<u8>, id: &Option<String>, reply_ack: &Option<String>) -> Result<(), EncodeError> {
    match (id, reply_ack) {
        (None, None) => {}
        (None, Some(_)) => return Err(EncodeError::new("a reply-ack goes with a message ID")),
        (Some(id), reply) => {
            if !id_ok(id) {
                return Err(EncodeError::new("a message ID is 1-5 letters or digits (APRS12c ch. 14)"));
            }
            out.push(b'{');
            out.extend_from_slice(id.as_bytes());
            if let Some(r) = reply {
                if !r.is_empty() && !id_ok(r) {
                    return Err(EncodeError::new("a reply-ack is 1-5 letters or digits"));
                }
                out.push(b'}');
                out.extend_from_slice(r.as_bytes());
            }
        }
    }
    Ok(())
}

fn ack_or_reject(
    out: &mut Vec<u8>,
    kind: &[u8],
    id: &str,
    message_id: &Option<String>,
    reply_ack: &Option<String>,
) -> Result<(), EncodeError> {
    if message_id.is_some() {
        return Err(EncodeError::new("an ack or rej carries no message ID of its own (UAP 5.32)"));
    }
    if !id_ok(id) {
        return Err(EncodeError::new("the message ID being acked or rejected is 1-5 letters or digits"));
    }
    out.extend_from_slice(kind);
    out.extend_from_slice(id.as_bytes());
    if let Some(r) = reply_ack {
        if !r.is_empty() && !id_ok(r) {
            return Err(EncodeError::new("a reply-ack is 1-5 letters or digits"));
        }
        out.push(b'}');
        out.extend_from_slice(r.as_bytes());
    }
    Ok(())
}
