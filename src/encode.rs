//! Encoding data back into an information field. The encoder writes only what APRS12c allows,
//! and checks free text by decoding what it wrote: a comment that would read back as something
//! else (an altitude, a data extension, a `!DAO!`) is refused rather than silently changed.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::comment::{self, KNOTS_TO_MPH, beacon_rate_char};
use crate::context::Context;
use crate::position::{self, type_to_bits};
use crate::weather::{self, push_digits};
use crate::{
    CompressionOrigin, CompressionType, DaoPrecision, Data, EncodeError, GpsFix, ItemReport, NmeaSource, ObjectReport, ParseOptions,
    PositionReport, Positioned, Timestamp, Tone,
};

impl Data {
    /// The information field for this data. Fails, naming the field, when the data cannot be
    /// written as APRS12c allows. Mic-E data needs its destination address too: use
    /// [`crate::Packet::create_mic_e`].
    pub fn encode(&self) -> Result<Vec<u8>, EncodeError> {
        let mut out = Vec::new();
        match self {
            Data::Position(r) => position_report(&mut out, r)?,
            Data::Object(o) => object(&mut out, o)?,
            Data::Item(i) => item(&mut out, i)?,
            Data::MicE(m) => return crate::mic_e::encode(m).map(|(_, info)| info),
            Data::Message(_)
            | Data::Ack(_)
            | Data::Reject(_)
            | Data::Bulletin(_)
            | Data::NwsBulletin(_)
            | Data::TelemetryNames(_)
            | Data::TelemetryUnits(_)
            | Data::TelemetryCoefficients(_)
            | Data::TelemetryBits(_)
            | Data::DirectedQuery(_) => return crate::message::encode(self),
            other => return crate::other::encode(other),
        }
        Ok(out)
    }
}

fn position_report(out: &mut Vec<u8>, r: &PositionReport) -> Result<(), EncodeError> {
    out.push(match (r.timestamp.is_some(), r.messaging) {
        (false, false) => b'!',
        (false, true) => b'=',
        (true, false) => b'/',
        (true, true) => b'@',
    });
    if let Some(t) = &r.timestamp {
        dhm_or_hms(out, t)?;
    }
    positioned(out, &r.fields)
}

fn object(out: &mut Vec<u8>, o: &ObjectReport) -> Result<(), EncodeError> {
    let name = o.name.as_bytes();
    if name.is_empty() || name.len() > 9 || !crate::text::is_printable_ascii(name) {
        return Err(EncodeError::new("an object name is 1-9 printable ASCII characters (APRS12c ch. 11)"));
    }
    if name.ends_with(b" ") {
        return Err(EncodeError::new("an object name cannot end in a space, which would be read back as padding"));
    }
    out.push(b';');
    out.extend_from_slice(name);
    out.extend(core::iter::repeat_n(b' ', 9 - name.len()));
    out.push(if o.killed { b'_' } else { b'*' });
    let Some(t) = &o.timestamp else {
        return Err(EncodeError::new("an object report must have a timestamp (APRS12c ch. 11)"));
    };
    dhm_or_hms(out, t)?;
    positioned(out, &o.fields)
}

fn item(out: &mut Vec<u8>, i: &ItemReport) -> Result<(), EncodeError> {
    let name = i.name.as_bytes();
    if !(3..=9).contains(&name.len()) || !crate::text::is_printable_ascii(name) || name.contains(&b'!') || name.contains(&b'_') {
        return Err(EncodeError::new("an item name is 3-9 printable ASCII characters other than ! and _ (APRS12c ch. 11)"));
    }
    out.push(b')');
    out.extend_from_slice(name);
    out.push(if i.killed { b'_' } else { b'!' });
    positioned(out, &i.fields)
}

pub(crate) fn dhm_or_hms(out: &mut Vec<u8>, t: &Timestamp) -> Result<(), EncodeError> {
    if matches!(t, Timestamp::MonthDayHourMinute { .. }) {
        return Err(EncodeError::new("this timestamp is DDHHMMz, DDHHMM/ or HHMMSSh; MDHM is only for positionless weather"));
    }
    check_timestamp(t)?;
    out.extend_from_slice(&t.to_bytes());
    Ok(())
}

