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
        let Some(i) = info.iter().take(11).skip(1).position(|&b| b == b':').map(|i| i + 1) else {
            ctx.error(Code::InvalidMessage, "a message is :ADDRESSEE:text, the addressee padded to 9 characters (APRS12c ch. 14)", Some(0));
            return None;
        };
        if !ctx.tolerate(Code::UnpaddedAddressee, "the addressee is not padded to 9 characters", Some(1)) {
            return None;
        }
        i
    };
    let raw = &info[1..colon];
    let mut end = raw.len();
    while end > 0 && raw[end - 1] == b' ' {
        end -= 1;
    }
    let addressee_bytes = &raw[..end];
    if addressee_bytes.is_empty() || !text::is_printable_ascii(addressee_bytes) {
        ctx.error(Code::InvalidMessage, "the addressee is empty or not printable ASCII", Some(1));
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
    for (prefix, reject) in [(&b"ack"[..], false), (&b"rej"[..], true)] {
        if let Some(rest) = body.strip_prefix(prefix) {
            if let Some((id, reply_ack)) = ack_id(ctx, rest, text_offset + 3) {
                let Ok(()) = id else { return None };
                return Some(if reject {
                    Data::Reject(Reject { addressee, rejected_id: id_string(rest), message_id: None, reply_ack })
                } else {
                    Data::Ack(Ack { addressee, acked_id: id_string(rest), message_id: None, reply_ack })
                });
            }
        }
    }

    let (text_bytes, message_id, reply_ack) = split_message_id(body);
    // The structure is checked before the text's encoding, so a strict decoder names the first.
    let bulletin = bulletin_kind(ctx, &addressee)?;
    if text_bytes.contains(&b'{')
        && !ctx.tolerate(
            Code::BraceInMessageText,
            "message text contains a { that does not start a message ID (APRS12c ch. 14)",
            Some(text_offset),
        )
    {
        return None;
    }
    let text_value = text::decode(ctx, text_bytes, text_offset)?;

    if let Some(nws) = bulletin {
        let b = Bulletin { addressee, text: text_value, message_id };
        return Some(if nws { Data::NwsBulletin(b) } else { Data::Bulletin(b) });
    }
    if let Some(data) = telemetry_metadata(ctx, &addressee, &text_value, text_offset) {
        return Some(data);
    }
    if text_value.starts_with('?') {
        if message_id.is_none() && reply_ack.is_none() {
            if let Some(data) = directed_query(&addressee, text_value.as_bytes()) {
                return Some(data);
            }
        }
        ctx.info(
            Code::InvalidQuery,
            "text starting with ? that is not a directed query; read as a message (APRS12c ch. 15)",
            Some(text_offset),
        );
    }
    Some(Data::Message(Message { addressee, text: text_value, message_id, reply_ack }))
}

/// The ID after ack or rej: `Some(Ok)` with its reply-ack, `Some(Err)` when a defect was not
/// tolerated, `None` when this is not an ack at all.
#[allow(clippy::type_complexity)]
fn ack_id(ctx: &mut Context, rest: &[u8], offset: usize) -> Option<(Result<(), ()>, Option<String>)> {
    let id_len = rest.iter().take_while(|b| b.is_ascii_alphanumeric()).count();
    if !(1..=5).contains(&id_len) {
        return None;
    }
    let after = &rest[id_len..];
    if after.is_empty() {
        return Some((Ok(()), None));
    }
    if after[0] == b'}' {
        let reply = &after[1..];
        if reply.len() <= 5 && reply.iter().all(u8::is_ascii_alphanumeric) {
            return Some((Ok(()), Some(String::from_utf8_lossy(reply).into_owned())));
        }
        return None;
    }
    if after[0] == b'{' && after[1..].iter().all(u8::is_ascii_alphanumeric) && (1..=5).contains(&(after.len() - 1)) {
        let ok = ctx.tolerate(Code::MessageIdOnAck, "an ack or rej carries a message ID of its own (UAP 5.32)", Some(offset + id_len));
        return Some((if ok { Ok(()) } else { Err(()) }, None));
    }
    None
}

fn id_string(rest: &[u8]) -> String {
    let n = rest.iter().take_while(|b| b.is_ascii_alphanumeric()).count();
    String::from_utf8_lossy(&rest[..n]).into_owned()
}

