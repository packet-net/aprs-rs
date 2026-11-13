//! The other data types: positionless and raw weather (APRS12c ch. 12), raw NMEA (ch. 7),
//! Maidenhead beacons, general queries and station capabilities (ch. 15), third-party traffic
//! (ch. 17), user-defined data (ch. 19), test data (ch. 20) and Agrelo DF reports.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::RefusalCode;
use crate::context::Context;
use crate::refusal::Pending;
use crate::weather;
use crate::{
    AgreloDf, Capabilities, Code, Data, EncodeError, Footprint, MaidenheadBeacon, Nmea, Packet, ParseOptions, Query, RawWeather,
    RawWeatherFormat, TestData, Timestamp, UserDefined, Weather, WeatherReport, status, telemetry, text,
};

pub(crate) fn raw_weather(ctx: &mut Context, info: &[u8], format: RawWeatherFormat, skip: usize) -> Option<Data> {
    ctx.info(Code::ObsoleteFormat, "raw weather station output, which the spec keeps only for old stations (APRS12c ch. 12)", Some(0));
    let raw = &info[skip..];
    if !text::is_printable_ascii(raw) {
        ctx.error(Code::InvalidWeather, "raw weather station data is printable ASCII", Some(skip));
        return None;
    }
    let data = String::from_utf8_lossy(raw).into_owned();
    Some(Data::RawWeather(RawWeather { format, data }))
}

/// `_MMDDHHMM` then weather fields, wind first as `c` and `s`.
pub(crate) fn positionless_weather(ctx: &mut Context, info: &[u8]) -> Option<Data> {
    ctx.info(Code::ObsoleteFormat, "a positionless weather report; APRS12c ch. 12 prefers weather with a position", Some(0));
    let Some(timestamp) = info.get(1..9).and_then(Timestamp::parse_mdhm) else {
        ctx.error(Code::InvalidTimestamp, "a positionless weather report starts with an 8-digit MDHM timestamp (APRS12c ch. 12)", Some(1));
        return None;
    };
    if !timestamp.is_valid() && !ctx.tolerate(Code::InvalidTimestamp, "the timestamp is out of range (APRS12c ch. 6)", Some(1)) {
        return None;
    }
    let mut weather = Weather::default();
    let at = 9 + weather::fields(ctx, &info[9..], 9, &mut weather, weather::Wind::Positionless)?.len;
    let rest = &info[at..];
    let mut comment = String::new();
    if !weather::software_and_unit(rest, &mut weather) && !rest.is_empty() {
        if !ctx.tolerate(Code::WeatherComment, "text after the weather data; a weather report has no comment (UAP 2.7.1)", Some(at)) {
            return None;
        }
        comment = text::decode(ctx, rest, at)?;
    }
    Some(Data::Weather(WeatherReport { timestamp: Some(timestamp), weather, comment }))
}

/// Raw NMEA: after `$`, an NMEA 0183 sentence (vectors interpretations.md, "What `$` text is an
/// NMEA sentence"): printable ASCII, an address field of five upper-case letters or digits (or
/// `P` and three or more, a proprietary sentence) and at least one field, with no `$` in it and
/// no `*` except the one that starts a checksum. The checksum, `*` and two hex digits, ends the
/// sentence; any text after it is the comment. The structure is checked before the checksum.
/// GGA, GLL, RMC, VTG and WPL are read, field by field; a field that is missing or does not
/// parse is left out.
pub(crate) fn nmea(ctx: &mut Context, info: &[u8]) -> Option<Data> {
    ctx.info(Code::ObsoleteFormat, "raw NMEA, which APRS12c ch. 7 does not recommend", Some(0));
    let body = &info[1..];
    let Some(sentence) = nmea_sentence(body) else {
        ctx.error(
            Code::InvalidNmea,
            "not an NMEA 0183 sentence: printable ASCII, an address field such as GPRMC, then fields, with * only before a checksum",
            Some(1),
        );
        return None;
    };
    if sentence.checksum.is_some_and(|sum| sum != nmea_checksum(sentence.content)) {
        ctx.error(Code::NmeaChecksumMismatch, "the NMEA checksum does not match, so the sentence is corrupt", Some(1));
        return None;
    }
    let comment_at = 1 + sentence.len;
    let comment = text::decode(ctx, &body[sentence.len..], comment_at)?;
    let mut n = nmea_fields(sentence.content);
    n.sentence = String::from_utf8_lossy(&body[..sentence.len]).into_owned();
    n.has_checksum = sentence.checksum.is_some();
    n.comment = comment;
    Some(Data::Nmea(n))
}

