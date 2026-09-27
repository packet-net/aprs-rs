//! Mic-E (APRS12c ch. 10): latitude, message code and digipeat path in the destination address;
//! longitude, speed, course and symbol in the information field; then a status text that can
//! carry a device type code, altitude, grid locator and device suffix.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::comment::{self, METRES_TO_FEET};
use crate::context::Context;
use crate::encode;
use crate::{
    Address, Code, DaoPrecision, Data, EncodeError, MicEMessage, MicEReport, ParseOptions, Position, Positioned, base91, deviceid, position,
};

/// A destination character's meaning: a latitude digit (or a blank for ambiguity), and its flag bit.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Bit {
    Zero,
    Standard,
    Custom,
}

fn classify(c: u8) -> Option<(Option<u8>, Bit)> {
    match c {
        b'0'..=b'9' => Some((Some(c - b'0'), Bit::Zero)),
        b'A'..=b'J' => Some((Some(c - b'A'), Bit::Custom)),
        b'K' => Some((None, Bit::Custom)),
        b'L' => Some((None, Bit::Zero)),
        b'P'..=b'Y' => Some((Some(c - b'P'), Bit::Standard)),
        b'Z' => Some((None, Bit::Standard)),
        _ => None,
    }
}

const MESSAGES: [MicEMessage; 8] = [
    MicEMessage::Emergency,
    MicEMessage::Priority,
    MicEMessage::Special,
    MicEMessage::Committed,
    MicEMessage::Returning,
    MicEMessage::InService,
    MicEMessage::EnRoute,
    MicEMessage::OffDuty,
];

