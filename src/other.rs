//! The other data types: positionless and raw weather (APRS12c ch. 12), raw NMEA (ch. 7),
//! Maidenhead beacons, general queries and station capabilities (ch. 15), third-party traffic
//! (ch. 17), user-defined data (ch. 19), test data (ch. 20) and Agrelo DF reports.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::context::Context;
use crate::weather::{self, Start};
use crate::{
    AgreloDf, Capabilities, Code, Data, EncodeError, Footprint, MaidenheadBeacon, Nmea, Packet, Query, RawWeather, RawWeatherFormat,
    TestData, Timestamp, UserDefined, Weather, WeatherReport, status, telemetry, text,
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
    let run = weather::fields(ctx, &info[9..], 9, &mut weather, Start::Positionless)?;
    let at = 9 + run.len;
    let complete = run.has_wind && run.has_wind_speed && run.has_gust && run.has_temperature;
    if !complete && !ctx.tolerate(Code::IncompleteWeather, "the weather report lacks a mandatory field (APRS12c ch. 12)", Some(9)) {
        return None;
    }
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

/// Raw NMEA: `$GPRMC`, `$GPGGA`, `$GPGLL`, `$GPVTG`, `$GPWPL` are read; other sentences are kept as text.
pub(crate) fn nmea(ctx: &mut Context, info: &[u8]) -> Option<Data> {
    ctx.info(Code::ObsoleteFormat, "raw NMEA, which APRS12c ch. 7 does not recommend", Some(0));
    let Some(sentence) = core::str::from_utf8(&info[1..]).ok().filter(|s| s.bytes().all(|b| (0x20..=0x7E).contains(&b))) else {
        ctx.error(Code::InvalidNmea, "an NMEA sentence is printable ASCII", Some(1));
        return None;
    };
    let (body, has_checksum) = match checksum(sentence) {
        Checksum::None => (sentence, false),
        Checksum::Matches(body) => (body, true),
        Checksum::Mismatch => {
            ctx.error(Code::NmeaChecksumMismatch, "the NMEA checksum does not match, so the sentence is corrupt", Some(1));
            return None;
        }
    };
    let f: Vec<&str> = body.split(',').collect();
    let kind = f[0];
    if kind.len() != 5 || !kind.bytes().all(|b| b.is_ascii_uppercase()) {
        ctx.error(Code::InvalidNmea, "an NMEA sentence starts with a 5-letter talker and sentence type", Some(1));
        return None;
    }
    let mut n = Nmea { sentence: sentence.to_string(), has_checksum, ..Nmea::default() };
    let get = |i: usize| f.get(i).copied().unwrap_or("");
    let number = |t: &str| t.parse::<f64>().ok();
    let ok = match &kind[2..] {
        "RMC" => {
            n.time = time(get(1));
            n.fix_valid = status_letter(get(2));
            n.latitude = coordinate(get(3), get(4), 2);
            n.longitude = coordinate(get(5), get(6), 3);
            n.speed_knots = number(get(7));
            n.course_degrees = number(get(8));
            n.latitude.is_some() && n.longitude.is_some()
        }
        "GGA" => {
            n.time = time(get(1));
            n.latitude = coordinate(get(2), get(3), 2);
            n.longitude = coordinate(get(4), get(5), 3);
            n.fix_valid = get(6).parse::<u8>().ok().map(|q| q != 0);
            n.altitude_m = number(get(9));
            n.latitude.is_some() && n.longitude.is_some()
        }
        "GLL" => {
            n.latitude = coordinate(get(1), get(2), 2);
            n.longitude = coordinate(get(3), get(4), 3);
            n.time = time(get(5));
            n.fix_valid = status_letter(get(6));
            n.latitude.is_some() && n.longitude.is_some()
        }
        "VTG" => {
            n.course_degrees = number(get(1));
            n.speed_knots = number(get(5));
            true
        }
        "WPL" => {
            n.latitude = coordinate(get(1), get(2), 2);
            n.longitude = coordinate(get(3), get(4), 3);
            n.waypoint = Some(get(5).to_string()).filter(|w| !w.is_empty());
            n.latitude.is_some() && n.longitude.is_some()
        }
        _ => true,
    };
    if !ok {
        ctx.error(Code::InvalidNmea, "the NMEA sentence's position is malformed", Some(1));
        return None;
    }
    Some(Data::Nmea(n))
}

enum Checksum<'a> {
    None,
    Matches(&'a str),
    Mismatch,
}

fn checksum(sentence: &str) -> Checksum<'_> {
    let Some(star) = sentence.rfind('*') else {
        return Checksum::None;
    };
    let hex = &sentence[star + 1..];
    if hex.len() != 2 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Checksum::None;
    }
    let body = &sentence[..star];
    let sum = body.bytes().fold(0u8, |a, b| a ^ b);
    if u8::from_str_radix(hex, 16) == Ok(sum) { Checksum::Matches(body) } else { Checksum::Mismatch }
}

