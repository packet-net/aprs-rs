//! Objects and items (APRS12c ch. 11).

use alloc::string::String;

use crate::context::Context;
use crate::{Code, Data, ItemReport, ObjectReport, Positioned, Timestamp, comment, position, text};

/// `;` + name (9, space-padded) + `*` alive or `_` killed + timestamp + position.
pub(crate) fn object(ctx: &mut Context, info: &[u8]) -> Option<Data> {
    let marker = |b: u8| b == b'*' || b == b'_';
    let name_end = if info.len() > 10 && marker(info[10]) {
        10
    } else {
        let Some(i) = info.iter().take(10).skip(1).position(|&b| marker(b)).map(|i| i + 1) else {
            ctx.error(Code::InvalidObjectName, "no * or _ after the object name", Some(1));
            return None;
        };
        if !ctx.tolerate(Code::ObjectNameNotPadded, "the object name is not padded to 9 characters", Some(1)) {
            return None;
        }
        i
    };
    let name_bytes = trim_end_spaces(&info[1..name_end]);
    if name_bytes.is_empty() || !text::is_printable_ascii(name_bytes) {
        ctx.error(Code::InvalidObjectName, "the object name is empty or not printable ASCII", Some(1));
        return None;
    }
    let name = String::from(core::str::from_utf8(name_bytes).unwrap_or_default());
    let killed = info[name_end] == b'_';

    let mut at = name_end + 1;
    let mut timestamp = None;
    match info.get(at..at + 7).and_then(Timestamp::parse) {
        Some(t) => {
            if !t.is_valid()
                && !t.is_permanent_object_marker()
                && !ctx.tolerate(Code::InvalidTimestamp, "the timestamp is out of range (APRS12c ch. 6)", Some(at))
            {
                return None;
            }
            timestamp = Some(t);
            at += 7;
        }
        None => {
            // No timestamp at all (a position straight after the name) or a garbled one; when
            // neither reading gives a position, the object is taken to have none.
            if !matches!(crate::decode::timestamp_reading(ctx, info, at, true), crate::decode::Reading::Garbled) {
                if !ctx.tolerate(Code::ObjectWithoutTimestamp, "an object report has no timestamp (APRS12c ch. 11)", Some(at)) {
                    return None;
                }
            } else {
                if !ctx.tolerate(Code::MalformedTimestamp, "the timestamp is not timestamp-shaped (UAP 5.8)", Some(at)) {
                    return None;
                }
                at += 7;
            }
        }
    }

    let fields = positioned(ctx, info, at)?;
    Some(Data::Object(ObjectReport { name, killed, timestamp, fields }))
}

/// `)` + name (3-9) + `!` alive or `_` killed + position.
pub(crate) fn item(ctx: &mut Context, info: &[u8]) -> Option<Data> {
    let Some(end) = info.iter().enumerate().skip(1).take(10).find(|(_, b)| **b == b'!' || **b == b'_').map(|(i, _)| i) else {
        ctx.error(Code::InvalidItemName, "the item name is not 3-9 characters followed by ! or _ (APRS12c ch. 11)", Some(1));
        return None;
    };
    let name_bytes = &info[1..end];
    if !(3..=9).contains(&name_bytes.len()) || !text::is_printable_ascii(name_bytes) {
        ctx.error(Code::InvalidItemName, "the item name is not 3-9 printable characters followed by ! or _", Some(1));
        return None;
    }
    let name = String::from(core::str::from_utf8(name_bytes).unwrap_or_default());
    let killed = info[end] == b'_';
    let fields = positioned(ctx, info, end + 1)?;
    Some(Data::Item(ItemReport { name, killed, fields }))
}

fn positioned(ctx: &mut Context, info: &[u8], at: usize) -> Option<Positioned> {
    let rest = info.get(at..).unwrap_or_default();
    let decoded = position::decode(ctx, rest, at)?;
    let mut fields =
        Positioned { position: decoded.position, symbol: decoded.symbol, compressed: decoded.compressed, ..Positioned::default() };
    let after = at + decoded.len;
    comment::decode(ctx, &mut fields, decoded.cs, &info[after..], after).then_some(fields)
}

fn trim_end_spaces(b: &[u8]) -> &[u8] {
    let mut end = b.len();
    while end > 0 && b[end - 1] == b' ' {
        end -= 1;
    }
    &b[..end]
}