pub(crate) fn check_timestamp(t: &Timestamp) -> Result<(), EncodeError> {
    if t.is_valid() || t.is_permanent_object_marker() {
        Ok(())
    } else {
        Err(EncodeError::new(format!("timestamp {} has a field out of range", String::from_utf8_lossy(&t.to_bytes()))))
    }
}

/// The position, symbol, extension, weather and comment of a position, object or item.
pub(crate) fn positioned(out: &mut Vec<u8>, f: &Positioned) -> Result<(), EncodeError> {
    let weather_symbol = comment::is_weather_symbol(f);
    if weather_symbol && f.weather.is_none() {
        // A report with the weather station symbol reads back as weather, whatever it holds.
        return Err(EncodeError::new("a report with the weather station symbol, _, is a weather report: give it weather (APRS12c ch. 12)"));
    }
    if f.weather.is_some() {
        if !weather_symbol {
            return Err(EncodeError::new("weather needs the weather station symbol, _ (APRS12c ch. 12)"));
        }
        if !f.comment.is_empty() {
            return Err(EncodeError::new("a weather report has no comment field (APRS12c ch. 12, UAP 2.7.1)"));
        }
        if f.telemetry.is_some() || f.frequency.is_some() || f.signpost.is_some() || (f.altitude_feet.is_some() && !f.compressed) {
            return Err(EncodeError::new(
                "a weather report has no comment, so it cannot carry telemetry, frequency, signpost or /A= altitude (APRS12c ch. 12)",
            ));
        }
        // Its data extension, or its cs bytes, are the wind.
        if f.course_degrees.is_some()
            || f.speed_knots.is_some()
            || f.phg.is_some()
            || f.dfs.is_some()
            || f.area.is_some()
            || f.df_bearing.is_some()
            || f.storm.is_some()
            || (f.range_miles.is_some() && !f.compressed)
        {
            return Err(EncodeError::new(
                "a weather report's data extension is its wind, so it cannot carry a course, speed, PHG, RNG, DFS, area, DF bearing or storm data (APRS12c ch. 12)",
            ));
        }
    }

    let dao_mode = match f.dao {
        Some(d) if d.precision != DaoPrecision::None && !f.compressed => Some(d.precision == DaoPrecision::Base91),
        _ => None,
    };
    let start = out.len();
    let mut dao_digits = None;
    let mut altitude_in_cs = false;
    let mut extension = false;

    if f.compressed {
        if f.phg.is_some() || f.dfs.is_some() || f.area.is_some() || f.df_bearing.is_some() || f.storm.is_some() {
            return Err(EncodeError::new(
                "a compressed position cannot carry a 7-byte data extension (PHG, DFS, area, DF bearing, storm) (APRS12c ch. 9)",
            ));
        }
        position::encode_compressed(out, &f.position, f.symbol)?;
        altitude_in_cs = compressed_cs(out, f)?;
        if f.weather.is_some() && f.altitude_feet.is_some() && !altitude_in_cs {
            return Err(EncodeError::new(
                "a weather report has no comment, so an altitude the cs bytes cannot carry exactly has nowhere to go",
            ));
        }
    } else {
        let count = [
            f.course_degrees.is_some() || f.speed_knots.is_some(),
            f.phg.is_some(),
            f.range_miles.is_some(),
            f.dfs.is_some(),
            f.area.is_some(),
        ]
        .iter()
        .filter(|x| **x)
        .count();
        if count > 1 {
            return Err(EncodeError::new(
                "only one data extension (course/speed, PHG, RNG, DFS or area object) fits in a report (APRS12c ch. 7)",
            ));
        }
        dao_digits = position::encode_uncompressed(out, &f.position, f.symbol, dao_mode)?;
        extension = uncompressed_extension(out, f)?;
    }

    if let Some(w) = &f.weather {
        weather::encode_fields(out, w)?;
    }

    let tail_start = out.len();
    if f.weather.is_none() {
        if let Some(feet) = f.altitude_feet.filter(|_| !altitude_in_cs) {
            let n = libm::round(feet);
            if !(-99_999.0..=999_999.0).contains(&n) {
                return Err(EncodeError::new("/A= altitude must fit in 6 digits (or - and 5 digits)"));
            }
            if n < 0.0 {
                out.extend_from_slice(b"/A=-");
                push_digits(out, (-n) as u32, 5);
            } else {
                out.extend_from_slice(b"/A=");
                push_digits(out, n as u32, 6);
            }
        }
        if let Some(fr) = &f.frequency {
            if extension && out.len() == tail_start {
                out.push(b'/');
            }
            frequency(out, fr)?;
        }
        if let Some(sign) = &f.signpost {
            if !(f.symbol.table == '\\' && f.symbol.code == 'm')
                || sign.is_empty()
                || sign.len() > 3
                || sign.contains(['{', '}'])
                || !crate::text::is_printable_ascii(sign.as_bytes())
            {
                return Err(EncodeError::new("signpost text is 1-3 characters on the signpost symbol, \\m"));
            }
            out.push(b'{');
            out.extend_from_slice(sign.as_bytes());
            out.push(b'}');
        }
    }

    let text = f.comment.as_bytes();
    if crate::text::has_line_break(text) {
        return Err(EncodeError::new("comment text cannot contain a line break"));
    }
    let text_at = out.len();
    let mut trailer = Vec::new();
    if let Some(t) = &f.telemetry {
        telemetry_into(&mut trailer, t)?;
    }
    if let Some(d) = f.dao {
        if f.position.ambiguity > 0 && d.precision != DaoPrecision::None {
            return Err(EncodeError::new("a !DAO! adds precision to an ambiguous position, which contradicts it"));
        }
        check_dao_datum(d)?;
        trailer.push(b'!');
        match (d.precision, dao_digits) {
            (DaoPrecision::Thousandths, Some((lat, lon))) => trailer.extend_from_slice(&[d.datum as u8, b'0' + lat, b'0' + lon]),
            (DaoPrecision::Base91, Some((lat, lon))) => {
                trailer.extend_from_slice(&[d.datum.to_ascii_lowercase() as u8, lat + 33, lon + 33])
            }
            // A compressed position does not use the digits, but writing them keeps the precision.
            (DaoPrecision::Base91, None) => trailer.extend_from_slice(&[d.datum.to_ascii_lowercase() as u8, b'!', b'!']),
            (DaoPrecision::Thousandths, None) => trailer.extend_from_slice(&[d.datum as u8, b'0', b'0']),
            _ => trailer.extend_from_slice(&[d.datum as u8, b' ', b' ']),
        }
        trailer.push(b'!');
    }

    // Write the text as it is (after a frequency, a space first); if it does not read back
    // unchanged, try it without the space, then after a '/' delimiter.
    let separators: &[&[u8]] = if f.frequency.is_some() && !text.is_empty() { &[b" ", b"", b" /"] } else { &[b"", b"/"] };
    for separator in separators {
        out.truncate(text_at);
        if f.weather.is_some() && !separator.is_empty() {
            break;
        }
        out.extend_from_slice(separator);
        out.extend_from_slice(text);
        out.extend_from_slice(&trailer);
        if reads_back(&out[start..], f) {
            return Ok(());
        }
    }
    if f.weather.is_some() {
        return Err(EncodeError::new(
            "the weather does not read back as given: a software type and unit, or an extra field, that would read as weather fields",
        ));
    }
    Err(EncodeError::new(
        "comment text contains something that decodes as a structured element (altitude, !DAO!, |telemetry|, frequency, PHG/RNG/DFS or braces); set the property instead",
    ))
}