fn time(t: &str) -> Option<String> {
    let b = t.as_bytes();
    if b.len() < 6 || !b[..6].iter().all(u8::is_ascii_digit) {
        return None;
    }
    let fraction = t[6..].trim_end_matches('0');
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

fn coordinate(value: &str, hemisphere: &str, degree_digits: usize) -> Option<f64> {
    if value.len() <= degree_digits || !value.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return None;
    }
    let degrees: f64 = value[..degree_digits].parse().ok()?;
    let minutes: f64 = value[degree_digits..].parse().ok()?;
    let v = degrees + minutes / 60.0;
    match hemisphere {
        "N" | "E" => Some(v),
        "S" | "W" => Some(-v),
        _ => None,
    }
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
    if query_type.is_empty() || !query_type.iter().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()) {
        ctx.error(Code::InvalidGeneralQuery, "the query type is upper-case letters and digits", Some(1));
        return None;
    }
    let rest = &info[end + 1..];
    let footprint = if rest.is_empty() {
        None
    } else {
        match footprint(rest) {
            Some(f) => Some(f),
            None => {
                ctx.error(Code::InvalidGeneralQuery, "a query footprint is lat,lon,radius (APRS12c ch. 15)", Some(end + 1));
                return None;
            }
        }
    };
    Some(Data::Query(Query { query_type: String::from_utf8_lossy(query_type).into_owned(), footprint }))
}

fn footprint(rest: &[u8]) -> Option<Footprint> {
    let text = core::str::from_utf8(rest).ok()?;
    let parts: Vec<&str> = text.split(',').collect();
    if parts.len() != 3 {
        return None;
    }
    let signed = |t: &str| -> Option<f64> {
        let t = t.strip_prefix(' ').unwrap_or(t);
        if t.is_empty() || !t.bytes().all(|b| b.is_ascii_digit() || b == b'.' || b == b'-') {
            return None;
        }
        t.parse().ok()
    };
    let radius = parts[2];
    if radius.len() != 4 || !radius.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(Footprint { latitude: signed(parts[0])?, longitude: signed(parts[1])?, radius_miles: radius.parse().ok()? })
}

/// `TOKEN,TOKEN=VALUE,...`.
pub(crate) fn capabilities(ctx: &mut Context, info: &[u8]) -> Option<Data> {
    let body = text::decode(ctx, &info[1..], 1)?;
    if body.is_empty() {
        ctx.error(Code::InvalidCapabilities, "a capabilities report lists at least one token (APRS12c ch. 15)", Some(1));
        return None;
    }
    let items: Vec<(String, Option<String>)> = body
        .split(',')
        .map(|item| match item.split_once('=') {
            Some((t, v)) => (t.to_string(), Some(v.to_string())),
            None => (item.to_string(), None),
        })
        .collect();
    if items.iter().any(|(t, _)| t.is_empty() || t.contains(' ')) {
        if !ctx.tolerate(
            Code::FreeTextCapabilities,
            "a capabilities packet holds free text rather than TOKEN / TOKEN=VALUE items (APRS12c ch. 15)",
            Some(1),
        ) {
            return None;
        }
        return Some(Data::Capabilities(Capabilities { capabilities: alloc::vec![(String::from(body.trim_start()), None)] }));
    }
    Some(Data::Capabilities(Capabilities { capabilities: items }))
}