/// `text{MM`, `text{MM}` (reply-ack capable) or `text{MM}AA` (and acking AA).
fn split_message_id(body: &[u8]) -> (&[u8], Option<String>, Option<String>) {
    let Some(brace) = body.iter().rposition(|&b| b == b'{') else {
        return (body, None, None);
    };
    let tail = &body[brace + 1..];
    let (id, reply) = match tail.iter().position(|&b| b == b'}') {
        Some(i) => (&tail[..i], Some(&tail[i + 1..])),
        None => (tail, None),
    };
    let alnum = |b: &[u8]| b.iter().all(u8::is_ascii_alphanumeric);
    if !(1..=5).contains(&id.len()) || !alnum(id) || reply.is_some_and(|r| r.len() > 5 || !alnum(r)) {
        return (body, None, None);
    }
    (&body[..brace], Some(String::from_utf8_lossy(id).into_owned()), reply.map(|r| String::from_utf8_lossy(r).into_owned()))
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

/// `PARM.`, `UNIT.`, `EQNS.` and `BITS.` (APRS12c ch. 13). Anything malformed stays a plain message.
fn telemetry_metadata(ctx: &mut Context, addressee: &str, body: &str, offset: usize) -> Option<Data> {
    let kind = body.as_bytes().get(..5)?;
    let rest = body.get(5..)?;
    let rest_text = || String::from(rest);
    let bad = |ctx: &mut Context, what: &str| {
        ctx.info(Code::InvalidTelemetryMetadata, what, Some(offset));
        None
    };
    match kind {
        b"PARM." | b"UNIT." => {
            let labels: Vec<String> = rest_text().split(',').map(str::to_string).collect();
            if labels.len() > 13 {
                return bad(ctx, "telemetry metadata names at most 13 channels (APRS12c ch. 13)");
            }
            let t = TelemetryLabels { addressee: addressee.to_string(), labels };
            Some(if kind == b"PARM." { Data::TelemetryNames(t) } else { Data::TelemetryUnits(t) })
        }
        b"EQNS." => {
            let coefficients: Vec<String> = rest_text().split(',').map(str::to_string).collect();
            if coefficients.len() > 15 || !coefficients.iter().all(|c| is_decimal(c)) {
                return bad(ctx, "telemetry equations are up to 15 numbers, a, b and c for each analog channel (APRS12c ch. 13)");
            }
            Some(Data::TelemetryCoefficients(TelemetryCoefficients { addressee: addressee.to_string(), coefficients }))
        }
        b"BITS." => {
            let text = rest_text();
            let (bits, project) = text.split_once(',').unwrap_or((&text, ""));
            if bits.len() != 8 || !bits.bytes().all(|b| b == b'0' || b == b'1') {
                return bad(ctx, "telemetry bit sense is eight 0 or 1 characters, then a project title of up to 23 (APRS12c ch. 13)");
            }
            Some(Data::TelemetryBits(TelemetryBits {
                addressee: addressee.to_string(),
                bits: bits.to_string(),
                project: project.to_string(),
            }))
        }
        _ => None,
    }
}

pub(crate) fn is_decimal(t: &str) -> bool {
    let t = t.strip_prefix(['-', '+']).unwrap_or(t);
    let (whole, frac) = t.split_once('.').unwrap_or((t, ""));
    !(whole.is_empty() && frac.is_empty()) && whole.bytes().all(|b| b.is_ascii_digit()) && frac.bytes().all(|b| b.is_ascii_digit())
}

/// `?APRSx` and `?PING?`, optionally followed by the one callsign some types ask about.
fn directed_query(addressee: &str, body: &[u8]) -> Option<Data> {
    let rest = body.strip_prefix(b"?")?;
    let query_type = QUERY_TYPES.iter().find(|q| rest.starts_with(q.as_bytes()))?;
    let target = &rest[query_type.len()..];
    let target = if target.is_empty() {
        None
    } else if target.len() <= 9 && target.iter().all(|b| b.is_ascii_graphic()) {
        Some(String::from_utf8_lossy(target).trim().to_string())
    } else {
        return None;
    };
    Some(Data::DirectedQuery(DirectedQuery { addressee: addressee.to_string(), query_type: query_type.to_string(), target }))
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
            if t.labels.len() > 13 || t.labels.iter().any(|l| l.contains(',') || crate::text::has_line_break(l.as_bytes())) {
                return Err(EncodeError::new("telemetry metadata names at most 13 channels, without commas (APRS12c ch. 13)"));
            }
            addressee(&mut out, &t.addressee)?;
            out.extend_from_slice(if matches!(data, Data::TelemetryNames(_)) { b"PARM." } else { b"UNIT." });
            out.extend_from_slice(t.labels.join(",").as_bytes());
        }
        Data::TelemetryCoefficients(t) => {
            if t.coefficients.len() > 15 || !t.coefficients.iter().all(|c| is_decimal(c)) {
                return Err(EncodeError::new("telemetry equations are up to 15 numbers (APRS12c ch. 13)"));
            }
            addressee(&mut out, &t.addressee)?;
            out.extend_from_slice(b"EQNS.");
            out.extend_from_slice(t.coefficients.join(",").as_bytes());
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
        }
        Data::DirectedQuery(q) => {
            if !QUERY_TYPES.contains(&q.query_type.as_str()) {
                return Err(EncodeError::new(
                    "a directed query is one of ?APRSD, ?APRSH, ?APRSM, ?APRSO, ?APRSP, ?APRSS, ?APRST or ?PING? (APRS12c ch. 15)",
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
