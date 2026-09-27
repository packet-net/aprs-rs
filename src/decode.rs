//! Chooses a decoder by the data type identifier (APRS12c ch. 5).

use crate::context::Context;
use crate::{Address, Code, Data, PathEntry, PositionReport, Positioned, RawWeatherFormat, Timestamp, Unrecognized};
use crate::{comment, message, mic_e, object, other, position, status, telemetry};

/// How far into the information field a `!` may start an obsolete TNC beacon position.
const BEACON_POSITION_LIMIT: usize = 40;

pub(crate) fn information(ctx: &mut Context, source: &Address, destination: &Address, path: &[PathEntry], info: &[u8]) -> Data {
    let _ = source;
    let _ = path;
    let mut end = info.len();
    while end > 0 && matches!(info[end - 1], b'\r' | b'\n') {
        end -= 1;
    }
    if end < info.len()
        && !ctx.tolerate(Code::TrailingLineBreak, "the information field ends with CR or LF (APRS12c ch. 5, UAP 5.13)", Some(end))
    {
        return Data::Unrecognized(Unrecognized::Malformed);
    }
    let info = &info[..end];
    let Some(&dti) = info.first() else {
        return Data::Unrecognized(Unrecognized::Empty);
    };

    let data = match dti {
        b'!' if info.starts_with(b"!!") => other::raw_weather(ctx, info, RawWeatherFormat::UltimeterLogging, 2),
        b'!' | b'=' | b'/' | b'@' => position_report(ctx, info, 0),
        b'`' | b'\'' => mic_e::decode(ctx, destination, info),
        0x1C | 0x1D => {
            // Given at the data type identifier, before anything else is checked (vectors README).
            ctx.info(Code::ObsoleteFormat, "the Mic-E Rev 0 data type identifiers 0x1C and 0x1D are obsolete (APRS12c ch. 10)", Some(0));
            mic_e::decode(ctx, destination, info)
        }
        b';' => object::object(ctx, info),
        b')' => object::item(ctx, info),
        b':' => message::decode(ctx, info),
        b'>' => status::decode(ctx, info),
        b'T' => telemetry::report(ctx, info),
        b'_' => other::positionless_weather(ctx, info),
        b'#' => other::raw_weather(ctx, info, RawWeatherFormat::PeetBrosHash, 1),
        b'*' => other::raw_weather(ctx, info, RawWeatherFormat::PeetBrosStar, 1),
        b'$' if info.starts_with(b"$ULTW") => other::raw_weather(ctx, info, RawWeatherFormat::UltimeterPacket, 5),
        b'$' => other::nmea(ctx, info),
        b'[' => other::maidenhead_beacon(ctx, info),
        b'?' => other::query(ctx, info),
        b'<' => other::capabilities(ctx, info),
        b'}' => other::third_party(ctx, info),
        b'{' => other::user_defined(ctx, info),
        b',' => other::test_data(ctx, info),
        b'%' => other::agrelo(ctx, info),
        b'&' | b'+' | b'.' => {
            ctx.info(Code::ReservedDataType, "a reserved data type identifier with no defined format (APRS12c ch. 5)", Some(0));
            return Data::Unrecognized(Unrecognized::ReservedDataType);
        }
        _ => return not_aprs(ctx, info),
    };
    data.unwrap_or(Data::Unrecognized(Unrecognized::Malformed))
}

/// Not APRS by its first byte. An obsolete TNC beacon may still carry a `!` position further in.
fn not_aprs(ctx: &mut Context, info: &[u8]) -> Data {
    let limit = info.len().min(BEACON_POSITION_LIMIT);
    if let Some(i) = info[..limit].iter().position(|&b| b == b'!') {
        if ctx.options.tolerates(Code::PositionNotAtStart) {
            let mut trial = ctx.clone();
            if let Some(data) = position_report(&mut trial, &info[i..], i) {
                *ctx = trial;
                ctx.warn(Code::PositionNotAtStart, "a ! position found after other text (obsolete TNC beacon rule)", Some(i));
                return data;
            }
        }
    }
    ctx.info(Code::NotAprs, "the first byte is not an APRS data type identifier (APRS12c ch. 20)", Some(0));
    Data::Unrecognized(Unrecognized::NotAprs)
}

/// `!` and `=` (no timestamp), `/` and `@` (timestamp); `=` and `@` mean the sender can message.
pub(crate) fn position_report(ctx: &mut Context, info: &[u8], offset: usize) -> Option<Data> {
    let dti = info[0];
    let messaging = matches!(dti, b'=' | b'@');
    let mut at = 1;
    let mut timestamp = None;
    if matches!(dti, b'/' | b'@') {
        let Some(t) = info.get(1..8).and_then(Timestamp::parse) else {
            return bad_timestamp(ctx, info, offset, messaging);
        };
        if !t.is_valid() && !ctx.tolerate(Code::InvalidTimestamp, "the timestamp is out of range (APRS12c ch. 6)", Some(offset + 1)) {
            return None;
        }
        timestamp = Some(t);
        at = 8;
    }
    let fields = body(ctx, info, at, offset)?;
    Some(Data::Position(PositionReport { timestamp, messaging, fields }))
}

/// A `/` or `@` report whose timestamp is missing or garbled, a tolerated defect (UAP 5.8). A
/// position straight after the DTI (the timestamp is missing) is tried first: a timestamp starts
/// with six digits, so it cannot be mistaken for one. Then seven garbled bytes are skipped. Where
/// the position is is judged on the position itself, under the options in force, not on anything
/// after it (vectors README, "Timestamps that are not there").
fn bad_timestamp(ctx: &mut Context, info: &[u8], offset: usize, messaging: bool) -> Option<Data> {
    for at in [1, 8] {
        let mut probe = Context::new(ctx.options);
        if position::decode(&mut probe, info.get(at..).unwrap_or_default(), offset + at).is_none() {
            continue;
        }
        let why = if at == 1 {
            "the timestamp is missing: the position follows the data type identifier (UAP 5.8)"
        } else {
            "the timestamp is not 6 digits then z, / or h (APRS12c ch. 6); skipped (UAP 5.8)"
        };
        if !ctx.tolerate(Code::MalformedTimestamp, why, Some(offset + 1)) {
            return None;
        }
        let fields = body(ctx, info, at, offset)?;
        return Some(Data::Position(PositionReport { timestamp: None, messaging, fields }));
    }
    ctx.error(Code::MalformedTimestamp, "the timestamp is not timestamp-shaped and no position follows it (UAP 5.8)", Some(offset + 1));
    None
}

/// The position body shared by position reports, objects and items: the position and symbol,
/// then the data extension, weather and comment. `offset` is where `info` starts in the field.
pub(crate) fn body(ctx: &mut Context, info: &[u8], at: usize, offset: usize) -> Option<Positioned> {
    let rest = info.get(at..).unwrap_or_default();
    let decoded = position::decode(ctx, rest, offset + at)?;
    let mut fields =
        Positioned { position: decoded.position, symbol: decoded.symbol, compressed: decoded.compressed, ..Positioned::default() };
    let after = at + decoded.len;
    comment::decode(ctx, &mut fields, decoded.cs, &info[after..], offset + after).then_some(fields)
}