/// `}` and a whole TNC2 packet, decoded with the same options.
pub(crate) fn third_party(ctx: &mut Context, info: &[u8]) -> Option<Data> {
    match Packet::decode_tnc2(&info[1..], ctx.options) {
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

/// `%bbb/q`.
pub(crate) fn agrelo(ctx: &mut Context, info: &[u8]) -> Option<Data> {
    if info.len() < 6 || !text::all_digits(&info[1..4]) || info[4] != b'/' || !info[5].is_ascii_digit() {
        ctx.error(Code::InvalidAgreloDf, "an Agrelo DF report is %bbb/q", Some(0));
        return None;
    }
    Some(Data::AgreloDf(AgreloDf { bearing_degrees: text::digits(&info[1..4]) as u16, quality: info[5] - b'0' }))
}

// ------------------------------------------------------------------ encoding

pub(crate) fn encode(data: &Data) -> Result<Vec<u8>, EncodeError> {
    let printable = |t: &str, what: &str| -> Result<(), EncodeError> {
        if crate::text::has_line_break(t.as_bytes()) {
            return Err(EncodeError::new(format!("{what} cannot contain a line break")));
        }
        Ok(())
    };
    let mut out = Vec::new();
    match data {
        Data::Status(s) => return status::encode(s),
        Data::Telemetry(t) => return telemetry::encode(t),
        Data::Weather(w) => {
            let Some(t) = w.timestamp.filter(|t| matches!(t, Timestamp::MonthDayHourMinute { .. })) else {
                return Err(EncodeError::new("a positionless weather report has an MDHM timestamp (APRS12c ch. 12)"));
            };
            crate::encode::check_timestamp(&t)?;
            if !w.comment.is_empty() {
                return Err(EncodeError::new("a weather report has no comment field (APRS12c ch. 12)"));
            }
            out.push(b'_');
            out.extend_from_slice(&t.to_bytes());
            weather::field(&mut out, b'c', w.weather.wind_direction_degrees.map(f64::from), 3, false)?;
            weather::field(&mut out, b's', w.weather.wind_speed_mph, 3, false)?;
            weather::encode_fields(&mut out, &w.weather)?;
        }
        Data::RawWeather(r) => {
            printable(&r.data, "raw weather data")?;
            out.extend_from_slice(match r.format {
                RawWeatherFormat::PeetBrosHash => b"#",
                RawWeatherFormat::PeetBrosStar => b"*",
                RawWeatherFormat::UltimeterPacket => b"$ULTW",
                RawWeatherFormat::UltimeterLogging => b"!!",
            });
            out.extend_from_slice(r.data.as_bytes());
        }
        Data::Nmea(n) => {
            if !n.sentence.bytes().all(|b| (0x20..=0x7E).contains(&b)) {
                return Err(EncodeError::new("an NMEA sentence is printable ASCII"));
            }
            match checksum(&n.sentence) {
                Checksum::Mismatch => return Err(EncodeError::new("the NMEA checksum does not match the sentence")),
                Checksum::None if n.has_checksum => {
                    return Err(EncodeError::new("has_checksum is set but the sentence has no *hh checksum"));
                }
                _ => {}
            }
            out.push(b'$');
            out.extend_from_slice(n.sentence.as_bytes());
        }
        Data::MaidenheadBeacon(m) => {
            if !status::is_locator(m.locator.as_bytes()) {
                return Err(EncodeError::new("a grid locator is 4 or 6 characters, e.g. IO91 or IO91SX"));
            }
            printable(&m.comment, "a beacon comment")?;
            out.push(b'[');
            out.extend_from_slice(m.locator.to_ascii_uppercase().as_bytes());
            out.push(b']');
            out.extend_from_slice(m.comment.as_bytes());
        }
        Data::Query(q) => {
            if q.query_type.is_empty() || !q.query_type.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()) {
                return Err(EncodeError::new("a query type is upper-case letters and digits"));
            }
            out.push(b'?');
            out.extend_from_slice(q.query_type.as_bytes());
            out.push(b'?');
            if let Some(f) = q.footprint {
                if !(-90.0..=90.0).contains(&f.latitude) || !(-180.0..=180.0).contains(&f.longitude) || f.radius_miles > 9999 {
                    return Err(EncodeError::new("a query footprint is within -90..90, -180..180 and a radius of up to 9999 miles"));
                }
                let signed = |v: f64| if v < 0.0 { format!("{v}") } else { format!(" {v}") };
                out.extend_from_slice(format!("{},{},{:04}", signed(f.latitude), signed(f.longitude), f.radius_miles).as_bytes());
            }
        }
        Data::Capabilities(c) => {
            if c.capabilities.is_empty() {
                return Err(EncodeError::new("a capabilities report lists at least one token"));
            }
            let bad = |t: &str| t.is_empty() || t.contains([',', '=', ' ']) || crate::text::has_line_break(t.as_bytes());
            if c.capabilities
                .iter()
                .any(|(t, v)| bad(t) || v.as_ref().is_some_and(|v| v.contains(',') || crate::text::has_line_break(v.as_bytes())))
            {
                return Err(EncodeError::new(
                    "capability tokens and values are text without ',' or line breaks (and tokens without '=' or spaces)",
                ));
            }
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
            out.push(b'}');
            out.extend_from_slice(&p.to_tnc2());
        }
        Data::UserDefined(u) => {
            // A space is printable ASCII too: APRS12c ch. 19 puts no limit on these two characters.
            if !(' '..='~').contains(&u.user_id) || !(' '..='~').contains(&u.packet_type) {
                return Err(EncodeError::new("user ID and packet type are printable ASCII"));
            }
            out.push(b'{');
            out.push(u.user_id as u8);
            out.push(u.packet_type as u8);
            out.extend_from_slice(&u.data);
        }
        Data::Test(t) => {
            printable(&t.data, "test data")?;
            out.push(b',');
            out.extend_from_slice(t.data.as_bytes());
        }
        Data::AgreloDf(a) => {
            if a.bearing_degrees > 360 || a.quality > 9 {
                return Err(EncodeError::new("an Agrelo bearing is 0-360 and the quality 0-9"));
            }
            out.extend_from_slice(format!("%{:03}/{}", a.bearing_degrees, a.quality).as_bytes());
        }
        Data::Unrecognized(_) => return Err(EncodeError::new("undecoded data has nothing to encode")),
        _ => return Err(EncodeError::new("not one of the other data types")),
    }
    Ok(out)
}