/// The structure of an NMEA sentence at the start of the text after `$`.
struct NmeaSentence<'a> {
    /// The address field and data fields: everything before any `*`.
    content: &'a str,
    /// The checksum sent, if any.
    checksum: Option<u8>,
    /// Bytes the sentence takes, checksum included; any after it are the comment.
    len: usize,
}

/// Reads the sentence structure; `None` when the text is not an NMEA sentence.
fn nmea_sentence(body: &[u8]) -> Option<NmeaSentence<'_>> {
    let star = body.iter().position(|&b| b == b'*');
    let (content, checksum, len) = match star {
        None => (body, None, body.len()),
        Some(i) => {
            // A * that does not start a checksum is a reserved character in a field.
            let hex = body.get(i + 1..i + 3)?;
            let sum = core::str::from_utf8(hex).ok().filter(|h| h.bytes().all(|b| b.is_ascii_hexdigit()))?;
            (&body[..i], Some(u8::from_str_radix(sum, 16).ok()?), i + 3)
        }
    };
    if !text::is_printable_ascii(content) || content.contains(&b'$') {
        return None;
    }
    let content = core::str::from_utf8(content).ok()?;
    let (address, _) = content.split_once(',')?;
    let upper_or_digit = |b: u8| b.is_ascii_uppercase() || b.is_ascii_digit();
    let address_ok = address.bytes().all(upper_or_digit) && (address.len() == 5 || (address.starts_with('P') && address.len() >= 4));
    address_ok.then_some(NmeaSentence { content, checksum, len })
}

/// The XOR of every character between `$` and `*`.
fn nmea_checksum(content: &str) -> u8 {
    content.bytes().fold(0u8, |a, b| a ^ b)
}

/// The fields of a GGA, GLL, RMC, VTG or WPL sentence, by position. Only a five-character address
/// that does not start `P` has a sentence formatter, in its last three characters.
fn nmea_fields(content: &str) -> Nmea {
    let f: Vec<&str> = content.split(',').collect();
    let field = |i: usize| f.get(i).copied();
    let mut n = Nmea::default();
    let position = |n: &mut Nmea, at: usize| {
        let latitude = nmea_coordinate(field(at), field(at + 1), ("N", "S"), 90.0);
        let longitude = nmea_coordinate(field(at + 2), field(at + 3), ("E", "W"), 180.0);
        if let (Some(la), Some(lo)) = (latitude, longitude) {
            (n.latitude, n.longitude) = (Some(la), Some(lo));
        }
    };
    let address = f[0];
    let formatter = if address.len() == 5 && !address.starts_with('P') { &address[2..] } else { "" };
    match formatter {
        "GGA" => {
            n.time = field(1).and_then(nmea_time);
            position(&mut n, 2);
            n.fix_valid = match field(6).map(str::as_bytes) {
                Some([q]) if q.is_ascii_digit() => Some(*q != b'0'),
                _ => None,
            };
            n.altitude_m = field(9).and_then(nmea_number);
        }
        "RMC" => {
            n.time = field(1).and_then(nmea_time);
            n.fix_valid = field(2).and_then(status_letter);
            position(&mut n, 3);
            n.speed_knots = field(7).and_then(nmea_number);
            n.course_degrees = field(8).and_then(nmea_number);
        }
        "GLL" => {
            position(&mut n, 1);
            n.time = field(5).and_then(nmea_time);
            n.fix_valid = field(6).and_then(status_letter);
        }
        "VTG" => {
            n.course_degrees = field(1).and_then(nmea_number);
            n.speed_knots = field(5).and_then(nmea_number);
        }
        "WPL" => {
            position(&mut n, 1);
            n.waypoint = field(5).filter(|w| !w.is_empty()).map(str::to_string);
        }
        _ => {}
    }
    n
}