/// Writes the compressed cs and type bytes. `true` when they carry the altitude exactly.
///
/// The cs bytes carry one thing: a GGA altitude, a range, or a course and speed (the wind, for a
/// weather station). Whatever else would have to go in them is refused, not dropped, and so is a
/// compression type with nothing to carry, since blank cs bytes have no type byte (vectors
/// interpretations.md, "Re-encoding into compressed bytes rounds").
fn compressed_cs(out: &mut Vec<u8>, f: &Positioned) -> Result<bool, EncodeError> {
    let wind = f.weather.as_ref().filter(|w| w.wind_direction_degrees.is_some() || w.wind_speed_mph.is_some());
    let course_speed = f.course_degrees.is_some() || f.speed_knots.is_some();
    let t =
        f.compression.unwrap_or(CompressionType { fix: GpsFix::Current, source: NmeaSource::Other, origin: CompressionOrigin::Software });
    let log = |x: f64, base: f64| libm::log(x) / libm::log(base);
    let (c, s, altitude) = if t.source == NmeaSource::Gga {
        // GGA cs bytes are read as an altitude, whatever else was meant.
        let Some(feet) = f.altitude_feet else {
            return Err(EncodeError::new("a GGA compression type carries an altitude in the cs bytes, and there is none"));
        };
        if course_speed || f.range_miles.is_some() || wind.is_some() {
            return Err(EncodeError::new(
                "the cs bytes carry the GGA altitude, so a compressed position cannot also carry a course, speed, range or wind",
            ));
        }
        if feet.is_nan() {
            return Err(EncodeError::new("the altitude is not a number"));
        }
        // 1.002^cs feet cannot be 1 foot or less: the nearest is cs 0, and /A= carries the rest.
        let cs = if feet <= 1.0 { 0 } else { libm::round(log(feet, 1.002)) as i64 };
        if cs > 90 * 91 + 90 {
            return Err(EncodeError::new("the altitude is too high for a compressed position"));
        }
        // The cs bytes carry altitude only to 0.2%; one they cannot carry exactly is also written
        // as /A=, which the decoder prefers.
        let exact = (libm::pow(1.002, cs as f64) - feet).abs() <= 1e-9 * feet.abs();
        ((cs / 91) as u8, (cs % 91) as u8, exact)
    } else if let Some(w) = wind {
        // A weather station's cs course and speed are its wind.
        if f.range_miles.is_some() {
            return Err(EncodeError::new("the cs bytes carry the wind or a range, not both"));
        }
        // The cs bytes carry a direction and a speed together; one without the other would read
        // back as 0.
        let (Some(dir), Some(mph)) = (w.wind_direction_degrees, w.wind_speed_mph) else {
            return Err(EncodeError::new("the cs bytes carry the wind as a direction and a speed together, so both are needed"));
        };
        if dir > 360 {
            return Err(EncodeError::new("wind direction must be 0-360 degrees"));
        }
        let knots = mph / KNOTS_TO_MPH;
        (direction_code(dir), speed_code(knots)?, false)
    } else if let Some(range) = f.range_miles {
        if course_speed {
            return Err(EncodeError::new("the cs bytes carry a course and speed or a range, not both (APRS12c ch. 9)"));
        }
        if range.is_nan() || range < 2.0 {
            return Err(EncodeError::new("a compressed range must be at least 2 miles"));
        }
        let s = libm::round(log(range / 2.0, 1.08));
        if s > 90.0 {
            return Err(EncodeError::new("the range is too far for a compressed position"));
        }
        (b'{' - 33, s as u8, false)
    } else if course_speed {
        let course = f.course_degrees.unwrap_or(0);
        if course > 360 {
            return Err(EncodeError::new("course must be 0-360 degrees"));
        }
        (direction_code(course), speed_code(f.speed_knots.unwrap_or(0.0))?, false)
    } else {
        // No course/speed, range or altitude: the spec's own form for that, which has no type byte.
        if f.compression.is_some() {
            return Err(EncodeError::new(
                "a compression type needs something in the cs bytes: a course and speed, range, altitude or wind",
            ));
        }
        out.extend_from_slice(b" sT");
        return Ok(false);
    };
    out.push(c + 33);
    out.push(s + 33);
    out.push(type_to_bits(t) + 33);
    Ok(altitude)
}

