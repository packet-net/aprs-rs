//! Objects and items (APRS12c ch. 11).

use alloc::string::String;

use crate::context::Context;
use crate::{Code, Data, ItemReport, ObjectReport, Timestamp, decode, position, text};

/// `;` + name (9, space-padded) + `*` alive or `_` killed + timestamp + position.
pub(crate) fn object(ctx: &mut Context, info: &[u8]) -> Option<Data> {
    if info.len() < 11 {
        ctx.error(Code::Truncated, "an object report is shorter than ; and a 9-character name and * or _", Some(0));
        return None;
    }
    let marker = |b: u8| b == b'*' || b == b'_';
    let name_end = if marker(info[10]) {
        10
    } else {
        // A name that was not padded to 9 characters: the marker comes early.
        let Some(i) = info[1..10].iter().position(|&b| marker(b)) else {
            ctx.error(Code::InvalidObjectName, "no * or _ after the 9-character object name", Some(10));
            return None;
        };
        if !ctx.tolerate(Code::ObjectNameNotPadded, "the object name is not padded to 9 characters", Some(1)) {
            return None;
        }
        i + 1
    };
    if !text::is_printable_ascii(&info[1..name_end]) {
        ctx.error(Code::InvalidObjectName, "the object name is not printable ASCII", Some(1));
        return None;
    }
    let name_bytes = trim_end_spaces(&info[1..name_end]);
    if name_bytes.is_empty() {
        ctx.error(Code::InvalidObjectName, "the object name is empty", Some(1));
        return None;
    }
    let name = String::from(core::str::from_utf8(name_bytes).unwrap_or_default());
    let killed = info[name_end] == b'_';

    let mut at = name_end + 1;
    let mut timestamp = None;
    if let Some(t) = info.get(at..at + 7).and_then(Timestamp::parse) {
        if !t.is_valid() && !ctx.tolerate(Code::InvalidTimestamp, "the timestamp is out of range (APRS12c ch. 6)", Some(at)) {
            return None;
        }
        timestamp = Some(t);
        at += 7;
    } else if garbled_timestamp(ctx, info, at) {
        // Timestamp-shaped (six digits, or a z, / or h suffix) but not a timestamp, then a
        // position: garbled, not missing. Read straight after the marker instead, those seven
        // bytes would decode as a compressed position.
        if !ctx.tolerate(Code::MalformedTimestamp, "the timestamp is not 6 digits then z, / or h (APRS12c ch. 6); skipped", Some(at)) {
            return None;
        }
        at += 7;
    } else if !ctx.tolerate(Code::ObjectWithoutTimestamp, "an object report has no timestamp (APRS12c ch. 11)", Some(at)) {
        return None;
    }

    let fields = decode::body(ctx, info, at, 0)?;
    Some(Data::Object(ObjectReport { name, killed, timestamp, fields }))
}

/// Seven timestamp-shaped bytes with a position after them, judged on the position itself
/// (latitude, table, longitude and symbol, or the 13 compressed bytes) under the options in
/// force: a defect later in the report does not change what these bytes are.
fn garbled_timestamp(ctx: &Context, info: &[u8], at: usize) -> bool {
    info.len() > at + 7
        && (text::all_digits(&info[at..at + 6]) || matches!(info[at + 6], b'z' | b'/' | b'h'))
        && position::decode(&mut Context::new(ctx.options), &info[at + 7..], at + 7).is_some()
}

/// `)` + name (3-9) + `!` alive or `_` killed + position.
pub(crate) fn item(ctx: &mut Context, info: &[u8]) -> Option<Data> {
    // The marker ends a name of at least 3 characters, so it is looked for from the fourth byte on.
    let Some(end) = (4..info.len().min(11)).find(|&i| info[i] == b'!' || info[i] == b'_') else {
        ctx.error(Code::InvalidItemName, "no ! or _ after a 3-9 character item name (APRS12c ch. 11)", Some(1));
        return None;
    };
    let name_bytes = &info[1..end];
    if !text::is_printable_ascii(name_bytes) {
        ctx.error(Code::InvalidItemName, "the item name is not printable ASCII", Some(1));
        return None;
    }
    let name = String::from(core::str::from_utf8(name_bytes).unwrap_or_default());
    let killed = info[end] == b'_';
    let fields = decode::body(ctx, info, end + 1, 0)?;
    Some(Data::Item(ItemReport { name, killed, fields }))
}

fn trim_end_spaces(b: &[u8]) -> &[u8] {
    let mut end = b.len();
    while end > 0 && b[end - 1] == b' ' {
        end -= 1;
    }
    &b[..end]
}