/// Speed, course or altitude: an optional `-`, then digits with an optional `.` and fraction (the
/// digits before or after the `.` may be left out, not both).
fn nmea_number(t: &str) -> Option<f64> {
    let digits = t.strip_prefix('-').unwrap_or(t);
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    if (whole.is_empty() && fraction.is_empty())
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    t.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// `hhmmss` (hours 00-23, minutes and seconds 00-59) with an optional `.` and fraction, as
/// `HH:MM:SS.f`: the fraction as sent, less trailing zeros.
fn nmea_time(t: &str) -> Option<String> {
    let b = t.as_bytes();
    if b.len() < 6 || !b[..6].iter().all(u8::is_ascii_digit) {
        return None;
    }
    let fraction = &t[6..];
    if !(fraction.is_empty() || (fraction.starts_with('.') && fraction[1..].bytes().all(|b| b.is_ascii_digit()))) {
        return None;
    }
    let pair = |i: usize| u32::from(b[i] - b'0') * 10 + u32::from(b[i + 1] - b'0');
    if pair(0) > 23 || pair(2) > 59 || pair(4) > 59 {
        return None;
    }
    let fraction = fraction.trim_end_matches('0');
    let fraction = if fraction == "." { "" } else { fraction };
    Some(format!("{}:{}:{}{}", &t[0..2], &t[2..4], &t[4..6], fraction))
}

fn status_letter(s: &str) -> Option<bool> {
    match s {
        "A" => Some(true),
        "V" => Some(false),
        _ => None,
    }
}

/// An NMEA coordinate, `ddmm.mm` or `dddmm.mm`: digits with an optional `.` and fraction, at least
/// three before the `.`; the last two of those are minutes (below 60) and the rest degrees,
/// however many digits they have. Within `limit` degrees, with the hemisphere letter exact.
fn nmea_coordinate(value: Option<&str>, hemisphere: Option<&str>, (positive, negative): (&str, &str), limit: f64) -> Option<f64> {
    let (value, hemisphere) = (value?, hemisphere?);
    let sign = match hemisphere {
        h if h == positive => 1.0,
        h if h == negative => -1.0,
        _ => return None,
    };
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    if whole.len() < 3 || !whole.bytes().all(|b| b.is_ascii_digit()) || !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (degrees, minutes) = whole.split_at(whole.len() - 2);
    let degrees: f64 = degrees.parse().ok()?;
    let minutes: f64 = if fraction.is_empty() { minutes.parse().ok()? } else { format!("{minutes}.{fraction}").parse().ok()? };
    if minutes >= 60.0 {
        return None;
    }
    let v = degrees + minutes / 60.0;
    (v <= limit).then_some(sign * v)
}

/// `[IO91SX]` and a comment.
pub(crate) fn maidenhead_beacon(ctx: &mut Context, info: &[u8]) -> Option<Data> {
    ctx.info(Code::ObsoleteFormat, "a Maidenhead locator beacon, which APRS12c ch. 7 marks obsolete", Some(0));
    let Some(end) = info.iter().position(|&b| b == b']') else {
        ctx.error(Code::InvalidLocator, "a locator beacon is [locator]", Some(1));
        return None;
    };
    let locator = &info[1..end];
    if !status::is_locator(locator) {
        ctx.error(Code::InvalidLocator, "the locator is not 4 or 6 characters, e.g. IO91 or IO91SX", Some(1));
        return None;
    }
    let comment = text::decode(ctx, &info[end + 1..], end + 1)?;
    Some(Data::MaidenheadBeacon(MaidenheadBeacon { locator: String::from_utf8_lossy(locator).to_ascii_uppercase(), comment }))
}

/// `?TYPE?`, optionally with a footprint: ` lat,lon,rrrr` (a space for a positive value).
pub(crate) fn query(ctx: &mut Context, info: &[u8]) -> Option<Data> {
    let Some(end) = info.iter().skip(1).position(|&b| b == b'?').map(|i| i + 1) else {
        ctx.error(Code::InvalidGeneralQuery, "a general query is ?TYPE? (APRS12c ch. 15)", Some(0));
        return None;
    };
    let query_type = &info[1..end];
    if query_type.is_empty() || !query_type.iter().all(u8::is_ascii_uppercase) {
        ctx.error(Code::InvalidGeneralQuery, "the query type is upper-case letters (APRS12c ch. 15)", Some(1));
        return None;
    }
    let rest = &info[end + 1..];
    let footprint = if rest.is_empty() {
        None
    } else {
        match footprint(rest) {
            Some(f) => Some(f),
            None => {
                ctx.error(
                    Code::InvalidGeneralQuery,
                    "a query footprint is a latitude (to 90) and longitude (to 180) in degrees and a 4-digit radius (APRS12c ch. 15)",
                    Some(end + 1),
                );
                return None;
            }
        }
    };
    Some(Data::Query(Query { query_type: String::from_utf8_lossy(query_type).into_owned(), footprint }))
}

/// The latitude and longitude as sent, and the radius.
fn footprint(rest: &[u8]) -> Option<Footprint> {
    let text = core::str::from_utf8(rest).ok()?;
    let parts: Vec<&str> = text.split(',').collect();
    if parts.len() != 3 {
        return None;
    }
    let latitude = footprint_degrees(parts[0], 90.0)?;
    let longitude = footprint_degrees(parts[1], 180.0)?;
    let radius = parts[2];
    if radius.len() != 4 || !radius.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(Footprint {
        latitude,
        longitude,
        radius_miles: radius.parse::<u32>().ok()?.into(),
        latitude_text: parts[0].to_string(),
        longitude_text: parts[1].to_string(),
    })
}

/// A footprint's degrees: a decimal number, with a minus sign or, only for a positive value, a
/// leading space ("Note the leading space in the latitude, as its value is positive", APRS12c ch.
/// 15), within `limit` either way.
pub(crate) fn footprint_degrees(t: &str, limit: f64) -> Option<f64> {
    let (t, digits) = match t.strip_prefix(' ') {
        Some(positive) => (positive, positive),
        None => (t, t.strip_prefix('-').unwrap_or(t)),
    };
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    if (whole.is_empty() && fraction.is_empty())
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    t.parse::<f64>().ok().filter(|v| v.abs() <= limit)
}

/// A footprint's latitude or longitude as it is written: the text it was sent as, when that is a
/// number as APRS12c ch. 15 writes one and still says `value`, and otherwise the number as the
/// shortest decimal that reads back the same, after a space unless it is negative (0 included).
fn footprint_text(value: f64, text: &str, limit: f64, what: &str) -> Result<String, EncodeError> {
    if let Some(sent) = footprint_degrees(text, limit) {
        if sent == value {
            return Ok(text.to_string());
        }
    }
    if value.is_nan() || value.abs() > limit {
        return Err(EncodeError::new(
            RefusalCode::for_value(value),
            format!("a query footprint's {what} is decimal degrees within {limit} (a leading space only before a positive value)"),
        ));
    }
    let digits = crate::steps::number_text(value);
    Ok(if value < 0.0 { digits } else { format!(" {digits}") })
}

/// `TOKEN,TOKEN=VALUE,...`.
pub(crate) fn capabilities(ctx: &mut Context, info: &[u8]) -> Option<Data> {
    let body = text::decode(ctx, &info[1..], 1)?;
    // TOKEN or TOKEN=VALUE items, separated by commas and split at the first '='. The spaces
    // (U+0020 only) around an item, a token or a value are padding, not part of them.
    let pad = |t: &str| t.trim_matches(' ').to_string();
    let items: Vec<(String, Option<String>)> = body
        .split(',')
        .map(|item| item.trim_matches(' '))
        .filter(|item| !item.is_empty())
        .map(|item| match item.split_once('=') {
            Some((t, v)) => (pad(t), Some(pad(v))),
            None => (item.to_string(), None),
        })
        .collect();
    if items.is_empty() {
        ctx.error(Code::InvalidCapabilities, "a capabilities report lists at least one capability (APRS12c ch. 15)", Some(1));
        return None;
    }
    // An empty token, or one with a space or a control character in it, or a value with a
    // control character, is free text: a beacon sent with the wrong data type identifier.
    let free_text = items.iter().any(|(t, v)| {
        t.is_empty() || t.chars().any(|c| c <= ' ' || c == '\x7F') || v.as_ref().is_some_and(|v| v.chars().any(|c| c < ' ' || c == '\x7F'))
    });
    if free_text
        && !ctx.tolerate(
            Code::FreeTextCapabilities,
            "a capabilities packet holds free text rather than TOKEN / TOKEN=VALUE items (APRS12c ch. 15)",
            Some(1),
        )
    {
        return None;
    }
    Some(Data::Capabilities(Capabilities { capabilities: items }))
}

/// How many third-party headers a decoder reads, one inside another.
const MAX_THIRD_PARTY_DEPTH: usize = 4;

/// `}` and a whole TNC2 packet, decoded with the same options. Its source may be any 1-9
/// printable ASCII characters other than `>` and `:` (APRS12c ch. 17); a defect its header may
/// tolerate is a warning on the inner packet, and rejects it when not tolerated.
pub(crate) fn third_party(ctx: &mut Context, info: &[u8]) -> Option<Data> {
    if ctx.depth >= MAX_THIRD_PARTY_DEPTH {
        ctx.error(Code::InvalidThirdParty, "third-party packets nest at most four deep", Some(1));
        return None;
    }
    match Packet::decode_third_party(&info[1..], ctx.options, ctx.depth + 1) {
        Ok(inner) => Some(Data::ThirdParty(Box::new(inner))),
        Err(_) => {
            ctx.error(Code::InvalidThirdParty, "the third-party header is not SOURCE>DEST,PATH: (APRS12c ch. 17)", Some(1));
            None
        }
    }
}

pub(crate) fn user_defined(ctx: &mut Context, info: &[u8]) -> Option<Data> {
    if info.len() < 3 {
        ctx.error(Code::InvalidUserDefined, "user-defined data needs { then a user ID and a packet type (APRS12c ch. 19)", Some(0));
        return None;
    }
    Some(Data::UserDefined(UserDefined { user_id: info[1] as char, packet_type: info[2] as char, data: info[3..].to_vec() }))
}

pub(crate) fn test_data(ctx: &mut Context, info: &[u8]) -> Option<Data> {
    Some(Data::Test(TestData { data: text::decode(ctx, &info[1..], 1)? }))
}

/// `%bbb/q`, exactly: a bearing of 000 to 360 and a quality digit (APRS12c Appendix 1).
pub(crate) fn agrelo(ctx: &mut Context, info: &[u8]) -> Option<Data> {
    if info.len() != 6 || !text::all_digits(&info[1..4]) || info[4] != b'/' || !info[5].is_ascii_digit() || text::digits(&info[1..4]) > 360
    {
        ctx.error(Code::InvalidAgreloDf, "an Agrelo DF report is exactly %bbb/q, with a bearing of 000 to 360", Some(0));
        return None;
    }
    Some(Data::AgreloDf(AgreloDf { bearing_degrees: text::digits(&info[1..4]) as u16, quality: info[5] - b'0' }))
}

// ------------------------------------------------------------------ encoding

pub(crate) fn encode(data: &Data) -> Result<Vec<u8>, EncodeError> {
    let printable = |t: &str, what: &str| -> Result<(), EncodeError> {
        if crate::text::has_line_break(t.as_bytes()) {
            return Err(EncodeError::new(RefusalCode::InvalidCharacter, format!("{what} cannot contain a line break")));
        }
        Ok(())
    };
    let mut out = Vec::new();
    match data {
        Data::Status(s) => return status::encode(s),
        Data::Telemetry(t) => return telemetry::encode(t),
        Data::Weather(w) => {
            let mut p = Pending::default();
            let t = match w.timestamp.filter(|t| matches!(t, Timestamp::MonthDayHourMinute { .. })) {
                Some(t) => {
                    p.run(crate::encode::check_timestamp(&t));
                    Some(t)
                }
                None => {
                    p.note(EncodeError::new(
                        RefusalCode::InvalidTimestamp,
                        "a positionless weather report has an MDHM timestamp (APRS12c ch. 12)",
                    ));
                    None
                }
            };
            if !w.comment.is_empty() {
                p.note(EncodeError::new(RefusalCode::FieldNotAllowed, "a weather report has no comment field (APRS12c ch. 12)"));
            }
            let direction =
                w.weather.wind_direction_degrees.and_then(|d| p.run(crate::encode::whole_degrees(d, "wind direction"))).map(f64::from);
            out.push(b'_');
            if let Some(t) = t {
                out.extend_from_slice(&t.to_bytes());
            }
            p.run(weather::field(&mut out, b'c', direction, 3, false));
            p.run(weather::field(&mut out, b's', w.weather.wind_speed_mph, 3, false));
            weather::write_fields(&mut out, &w.weather, &mut p);
            p.finish()?;
            // The software type and unit, and the extra fields, must not read back as fields.
            match positionless_weather(&mut Context::new(ParseOptions::LENIENT), &out) {
                Some(Data::Weather(back)) if weather::text_parts(&back.weather) == weather::text_parts(&w.weather) => {}
                _ => {
                    return Err(EncodeError::new(
                        RefusalCode::TextReadsAsField,
                        "the weather does not read back as given: a software type and unit, or an extra field, that would read as weather fields",
                    ));
                }
            }
        }
        Data::RawWeather(r) => {
            if !text::is_printable_ascii(r.data.as_bytes()) {
                return Err(EncodeError::new(RefusalCode::InvalidCharacter, "raw weather station data is printable ASCII"));
            }
            out.extend_from_slice(match r.format {
                RawWeatherFormat::PeetBrosHash => b"#",
                RawWeatherFormat::PeetBrosStar => b"*",
                RawWeatherFormat::UltimeterPacket => b"$ULTW",
                RawWeatherFormat::UltimeterLogging => b"!!",
            });
            out.extend_from_slice(r.data.as_bytes());
        }
        Data::Nmea(n) => return encode_nmea(n),
        Data::MaidenheadBeacon(m) => {
            let mut p = Pending::default();
            if !status::is_locator(m.locator.as_bytes()) || m.locator != m.locator.to_ascii_uppercase() {
                p.note(EncodeError::new(RefusalCode::Malformed, "a grid locator is 4 or 6 upper-case characters, e.g. IO91 or IO91SX"));
            }
            p.run(printable(&m.comment, "a beacon comment"));
            p.finish()?;
            out.push(b'[');
            out.extend_from_slice(m.locator.as_bytes());
            out.push(b']');
            out.extend_from_slice(m.comment.as_bytes());
        }
        Data::Query(q) => {
            let mut p = Pending::default();
            if q.query_type.is_empty() || !q.query_type.bytes().all(|b| b.is_ascii_uppercase()) {
                p.note(EncodeError::new(RefusalCode::InvalidCode, "a query type is upper-case letters (APRS12c ch. 15)"));
            }
            out.push(b'?');
            out.extend_from_slice(q.query_type.as_bytes());
            out.push(b'?');
            if let Some(f) = &q.footprint {
                // The numbers are written as they were sent when they still say the value, as
                // telemetry values are (vectors README, "Numbers as sent").
                let radius = crate::steps::round(f.radius_miles);
                if !(0.0..=9999.0).contains(&radius) {
                    p.note(EncodeError::new(
                        RefusalCode::for_value(f.radius_miles),
                        "a query footprint's radius is 0-9999 miles once rounded",
                    ));
                }
                let latitude = p.run(footprint_text(f.latitude, &f.latitude_text, 90.0, "latitude")).unwrap_or_default();
                let longitude = p.run(footprint_text(f.longitude, &f.longitude_text, 180.0, "longitude")).unwrap_or_default();
                out.extend_from_slice(format!("{latitude},{longitude},{:04}", radius as u32).as_bytes());
            }
            p.finish()?;
        }
        Data::Capabilities(c) => {
            if c.capabilities.is_empty() {
                return Err(EncodeError::new(RefusalCode::MissingField, "a capabilities report lists at least one token"));
            }
            // What would not read back the same: a token is fixed-form, a value is free text whose
            // spaces around it would be taken for padding.
            let mut p = Pending::default();
            for (t, v) in &c.capabilities {
                if t.is_empty() || t.chars().any(|c| c <= ' ' || c == '\x7F' || c == ',' || c == '=') {
                    p.note(EncodeError::new(
                        RefusalCode::Malformed,
                        "a capability token is not empty and has no spaces, ',', '=' or control characters",
                    ));
                }
                if let Some(v) = v {
                    if v.chars().any(|c| c < ' ' || c == '\x7F' || c == ',') {
                        p.note(EncodeError::new(RefusalCode::InvalidCharacter, "a capability value has no ',' or control characters"));
                    }
                    if v.starts_with(' ') || v.ends_with(' ') {
                        p.note(EncodeError::new(RefusalCode::PaddingSpace, "a capability value does not start or end with a space"));
                    }
                }
            }
            p.finish()?;
            out.push(b'<');
            let items: Vec<String> = c
                .capabilities
                .iter()
                .map(|(t, v)| match v {
                    Some(v) => format!("{t}={v}"),
                    None => t.clone(),
                })
                .collect();
            out.extend_from_slice(items.join(",").as_bytes());
        }
        Data::ThirdParty(p) => {
            third_party_header(p)?;
            out.push(b'}');
            out.extend_from_slice(&p.to_tnc2());
            // The information field is written as received; only a line break at its very end
            // would not read back.
            if matches!(out.last(), Some(b'\r' | b'\n')) {
                return Err(EncodeError::new(
                    RefusalCode::InvalidCharacter,
                    "a third-party packet's information field cannot end with a line break",
                ));
            }
        }
        Data::UserDefined(u) => {
            // APRS12c ch. 19 puts no restriction on user-defined data, these two characters
            // included: any byte is written back as it came.
            let (Ok(user_id), Ok(packet_type)) = (u8::try_from(u.user_id), u8::try_from(u.packet_type)) else {
                return Err(EncodeError::new(
                    RefusalCode::ValueOutOfRange,
                    "the user ID and packet type are one byte each (U+0000 to U+00FF)",
                ));
            };
            out.push(b'{');
            out.push(user_id);
            out.push(packet_type);
            out.extend_from_slice(&u.data);
            // A line break at the very end would be taken for the end of the line and dropped.
            if matches!(out.last(), Some(b'\r' | b'\n')) {
                return Err(EncodeError::new(
                    RefusalCode::InvalidCharacter,
                    "user-defined data cannot end with a line break, which would not read back",
                ));
            }
        }
        Data::Test(t) => {
            printable(&t.data, "test data")?;
            out.push(b',');
            out.extend_from_slice(t.data.as_bytes());
        }
        Data::AgreloDf(a) => {
            if a.bearing_degrees > 360 || a.quality > 9 {
                return Err(EncodeError::new(RefusalCode::ValueOutOfRange, "an Agrelo bearing is 0-360 and the quality 0-9"));
            }
            out.extend_from_slice(format!("%{:03}/{}", a.bearing_degrees, a.quality).as_bytes());
        }
        Data::Unrecognized(_) => return Err(EncodeError::new(RefusalCode::NotEncodable, "undecoded data has nothing to encode")),
        _ => return Err(EncodeError::new(RefusalCode::NotEncodable, "not one of the other data types")),
    }
    Ok(out)
}

/// `$`, the sentence and its comment, when they read back as given: the sentence well formed,
/// its checksum (if any) right and ending it, a comment only after a checksum, and every field
/// given agreeing with what the sentence says.
fn encode_nmea(n: &Nmea) -> Result<Vec<u8>, EncodeError> {
    let mut p = Pending::default();
    let sentence = n.sentence.as_bytes();
    if text::has_line_break(n.comment.as_bytes()) {
        p.note(EncodeError::new(RefusalCode::InvalidCharacter, "an NMEA comment cannot contain a line break"));
    }
    // $ULTW is raw weather, not NMEA, so such a sentence would not read back.
    match nmea_sentence(sentence).filter(|s| s.len == sentence.len() && !n.sentence.starts_with("ULTW")) {
        None => p.note(EncodeError::new(
            RefusalCode::InvalidNmea,
            "not an NMEA 0183 sentence: printable ASCII, an address field such as GPRMC, then fields, and at most a *hh checksum at the end",
        )),
        Some(structure) => {
            match structure.checksum {
                Some(sum) if sum != nmea_checksum(structure.content) => {
                    p.note(EncodeError::new(RefusalCode::InvalidNmea, "the NMEA checksum does not match the sentence"));
                }
                Some(_) if !n.has_checksum => {
                    p.note(EncodeError::new(RefusalCode::InvalidNmea, "the sentence has a *hh checksum but has_checksum is not set"));
                }
                None if n.has_checksum => {
                    p.note(EncodeError::new(RefusalCode::InvalidNmea, "has_checksum is set but the sentence has no *hh checksum"));
                }
                None if !n.comment.is_empty() => {
                    p.note(EncodeError::new(
                        RefusalCode::InvalidNmea,
                        "an NMEA comment goes after a *hh checksum, which is what ends the sentence",
                    ));
                }
                _ => {}
            }
            let read = nmea_fields(structure.content);
            fn agrees<T: PartialEq>(given: &Option<T>, read: &Option<T>) -> bool {
                given.is_none() || given == read
            }
            // Degrees reached by other arithmetic differ in the last bits, so numbers agree within a
            // part in 10^9 (absolute below 1), as the vectors compare them.
            fn near(given: &Option<f64>, read: &Option<f64>) -> bool {
                match (given, read) {
                    (None, _) => true,
                    (Some(g), Some(r)) => (g - r).abs() <= 1e-9 * g.abs().max(r.abs()).max(1.0),
                    (Some(_), None) => false,
                }
            }
            if !(near(&n.latitude, &read.latitude)
                && near(&n.longitude, &read.longitude)
                && agrees(&n.fix_valid, &read.fix_valid)
                && near(&n.course_degrees, &read.course_degrees)
                && near(&n.speed_knots, &read.speed_knots)
                && near(&n.altitude_m, &read.altitude_m)
                && agrees(&n.time, &read.time)
                && agrees(&n.waypoint, &read.waypoint))
            {
                p.note(EncodeError::new(RefusalCode::InvalidNmea, "a field given does not match what the NMEA sentence says"));
            }
        }
    }
    p.finish()?;
    let mut out = Vec::with_capacity(1 + sentence.len() + n.comment.len());
    out.push(b'$');
    out.extend_from_slice(sentence);
    out.extend_from_slice(n.comment.as_bytes());
    Ok(out)
}

/// Checks that a third-party packet's header can be written so that it reads back the same: the
/// source 1-9 printable ASCII characters other than `>` and `:` (APRS12c ch. 17), the destination
/// and path APRS-IS addresses, the used entries a run from the start (the TNC2 form marks only
/// the last), and no header defect that the inner decode tolerated: that is part of its data, and
/// no clean form reproduces it.
fn third_party_header(p: &Packet) -> Result<(), EncodeError> {
    if !crate::Address::is_third_party_source(p.source.as_str()) {
        return Err(EncodeError::new(
            RefusalCode::InvalidThirdPartyHeader,
            "a third-party source is 1-9 printable ASCII characters other than > and : (APRS12c ch. 17)",
        ));
    }
    if !crate::Address::is_valid(p.destination.as_str()) || p.path.iter().any(|e| !crate::Address::is_valid(e.address.as_str())) {
        return Err(EncodeError::new(
            RefusalCode::InvalidThirdPartyHeader,
            "a third-party destination and path entries are addresses: 1-9 letters, digits or -",
        ));
    }
    if let Some(last) = p.path.iter().rposition(|e| e.used) {
        if p.path[..last].iter().any(|e| !e.used) {
            return Err(EncodeError::new(
                RefusalCode::InvalidThirdPartyHeader,
                "a used path entry after one that is not used cannot be written in TNC2 form",
            ));
        }
    }
    let header_defect = |c: Code| {
        matches!(
            c,
            Code::EmptyDestination
                | Code::EmptyPathEntry
                | Code::MultipleUsedMarkers
                | Code::NulPaddedAddress
                | Code::InvalidAx25AddressCharacters
        )
    };
    if p.diagnostics.iter().any(|d| header_defect(d.code)) {
        return Err(EncodeError::new(
            RefusalCode::InvalidThirdPartyHeader,
            "the third-party packet's header has a defect that was tolerated, which is part of its data and cannot be written back",
        ));
    }
    Ok(())
}