/// A course or wind direction in the compressed c byte's 4-degree steps: the nearest step (103
/// degrees is written as 104), with 360 and 0 both north.
fn direction_code(degrees: u16) -> u8 {
    ((u32::from(degrees) + 2) / 4 % 90) as u8
}

fn speed_code(knots: f64) -> Result<u8, EncodeError> {
    if knots < 0.0 {
        return Err(EncodeError::new("speed cannot be negative"));
    }
    let s = libm::round(libm::log(knots + 1.0) / libm::log(1.08));
    if s > 90.0 {
        return Err(EncodeError::new("the speed is too high for a compressed position"));
    }
    Ok(s as u8)
}

fn three(out: &mut Vec<u8>, v: Option<u32>, max: u32, what: &str) -> Result<(), EncodeError> {
    match v {
        None => out.extend_from_slice(b"..."),
        Some(v) if v <= max => push_digits(out, v, 3),
        Some(_) => return Err(EncodeError::new(format!("{what} does not fit in its three digits"))),
    }
    Ok(())
}

/// Writes the 7-byte data extension of an uncompressed position; `true` if there is one.
fn uncompressed_extension(out: &mut Vec<u8>, f: &Positioned) -> Result<bool, EncodeError> {
    if let Some(w) = f.weather.as_ref() {
        three(out, w.wind_direction_degrees.map(u32::from), 360, "wind direction")?;
        out.push(b'/');
        three(out, whole(w.wind_speed_mph, "wind speed")?, 999, "wind speed")?;
        return Ok(true);
    }
    if f.course_degrees.is_some() || f.speed_knots.is_some() || f.df_bearing.is_some() || f.storm.is_some() {
        three(out, f.course_degrees.map(u32::from), 360, "course")?;
        out.push(b'/');
        three(out, whole(f.speed_knots, "speed")?, 999, "speed")?;
        if let Some(b) = f.df_bearing {
            if !(f.symbol.table == '/' && f.symbol.code == '\\') {
                return Err(EncodeError::new("a DF bearing needs the DF symbol, /\\"));
            }
            if b.bearing_degrees > 360 || b.number > 9 || b.range > 9 || b.quality > 9 {
                return Err(EncodeError::new("DF bearing is 0-360 and N, R, Q are single digits"));
            }
            out.push(b'/');
            push_digits(out, u32::from(b.bearing_degrees), 3);
            out.push(b'/');
            out.extend_from_slice(&[b'0' + b.number, b'0' + b.range, b'0' + b.quality]);
        }
        if let Some(s) = f.storm {
            out.push(b'/');
            out.extend_from_slice(match s.kind {
                crate::StormKind::TropicalStorm => b"TS",
                crate::StormKind::Hurricane => b"HC",
                crate::StormKind::TropicalDepression => b"TD",
            });
            for (marker, value, width) in [
                (b'/', s.sustained_wind_knots, 3),
                (b'^', s.gust_knots, 3),
                (b'/', s.central_pressure_mbar, 4),
                (b'>', s.hurricane_radius_nm, 3),
                (b'&', s.tropical_storm_radius_nm, 3),
            ] {
                out.push(marker);
                storm_value(out, value, width)?;
            }
            if let Some(g) = s.whole_gale_radius_nm {
                out.push(b'%');
                storm_value(out, Some(g), 3)?;
            }
        }
        return Ok(true);
    }
    if let Some(p) = f.phg {
        phg_codes(out, p)?;
        if let Some(rate) = p.beacons_per_hour {
            let Some(c) = beacon_rate_char(rate) else {
                return Err(EncodeError::new("PHGR beacons per hour is 1-35"));
            };
            out.push(c);
            out.push(b'/');
        }
        return Ok(true);
    }
    if let Some(r) = f.range_miles {
        let n = libm::round(r);
        if !(0.0..=9999.0).contains(&n) {
            return Err(EncodeError::new("RNG range is 0-9999 miles"));
        }
        out.extend_from_slice(b"RNG");
        push_digits(out, n as u32, 4);
        return Ok(true);
    }
    if let Some(d) = f.dfs {
        dfs_codes(out, d)?;
        return Ok(true);
    }
    if let Some(a) = f.area {
        if !(f.symbol.table == '\\' && f.symbol.code == 'l') {
            return Err(EncodeError::new("an area object needs the area symbol, \\l"));
        }
        if a.lat_offset > 99 || a.lon_offset > 99 {
            return Err(EncodeError::new("area offsets are two digits"));
        }
        out.push(b'0' + a.shape as u8);
        push_digits(out, u32::from(a.lat_offset), 2);
        let color = comment::AREA_COLORS.iter().position(|c| *c == a.color).unwrap_or(0) as u32;
        if color < 10 {
            out.push(b'/');
            out.push(b'0' + color as u8);
        } else {
            push_digits(out, color, 2);
        }
        push_digits(out, u32::from(a.lon_offset), 2);
        if let Some(w) = a.corridor_width_miles {
            if w > 999 {
                return Err(EncodeError::new("an area corridor width is at most 999 miles"));
            }
            out.extend_from_slice(format!("{{{w}}}").as_bytes());
        }
        return Ok(true);
    }
    Ok(false)
}