pub(crate) fn decode(ctx: &mut Context, destination: &Address, info: &[u8]) -> Option<Data> {
    let old_data = matches!(info[0], b'\'' | 0x1D);
    let dest = destination.callsign().as_bytes();
    let chars: Option<Vec<(Option<u8>, Bit)>> = dest.iter().map(|&c| classify(c)).collect();
    let Some(chars) = chars.filter(|c| c.len() == 6 && c[3..].iter().all(|(_, b)| *b != Bit::Custom)) else {
        ctx.error(Code::InvalidMicEDestination, "the destination is not a Mic-E latitude and message code (APRS12c ch. 10)", None);
        return None;
    };

    // Ambiguity: latitude digits blanked from the right.
    let mut ambiguity = 0u8;
    for i in [5, 4, 3, 2] {
        if chars[i].0.is_none() {
            ambiguity += 1;
        } else {
            break;
        }
    }
    if chars[..6 - ambiguity as usize].iter().any(|(d, _)| d.is_none()) || chars[0].0.is_none() || chars[1].0.is_none() {
        ctx.error(Code::InvalidMicEDestination, "the Mic-E latitude has a blank that is not trailing", None);
        return None;
    }
    let digit = |i: usize| f64::from(chars[i].0.unwrap_or(0));
    let lat_deg = digit(0) * 10.0 + digit(1);
    let lat_min = digit(2) * 10.0 + digit(3) + digit(4) / 10.0 + digit(5) / 100.0;
    let half_box = [0.0, 0.05, 0.5, 5.0, 30.0][ambiguity as usize];
    if lat_min >= 60.0 || lat_deg + (lat_min + half_box) / 60.0 > 90.0 {
        ctx.error(Code::InvalidMicEDestination, "the Mic-E latitude is out of range", None);
        return None;
    }
    let north = chars[3].1 == Bit::Standard;
    let lon_offset = chars[4].1 == Bit::Standard;
    let west = chars[5].1 == Bit::Standard;

    let bits: Vec<Bit> = chars[..3].iter().map(|(_, b)| *b).collect();
    let custom = bits.contains(&Bit::Custom);
    let message = if custom && bits.contains(&Bit::Standard) {
        MicEMessage::Unknown
    } else {
        let n = bits.iter().fold(0usize, |n, b| n * 2 + usize::from(*b != Bit::Zero));
        match (MESSAGES[n], custom) {
            (MicEMessage::Emergency, _) => MicEMessage::Emergency,
            (_, true) => MicEMessage::Custom(7 - n as u8),
            (m, false) => m,
        }
    };

    if info.len() < 9 {
        ctx.error(Code::InvalidMicEInformation, "a Mic-E information field is at least 9 bytes (APRS12c ch. 10)", Some(0));
        return None;
    }
    let (d, m, h) = (info[1], info[2], info[3]);
    if !(38..=127).contains(&d) || !(38..=97).contains(&m) || !(28..=127).contains(&h) || info[4..7].iter().any(|b| !(28..=127).contains(b))
    {
        ctx.error(Code::InvalidMicEInformation, "a Mic-E longitude, speed or course byte is out of range (APRS12c ch. 10)", Some(1));
        return None;
    }
    let mut lon_deg = u32::from(d - 28) + if lon_offset { 100 } else { 0 };
    if (180..=189).contains(&lon_deg) {
        lon_deg -= 80;
    } else if (190..=199).contains(&lon_deg) {
        lon_deg -= 190;
    }
    let mut lon_min = u32::from(m - 28);
    if lon_min >= 60 {
        lon_min -= 60;
    }
    let lon_hundredths = u32::from(h - 28);
    if lon_deg > 180 || lon_hundredths > 99 {
        ctx.error(Code::InvalidMicEInformation, "the Mic-E longitude is out of range", Some(1));
        return None;
    }
    // The latitude's ambiguity applies to the longitude (interpretations.md): blanked digits, then the centre.
    let lon_units = lon_min * 100 + lon_hundredths;
    let lon_units = match ambiguity {
        0 => lon_units,
        1 => lon_units / 10 * 10,
        2 => lon_units / 100 * 100,
        3 => lon_units / 1000 * 1000,
        _ => 0,
    };
    let latitude = lat_deg + (lat_min + half_box) / 60.0;
    let longitude = f64::from(lon_deg) + (f64::from(lon_units) / 100.0 + half_box) / 60.0;

    let (sp, dc, se) = (u32::from(info[4] - 28), u32::from(info[5] - 28), u32::from(info[6] - 28));
    let mut speed = sp * 10 + dc / 10;
    if speed >= 800 {
        speed -= 800;
    }
    let mut course = (dc % 10) * 100 + se;
    if course >= 400 {
        course -= 400;
    }
    let course = match course {
        0 => None,
        1..=360 => Some(course as u16),
        _ => {
            if !ctx.tolerate(Code::OutOfRangeValue, "a Mic-E course over 360 degrees was dropped", Some(5)) {
                return None;
            }
            None
        }
    };

    let symbol = position::symbol(ctx, info[8], info[7], 8, 7, false)?;
    let mut report = MicEReport {
        message,
        old_data,
        destination_ssid: destination.ssid(),
        fields: Positioned {
            position: Position {
                latitude: if north { latitude } else { -latitude },
                longitude: if west { -longitude } else { longitude },
                ambiguity,
            },
            symbol,
            course_degrees: course,
            speed_knots: Some(f64::from(speed)),
            ..Positioned::default()
        },
        ..MicEReport::default()
    };
    if !status(ctx, &mut report, &info[9..], 9) {
        return None;
    }
    Some(Data::MicE(report))
}

