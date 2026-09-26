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
        b'`' | b'\'' | 0x1C | 0x1D => mic_e::decode(ctx, destination, info),
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
        let (t, next) = position_timestamp(ctx, info, offset)?;
        timestamp = t;
        at = next;
    }

    let decoded = position::decode(ctx, &info[at..], offset + at)?;
    let mut fields =
        Positioned { position: decoded.position, symbol: decoded.symbol, compressed: decoded.compressed, ..Positioned::default() };
    at += decoded.len;
    if !comment::decode(ctx, &mut fields, decoded.cs, &info[at..], offset + at) {
        return None;
    }
    Some(Data::Position(PositionReport { timestamp, messaging, fields }))
}

/// The timestamp of a `/` or `@` report, and where the position starts. A timestamp that is not
/// timestamp-shaped is a tolerated defect: the position is then read straight after the DTI if
/// one is there (the timestamp is missing), otherwise after the seven bytes (it is garbled).
fn position_timestamp(ctx: &mut Context, info: &[u8], offset: usize) -> Option<(Option<Timestamp>, usize)> {
    if let Some(t) = info.get(1..8).and_then(Timestamp::parse) {
        if !t.is_valid() && !ctx.tolerate(Code::InvalidTimestamp, "the timestamp is out of range (APRS12c ch. 6)", Some(offset + 1)) {
            return None;
        }
        return Some((Some(t), 8));
    }
    if !ctx.tolerate(Code::MalformedTimestamp, "the timestamp is missing or not timestamp-shaped (UAP 5.8)", Some(offset + 1)) {
        return None;
    }
    match timestamp_reading(ctx, info, 1, false) {
        Reading::Missing => Some((None, 1)),
        Reading::Garbled => Some((None, 8.min(info.len()))),
        Reading::Neither => {
            // Neither reading gives a position: the tolerance cannot help, so it is an error.
            ctx.diagnostics.pop();
            ctx.error(
                Code::MalformedTimestamp,
                "the timestamp is not timestamp-shaped and no position follows it (UAP 5.8)",
                Some(offset + 1),
            );
            None
        }
    }
}

pub(crate) enum Reading {
    /// The position starts where the timestamp should.
    Missing,
    /// Seven bytes of garbage, then the position.
    Garbled,
    /// No position either way.
    Neither,
}

/// After a timestamp slot that holds no timestamp: whether the timestamp is missing (the position
/// starts at `at`) or garbled (it starts seven bytes later). An uncompressed position at `at`
/// settles it (for an object, an uncompressed latitude is enough); otherwise a position seven
/// bytes on wins, since garbage often happens to read as a compressed position.
pub(crate) fn timestamp_reading(ctx: &Context, info: &[u8], at: usize, latitude_is_enough: bool) -> Reading {
    let valid_at = |i: usize| {
        let mut trial = ctx.clone();
        info.get(i..).is_some_and(|rest| position::decode(&mut trial, rest, i).is_some())
    };
    let here = if latitude_is_enough { info.get(at..).is_some_and(position::starts_with_latitude) } else { valid_at(at) };
    if info.get(at).is_some_and(u8::is_ascii_digit) && here {
        Reading::Missing
    } else if valid_at(at + 7) {
        Reading::Garbled
    } else if valid_at(at) {
        Reading::Missing
    } else {
        Reading::Neither
    }
}