/// The highest PHG or DFS height code: `~`, the last printable character after `0` (APRS12c ch.
/// 7: "any ASCII character 0-9 and above").
const MAX_HEIGHT_CODE: u8 = b'~' - b'0';

/// `PHGphgd` (without the PHGR rate): the power, gain and directivity codes are digits, and the
/// height code runs on past 9 through the ASCII table (`:` is 10), for balloons and aircraft.
pub(crate) fn phg_codes(out: &mut Vec<u8>, p: crate::Phg) -> Result<(), EncodeError> {
    if p.power > 9 || p.gain > 9 || p.directivity > 9 || p.height > MAX_HEIGHT_CODE {
        return Err(EncodeError::new("PHG power, gain and directivity codes are single digits, and the height code 0-78"));
    }
    out.extend_from_slice(b"PHG");
    out.extend_from_slice(&[b'0' + p.power, b'0' + p.height, b'0' + p.gain, b'0' + p.directivity]);
    Ok(())
}

/// `DFSshgd`: the height code as for PHG, the others digits.
pub(crate) fn dfs_codes(out: &mut Vec<u8>, d: crate::DfSignalStrength) -> Result<(), EncodeError> {
    if d.strength > 9 || d.gain > 9 || d.directivity > 9 || d.height > MAX_HEIGHT_CODE {
        return Err(EncodeError::new("DFS strength, gain and directivity codes are single digits, and the height code 0-78"));
    }
    out.extend_from_slice(b"DFS");
    out.extend_from_slice(&[b'0' + d.strength, b'0' + d.height, b'0' + d.gain, b'0' + d.directivity]);
    Ok(())
}