/// The status text: type code, altitude, grid locator, a data extension, the comment with what can
/// be lifted out of it, and the device suffix.
fn status(ctx: &mut Context, report: &mut MicEReport, bytes: &[u8], offset: usize) -> bool {
    let mut s: Vec<u8> = bytes.to_vec();
    if s.contains(&0xFF) {
        if !ctx.tolerate(Code::KenwoodFfPadding, "Kenwood 0xFF padding in the status text was removed (UAP 5.10)", Some(offset)) {
            return false;
        }
        s.retain(|&b| b != 0xFF);
    }

    // Rev 0 binary telemetry, looked for once the 0xFF padding is gone (vectors interpretations.md).
    if s.len() >= 6 && s[0] == 0x1D {
        ctx.info(Code::ObsoleteFormat, "obsolete Mic-E binary telemetry (APRS12c ch. 10)", Some(offset));
        report.legacy_telemetry = s[1..6].to_vec();
        s.drain(..6);
    }

    let type_code = match s.first() {
        Some(&b @ (b'`' | b'\'' | b'>' | b']' | b' ')) => Some(b),
        _ => None,
    };
    if type_code.is_some() {
        s.remove(0);
    } else if !s.is_empty() {
        ctx.info(Code::MicEMissingDeviceType, "a Mic-E report without a device type prefix (UAP 5.4)", Some(offset));
    }
    report.type_code = type_code.map(char::from);

    let suffix = deviceid::mic_e_suffix_len(type_code, &s);
    report.device_suffix = String::from_utf8_lossy(&s[s.len() - suffix..]).into_owned();
    s.truncate(s.len() - suffix);

    // Altitude: xxx} first. One later in the text (a tolerated reading) is looked for once a
    // locator and a data extension at the start have taken their bytes.
    let altitude_first = altitude(&s);
    if let Some(feet) = altitude_first {
        report.fields.altitude_feet = Some(feet);
        s.drain(..4);
    }

    // A grid locator and the /G symbol, then a space before any text.
    if let Some(len) = locator_len(&s) {
        report.locator = Some(String::from_utf8_lossy(&s[..len]).to_ascii_uppercase());
        s.drain(..len + 2);
        if !s.is_empty() {
            if s[0] == b' ' {
                s.remove(0);
            } else if !ctx.tolerate(
                Code::MissingSpaceAfterLocator,
                "text straight after the grid locator; a space comes first (APRS12c ch. 10)",
                Some(offset),
            ) {
                return false;
            }
        }
    }

    // A data extension at the start of the text, lifted before an altitude is looked for later
    // on, so its bytes are never read as one (`0PH}` in `PHG3330PH}`); one later on is found by
    // the comment's rules.
    if let Some(len) = comment::mic_e_extension(&mut report.fields, &s) {
        s.drain(..len);
    }

    // An altitude later in the text, when none came first: a tolerated reading. Taking it out
    // brings the bytes either side of it together, and a !DAO! across that join is not one.
    let mut joined = None;
    if altitude_first.is_none() {
        if let Some(i) = (0..s.len().saturating_sub(3)).find(|&i| altitude(&s[i..]).is_some()) {
            if ctx.allows(
                Code::MicEAltitudeNotFirst,
                "a Mic-E altitude after other status text instead of first (APRS12c ch. 10)",
                Some(offset + i),
            ) {
                report.fields.altitude_feet = altitude(&s[i..]);
                s.drain(i..i + 4);
                joined = Some(i);
            }
        }
    }
    comment::tail(ctx, &mut report.fields, &s, offset, joined)
}

/// A Mic-E altitude, `xxx}`, at the start of `s`, in feet.
fn altitude(s: &[u8]) -> Option<f64> {
    if s.len() >= 4 && s[3] == b'}' {
        let metres = f64::from(base91::decode(&s[..3])?) - 10_000.0;
        Some(metres * METRES_TO_FEET)
    } else {
        None
    }
}

/// `AA00` or `AA00aa`, then `/G`.
fn locator_len(s: &[u8]) -> Option<usize> {
    let letters = |b: &[u8], hi: u8| b.iter().all(|c| (b'A'..=hi).contains(&c.to_ascii_uppercase()));
    let base = s.len() >= 6 && letters(&s[0..2], b'R') && s[2..4].iter().all(u8::is_ascii_digit);
    if !base {
        return None;
    }
    if s.len() >= 8 && letters(&s[4..6], b'X') && &s[6..8] == b"/G" {
        Some(6)
    } else if &s[4..6] == b"/G" {
        Some(4)
    } else {
        None
    }
}

// ------------------------------------------------------------------ encoding