/// A `!DAO!` datum the decoder reads back: an upper-case letter (`W` for WGS84), or a digit (a
/// local datum), which has no case to say how to read added digits and so carries none.
pub(crate) fn check_dao_datum(d: crate::Dao) -> Result<(), EncodeError> {
    if d.datum.is_ascii_uppercase() || (d.datum.is_ascii_digit() && d.precision == DaoPrecision::None) {
        Ok(())
    } else {
        Err(EncodeError::new("the !DAO! datum is an upper-case letter (W for WGS84), or a digit with no added precision"))
    }
}

fn whole(v: Option<f64>, what: &str) -> Result<Option<u32>, EncodeError> {
    match v {
        None => Ok(None),
        Some(x) if x >= 0.0 => Ok(Some(libm::round(x) as u32)),
        Some(_) => Err(EncodeError::new(format!("{what} cannot be negative"))),
    }
}

fn storm_value(out: &mut Vec<u8>, v: Option<u16>, width: usize) -> Result<(), EncodeError> {
    match v {
        None => out.extend(core::iter::repeat_n(b'.', width)),
        Some(v) if u32::from(v) < 10u32.pow(width as u32) => push_digits(out, u32::from(v), width),
        Some(_) => return Err(EncodeError::new("a storm value does not fit its field")),
    }
    Ok(())
}