/// The destination address and information field for a Mic-E report.
pub(crate) fn encode(r: &MicEReport) -> Result<(Address, Vec<u8>), EncodeError> {
    let f = &r.fields;
    position::check_symbol(f.symbol)?;
    let p = f.position;
    if !(-90.0..=90.0).contains(&p.latitude) || !(-180.0..180.0).contains(&p.longitude) {
        return Err(EncodeError::new("a Mic-E position must be within -90 to 90 and -180 to 180 degrees"));
    }
    if p.ambiguity > 4 {
        return Err(EncodeError::new("position ambiguity is 0-4 digits"));
    }
    if f.compressed || f.weather.is_some() || f.area.is_some() || f.df_bearing.is_some() || f.storm.is_some() || f.signpost.is_some() {
        return Err(EncodeError::new("Mic-E carries no compression, weather, area, DF bearing, storm or signpost"));
    }
    if [f.phg.is_some(), f.range_miles.is_some(), f.dfs.is_some()].iter().filter(|x| **x).count() > 1 {
        return Err(EncodeError::new("only one data extension (PHG, RNG or DFS) fits in a report"));
    }
    if r.destination_ssid > 15 {
        return Err(EncodeError::new("the destination SSID is 0-15"));
    }
    let (bits, custom) = match r.message {
        MicEMessage::Unknown => {
            return Err(EncodeError::new("an unknown Mic-E message code (mixed standard and custom bits) cannot be sent"));
        }
        MicEMessage::Custom(n) if n > 6 => return Err(EncodeError::new("custom Mic-E messages are C0-C6")),
        MicEMessage::Custom(n) => (7 - usize::from(n), true),
        m => (MESSAGES.iter().position(|x| *x == m).unwrap_or(0), false),
    };
    let dao = match f.dao {
        Some(d) if d.precision != DaoPrecision::None => {
            if p.ambiguity > 0 {
                return Err(EncodeError::new("a !DAO! adds precision to an ambiguous position, which contradicts it"));
            }
            Some(d.precision == DaoPrecision::Base91)
        }
        _ => None,
    };
    let (per_minute, extra_base) = match dao {
        None => (100.0, 1u64),
        Some(false) => (1000.0, 10),
        Some(true) => (9100.0, 91),
    };

    // Latitude digits into the destination, blanked for ambiguity.
    let half_box = [0.0, 0.05, 0.5, 5.0, 30.0][p.ambiguity as usize];
    let mut lat_units = libm::round((p.latitude.abs() * 60.0 - half_box).max(0.0) * per_minute) as u64;
    let lat_extra = (lat_units % extra_base) as u8;
    lat_units /= extra_base;
    let lat_text = format!("{:02}{:02}{:02}", lat_units / 6000, lat_units % 6000 / 100, lat_units % 100);
    let mut lon_units = libm::round(p.longitude.abs() * 60.0 * per_minute) as u64;
    let lon_extra = (lon_units % extra_base) as u8;
    lon_units /= extra_base;
    let lon_deg = (lon_units / 6000) as u32;
    let lon_min = (lon_units % 6000 / 100) as u32;
    let lon_hun = (lon_units % 100) as u32;
    if lon_deg >= 180 {
        return Err(EncodeError::new("Mic-E cannot carry a longitude of 180 degrees"));
    }
    let offset = !(10..=99).contains(&lon_deg);

    let mut dest = String::with_capacity(9);
    for (i, ch) in lat_text.bytes().enumerate() {
        let blank = i >= 6 - p.ambiguity as usize;
        let d = ch - b'0';
        let (flag, is_custom) = match i {
            0..=2 => ((bits >> (2 - i)) & 1 == 1, custom),
            3 => (p.latitude >= 0.0 && !p.latitude.is_sign_negative(), false),
            4 => (offset, false),
            _ => (p.longitude.is_sign_negative(), false),
        };
        dest.push(match (flag, is_custom, blank) {
            (true, true, false) => (b'A' + d) as char,
            (true, true, true) => 'K',
            (true, false, false) => (b'P' + d) as char,
            (true, false, true) => 'Z',
            (false, _, false) => (b'0' + d) as char,
            (false, _, true) => 'L',
        });
    }
    if r.destination_ssid != 0 {
        dest.push_str(&format!("-{}", r.destination_ssid));
    }

    let mut out = Vec::with_capacity(32);
    out.push(if r.old_data { b'\'' } else { b'`' });
    out.push(match lon_deg {
        0..=9 => lon_deg + 118,
        10..=99 => lon_deg + 28,
        100..=109 => lon_deg + 8,
        _ => lon_deg - 72,
    } as u8);
    out.push(if lon_min < 10 { lon_min + 88 } else { lon_min + 28 } as u8);
    out.push((lon_hun + 28) as u8);
    let speed = match f.speed_knots {
        None => 0,
        Some(s) if (0.0..=799.4).contains(&s) => libm::round(s) as u32,
        Some(_) => return Err(EncodeError::new("Mic-E speed is 0-799 knots")),
    };
    let course = u32::from(f.course_degrees.unwrap_or(0));
    if course > 360 {
        return Err(EncodeError::new("course must be 0-360 degrees"));
    }
    // The encodings the spec's own examples use: speed tens from 'l', course hundreds + 4.
    out.push((if speed < 200 { 108 + speed / 10 } else { 28 + speed / 10 }) as u8);
    out.push((32 + (speed % 10) * 10 + course / 100) as u8);
    out.push((28 + course % 100) as u8);
    out.push(f.symbol.code as u8);
    out.push(f.symbol.table as u8);

    let status_start = out.len();
    if !r.legacy_telemetry.is_empty() {
        // A 255 would be taken for Kenwood 0xFF padding and removed on the way back in.
        if r.legacy_telemetry.len() != 5 || r.legacy_telemetry.contains(&255) {
            return Err(EncodeError::new("obsolete Mic-E binary telemetry is 5 values, each 0-254"));
        }
        out.push(0x1D);
        out.extend_from_slice(&r.legacy_telemetry);
    }
    if let Some(t) = r.type_code {
        if !matches!(t, '`' | '\'' | '>' | ']' | ' ') {
            return Err(EncodeError::new("a Mic-E type code is `, ', >, ] or a space"));
        }
        out.push(t as u8);
    }
    // Mic-E altitude is whole metres; one that is not (a /A= altitude in feet) goes in the text as /A=.
    let mut altitude_in_text = None;
    if let Some(feet) = f.altitude_feet {
        let metres = libm::round(feet / METRES_TO_FEET);
        if (metres * METRES_TO_FEET - feet).abs() > 1e-6 * feet.abs().max(1.0) {
            altitude_in_text = Some(feet);
        } else {
            let datum = metres + 10_000.0;
            if !(0.0..91.0 * 91.0 * 91.0).contains(&datum) {
                return Err(EncodeError::new("the Mic-E altitude is out of range"));
            }
            base91::encode(datum as u32, 3, &mut out);
            out.push(b'}');
        }
    }
    if let Some(loc) = &r.locator {
        let b = loc.as_bytes();
        if locator_len(&[b, b"/G"].concat()) != Some(b.len()) {
            return Err(EncodeError::new("a grid locator is 4 or 6 characters, e.g. IO91 or IO91SX"));
        }
        out.extend_from_slice(b);
        out.extend_from_slice(b"/G");
        // Anything after the locator and its symbol comes after a space (APRS12c ch. 10), a data
        // extension included; the device suffix is read off the end first, so it needs none.
        let follows = altitude_in_text.is_some()
            || f.phg.is_some()
            || f.range_miles.is_some()
            || f.dfs.is_some()
            || f.frequency.is_some()
            || !f.comment.is_empty()
            || f.telemetry.is_some()
            || f.dao.is_some();
        if follows {
            out.push(b' ');
        }
    }
    let before_extension = out.len();
    if let Some(ph) = f.phg {
        encode::phg_codes(&mut out, ph)?;
        if let Some(rate) = ph.beacons_per_hour {
            out.push(comment::beacon_rate_char(rate).ok_or_else(|| EncodeError::new("PHGR beacons per hour is 1-35"))?);
            out.push(b'/');
        }
    } else if let Some(range) = f.range_miles {
        let n = libm::round(range);
        if !(0.0..=9999.0).contains(&n) {
            return Err(EncodeError::new("RNG range is 0-9999 miles"));
        }
        out.extend_from_slice(format!("RNG{:04}", n as u32).as_bytes());
    } else if let Some(dfs) = f.dfs {
        encode::dfs_codes(&mut out, dfs)?;
    }
    let extension = out.len() > before_extension;
    if let Some(feet) = altitude_in_text {
        let n = libm::round(feet);
        if !(-99_999.0..=999_999.0).contains(&n) {
            return Err(EncodeError::new("/A= altitude must fit in 6 digits (or - and 5 digits)"));
        }
        if n < 0.0 {
            out.extend_from_slice(format!("/A=-{:05}", (-n) as u32).as_bytes());
        } else {
            out.extend_from_slice(format!("/A={:06}", n as u32).as_bytes());
        }
    }

    if let Some(fr) = &f.frequency {
        if extension {
            out.push(b'/');
        }
        encode::frequency(&mut out, fr)?;
    }

    let text = f.comment.as_bytes();
    if crate::text::has_line_break(text) {
        return Err(EncodeError::new("comment text cannot contain a line break"));
    }
    let mut trailer = Vec::new();
    if let Some(t) = &f.telemetry {
        encode::telemetry_into(&mut trailer, t)?;
    }
    if let Some(d) = f.dao {
        encode::check_dao_datum(d)?;
        trailer.push(b'!');
        match d.precision {
            DaoPrecision::Thousandths => trailer.extend_from_slice(&[d.datum as u8, b'0' + lat_extra, b'0' + lon_extra]),
            DaoPrecision::Base91 => trailer.extend_from_slice(&[d.datum.to_ascii_lowercase() as u8, lat_extra + 33, lon_extra + 33]),
            DaoPrecision::None => trailer.extend_from_slice(&[d.datum as u8, b' ', b' ']),
        }
        trailer.push(b'!');
    }
    let suffix = r.device_suffix.as_bytes();

    let text_at = out.len();
    let needs_space = !text.is_empty() && f.frequency.is_some();
    let separators: &[&[u8]] = if needs_space { &[b" ", b"", b" /"] } else { &[b"", b"/"] };
    for separator in separators {
        out.truncate(text_at);
        out.extend_from_slice(separator);
        out.extend_from_slice(text);
        out.extend_from_slice(&trailer);
        out.extend_from_slice(suffix);
        if reads_back(r, &out[status_start..]) {
            let destination = Address::new(&dest).map_err(|_| EncodeError::new("the Mic-E destination is not an address"))?;
            return Ok((destination, out));
        }
    }
    Err(EncodeError::new(
        "the status text does not read back as given: a device suffix the database does not know, or text that decodes as a structured element; set the property instead",
    ))
}