pub(crate) fn frequency(out: &mut Vec<u8>, f: &crate::VoiceFrequency) -> Result<(), EncodeError> {
    if !(0.0..24_300.0).contains(&f.mhz) {
        return Err(EncodeError::new("the frequency must be below 24300 MHz"));
    }
    let (per_mhz, decimals) = if f.ten_khz_resolution { (100u64, 2usize) } else { (1000, 3) };
    let units = libm::round(f.mhz * per_mhz as f64) as u64;
    let (whole, fraction) = (units / per_mhz, units % per_mhz);
    let head = if whole < 1000 {
        format!("{whole:03}")
    } else {
        let Some((letter, _)) = comment::MICROWAVE_PREFIXES.iter().find(|(_, p)| u64::from(*p) == whole / 100) else {
            return Err(EncodeError::new("APRS12c ch. 18 has no letter for this frequency above 999 MHz"));
        };
        format!("{}{:02}", *letter as char, whole % 100)
    };
    out.extend_from_slice(head.as_bytes());
    out.push(b'.');
    out.extend_from_slice(format!("{fraction:0decimals$}").as_bytes());
    out.extend_from_slice(if f.ten_khz_resolution { b" MHz" } else { b"MHz" });
    if let Some(tone) = f.tone {
        out.push(b' ');
        let letter = match tone {
            Tone::Off => {
                out.extend_from_slice(if f.narrow { b"toff" } else { b"Toff" });
                None
            }
            Tone::ToneBurst => {
                out.extend_from_slice(if f.narrow { b"l750" } else { b"1750" });
                None
            }
            Tone::Tone => Some(b'T'),
            Tone::Ctcss => Some(b'C'),
            Tone::Dcs => Some(b'D'),
        };
        if let Some(letter) = letter {
            let Some(value) = f.tone_value.filter(|v| *v <= 999) else {
                return Err(EncodeError::new("a tone needs a value of up to three digits"));
            };
            out.push(if f.narrow { letter.to_ascii_lowercase() } else { letter });
            push_digits(out, u32::from(value), 3);
        }
    } else if f.narrow {
        return Err(EncodeError::new("narrow band is written with the tone field, so it needs a tone"));
    }
    if let Some(offset) = f.offset_khz {
        if offset % 10 != 0 || offset.abs() > 9990 {
            return Err(EncodeError::new("the offset is in 10 kHz steps, up to 9.99 MHz"));
        }
        out.push(b' ');
        out.push(if offset < 0 { b'-' } else { b'+' });
        push_digits(out, (offset.abs() / 10) as u32, 3);
    }
    if let Some(range) = f.range {
        if range > 99 {
            return Err(EncodeError::new("the frequency range is two digits"));
        }
        out.extend_from_slice(b" R");
        push_digits(out, u32::from(range), 2);
        out.push(if f.range_km { b'k' } else { b'm' });
    } else if f.range_km {
        return Err(EncodeError::new("range_km needs a range"));
    }
    Ok(())
}

pub(crate) fn telemetry_into(out: &mut Vec<u8>, t: &crate::CommentTelemetry) -> Result<(), EncodeError> {
    if t.analog.is_empty() || t.analog.len() > 5 || (t.digital.is_some() && t.analog.len() != 5) {
        return Err(EncodeError::new("comment telemetry has 1-5 analog values, and the digital bits only after all five"));
    }
    let max = 91 * 91 - 1;
    if t.sequence > max || t.analog.iter().any(|v| *v > max) {
        return Err(EncodeError::new("comment telemetry values are 0-8280"));
    }
    out.push(b'|');
    crate::base91::encode(u32::from(t.sequence), 2, out);
    for v in &t.analog {
        crate::base91::encode(u32::from(*v), 2, out);
    }
    if let Some(d) = t.digital {
        crate::base91::encode(u32::from(d), 2, out);
    }
    out.push(b'|');
    Ok(())
}

/// Whether what was written for `f` (from the position on) decodes back to the same comment and
/// structured elements.
fn reads_back(written: &[u8], f: &Positioned) -> bool {
    let mut ctx = Context::new(ParseOptions::LENIENT);
    let Some(decoded) = position::decode(&mut ctx, written, 0) else {
        return false;
    };
    let mut back =
        Positioned { position: decoded.position, symbol: decoded.symbol, compressed: decoded.compressed, ..Positioned::default() };
    if !comment::decode(&mut ctx, &mut back, decoded.cs, &written[decoded.len..], decoded.len) || ctx.has_errors() {
        return false;
    }
    let close = |a: Option<f64>, b: Option<f64>, tolerance: f64| match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => (x - y).abs() <= tolerance * x.abs().max(1.0),
        _ => false,
    };
    let frequency_fields =
        |v: &Option<crate::VoiceFrequency>| v.as_ref().map(|x| (x.tone, x.tone_value, x.offset_khz, x.range, x.range_km, x.narrow));
    back.comment == f.comment
        && back.phg == f.phg
        && back.dfs == f.dfs
        && back.area == f.area
        && back.df_bearing == f.df_bearing
        && back.storm == f.storm
        && back.telemetry == f.telemetry
        && frequency_fields(&back.frequency) == frequency_fields(&f.frequency)
        && back.signpost == f.signpost
        && back.dao.map(|d| d.datum) == f.dao.map(|d| d.datum)
        && back.weather.as_ref().map(weather::text_parts) == f.weather.as_ref().map(weather::text_parts)
        && close(back.altitude_feet, f.altitude_feet, 0.01)
        && close(back.range_miles, f.range_miles, 0.1)
}