fn reads_back(r: &MicEReport, status_bytes: &[u8]) -> bool {
    let mut ctx = Context::new(ParseOptions::LENIENT);
    let mut back = MicEReport { fields: Positioned { symbol: r.fields.symbol, ..Positioned::default() }, ..MicEReport::default() };
    if !status(&mut ctx, &mut back, status_bytes, 0) || ctx.has_errors() {
        return false;
    }
    let (a, b) = (&back.fields, &r.fields);
    let close = |x: Option<f64>, y: Option<f64>| match (x, y) {
        (None, None) => true,
        (Some(x), Some(y)) => (x - y).abs() <= 1e-6 * x.abs().max(1.0),
        _ => false,
    };
    let freq = |v: &Option<crate::VoiceFrequency>| v.as_ref().map(|x| (x.tone, x.tone_value, x.offset_khz, x.range, x.range_km, x.narrow));
    back.type_code == r.type_code
        && back.legacy_telemetry == r.legacy_telemetry
        && back.device_suffix == r.device_suffix
        && back.locator.as_deref().map(str::to_ascii_uppercase) == r.locator.as_deref().map(str::to_ascii_uppercase)
        && a.comment == b.comment
        && a.phg == b.phg
        && a.dfs == b.dfs
        && a.telemetry == b.telemetry
        && a.dao.map(|d| d.datum) == b.dao.map(|d| d.datum)
        && freq(&a.frequency) == freq(&b.frequency)
        && close(a.altitude_feet, b.altitude_feet)
        && a.range_miles.map(libm::round) == b.range_miles.map(libm::round)
}
