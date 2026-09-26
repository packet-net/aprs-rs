//! Everything after a position: data extensions (APRS12c ch. 7), weather (ch. 12), storm data,
//! and the comment with what can be lifted out of it: altitude, `!DAO!`, base-91 telemetry, a
//! voice frequency and signpost text.

use alloc::string::String;
use alloc::vec::Vec;

use crate::context::Context;
use crate::position::Cs;
use crate::weather;
use crate::{
    AreaColor, AreaObject, AreaShape, Code, CommentTelemetry, Dao, DaoPrecision, DfBearing, DfSignalStrength, NmeaSource, Phg, Positioned,
    Storm, StormKind, Tone, VoiceFrequency, Weather, base91, text,
};

pub(crate) const KNOTS_TO_MPH: f64 = 1.150_779_448_023_543;
pub(crate) const METRES_TO_FEET: f64 = 1.0 / 0.3048;

pub(crate) fn is_weather_symbol(fields: &Positioned) -> bool {
    fields.symbol.code == '_'
}

fn is_df_symbol(fields: &Positioned) -> bool {
    fields.symbol.table == '/' && fields.symbol.code == '\\'
}

fn is_area_symbol(fields: &Positioned) -> bool {
    fields.symbol.table == '\\' && fields.symbol.code == 'l'
}

/// The hurricane symbol, in either table or with an overlay.
fn is_storm_symbol(fields: &Positioned) -> bool {
    fields.symbol.code == '@'
}

fn is_signpost_symbol(fields: &Positioned) -> bool {
    fields.symbol.table == '\\' && fields.symbol.code == 'm'
}

/// Reads everything after the position into `fields`: for an uncompressed position `bytes` starts
/// with the data extension, for a compressed one after the type byte. `false` when a defect was
/// not tolerated.
pub(crate) fn decode(ctx: &mut Context, fields: &mut Positioned, cs: Option<Cs>, bytes: &[u8], offset: usize) -> bool {
    let weather_symbol = is_weather_symbol(fields);
    let mut weather = Weather::default();
    let mut cs_is_wind = false;

    if let Some(cs) = cs {
        fields.compression = Some(cs.t);
        let (c, s) = (i32::from(cs.c - 33), i32::from(cs.s - 33));
        if cs.t.source == NmeaSource::Gga {
            fields.altitude_feet = Some(libm::pow(1.002, f64::from(c * 91 + s)));
        } else if cs.c == b'{' {
            fields.range_miles = Some(2.0 * libm::pow(1.08, f64::from(s)));
        } else if weather_symbol {
            weather.wind_direction_degrees = Some((c * 4) as u16);
            weather.wind_speed_mph = Some((libm::pow(1.08, f64::from(s)) - 1.0) * KNOTS_TO_MPH);
            cs_is_wind = true;
        } else {
            // The compressed course has no "unknown": 0 is north, reported as 360 (interpretations.md).
            fields.course_degrees = Some(if c == 0 { 360 } else { (c * 4) as u16 });
            fields.speed_knots = Some(libm::pow(1.08, f64::from(s)) - 1.0);
        }
    }

    if weather_symbol {
        // A weather station's report is weather, whatever else it holds: the wind, then the fields.
        let mut at = 0;
        let mut wind_as_fields = false;
        if bytes.len() >= 7 && is_course_speed(&bytes[..7]) {
            if fields.compressed
                && !ctx.tolerate(
                    Code::WindExtensionAfterCompressed,
                    "an uncompressed wind extension after a compressed weather position (UAP 5.33)",
                    Some(offset),
                )
            {
                return false;
            }
            // The extension replaces any wind in the cs bytes, unknown included (interpretations.md).
            let (dir, speed) = course_speed_values(&bytes[..7]);
            weather.wind_direction_degrees = match dir {
                Some(d) if d > 360 => {
                    if !ctx.tolerate(Code::OutOfRangeValue, "a wind direction over 360 degrees was dropped", Some(offset)) {
                        return false;
                    }
                    None
                }
                d => d.map(|d| d as u16),
            };
            weather.wind_speed_mph = speed.map(f64::from);
            at = 7;
        } else if !cs_is_wind && bytes.len() >= 4 && bytes[0] == b'c' {
            // c/s fields where the extension (or, compressed, the cs bytes) should carry the wind.
            if !ctx.tolerate(Code::WindFieldsInsteadOfExtension, "wind sent as c/s fields instead of the DDD/SSS extension", Some(offset)) {
                return false;
            }
            wind_as_fields = true;
        } else if !fields.compressed
            && !ctx.tolerate(
                Code::IncompleteWeather,
                "the weather report has no wind direction/speed extension (APRS12c ch. 12)",
                Some(offset),
            )
        {
            return false;
        }
        let Some(len) = weather::fields(ctx, &bytes[at..], offset + at, &mut weather, false, wind_as_fields) else {
            return false;
        };
        at += len;
        // Wind read from the bytes after a compressed position is written back into its cs bytes,
        // under the default type byte.
        if fields.compressed
            && !cs_is_wind
            && fields.compression.is_none()
            && (weather.wind_direction_degrees.is_some() || weather.wind_speed_mph.is_some())
        {
            fields.compression = Some(crate::CompressionType {
                fix: crate::GpsFix::Current,
                source: NmeaSource::Other,
                origin: crate::CompressionOrigin::Software,
            });
        }
        fields.weather = Some(weather);
        return weather_tail(ctx, fields, &bytes[at..], offset + at);
    }

    let mut at = 0;
    if !fields.compressed {
        match extension_at_start(ctx, fields, bytes, offset) {
            None => return false,
            Some(n) => at = n,
        }
    }
    tail(ctx, fields, &bytes[at..], offset + at)
}

/// After a position's weather data: the software and unit, or else text that a weather report
/// should not have, from which telemetry and a `!DAO!` are still lifted.
fn weather_tail(ctx: &mut Context, fields: &mut Positioned, bytes: &[u8], offset: usize) -> bool {
    if bytes.is_empty() || weather::software_and_unit(bytes, fields.weather.get_or_insert_with(Weather::default)) {
        return true;
    }
    let mut c: Vec<u8> = bytes.to_vec();
    if !lift_telemetry_and_dao(ctx, fields, &mut c, offset) {
        return false;
    }
    if c.is_empty() {
        return true;
    }
    if !ctx.tolerate(Code::WeatherComment, "text after the weather data; a weather report has no comment (UAP 2.7.1)", Some(offset)) {
        return false;
    }
    finish(ctx, fields, c, offset)
}

/// Lifts base-91 telemetry and a `!DAO!` out of `c`. The telemetry block is located first, so a
/// DAO-shaped run inside it is not taken for a DAO.
fn lift_telemetry_and_dao(ctx: &mut Context, fields: &mut Positioned, c: &mut Vec<u8>, offset: usize) -> bool {
    let telemetry = find_telemetry(c);
    let dao = find_dao(c, telemetry.as_ref().map(|t| t.0.clone()));
    let mut removals: Vec<core::ops::Range<usize>> = Vec::new();
    if let Some((range, t)) = telemetry {
        fields.telemetry = Some(t);
        removals.push(range);
    }
    if let Some((range, found)) = dao {
        if !apply_dao(ctx, fields, found, offset + range.start) {
            return false;
        }
        removals.push(range);
    }
    removals.sort_by_key(|r| core::cmp::Reverse(r.start));
    for r in removals {
        c.drain(r);
    }
    true
}

/// The comment after any data extension: lifts out telemetry and a `!DAO!` (from the end), then an
/// altitude, signpost or corridor braces, a data extension found later in the text, and a voice
/// frequency at the start, in the order that keeps one from being taken for another; the rest,
/// less one leading delimiter, is the comment.
pub(crate) fn tail(ctx: &mut Context, fields: &mut Positioned, bytes: &[u8], offset: usize) -> bool {
    let mut c: Vec<u8> = bytes.to_vec();
    if !lift_telemetry_and_dao(ctx, fields, &mut c, offset) {
        return false;
    }

    // A /A= altitude is read wherever it is, and wins over a compressed or Mic-E one.
    if let Some((i, feet)) = find_altitude(&c) {
        fields.altitude_feet = Some(f64::from(feet));
        c.drain(i..i + 9);
    }

    braces(fields, &mut c);

    if fields.phg.is_none() && fields.range_miles.is_none() && fields.dfs.is_none() {
        late_extension(ctx, fields, &mut c, offset);
    }

    // A voice frequency at the start, after at most one delimiter, and one space after it.
    let start = usize::from(matches!(c.first(), Some(b' ' | b'/')));
    if let Some((f, len)) = frequency(&c[start..]) {
        fields.frequency = Some(f);
        let mut end = start + len;
        if c.get(end) == Some(&b' ') {
            end += 1;
        }
        c.drain(..end);
    }

    finish(ctx, fields, c, offset)
}

/// One leading delimiter, a space or `/`, is not part of the text (APRS12c ch. 18); the rest is the comment.
fn finish(ctx: &mut Context, fields: &mut Positioned, mut c: Vec<u8>, offset: usize) -> bool {
    if matches!(c.first(), Some(b' ' | b'/')) {
        c.remove(0);
    }
    match text::decode(ctx, &c, offset) {
        Some(comment) => {
            fields.comment = comment;
            true
        }
        None => false,
    }
}

/// Signpost text `{ttt}` on the `\m` symbol, or a corridor width `{www}` on a line area object:
/// the first braces in the comment, holding 1-3 characters.
fn braces(fields: &mut Positioned, c: &mut Vec<u8>) {
    let signpost = is_signpost_symbol(fields);
    let corridor = fields.area.is_some_and(|a| matches!(a.shape, AreaShape::LineDownRight | AreaShape::LineDownLeft));
    if !signpost && !corridor {
        return;
    }
    let Some(open) = c.iter().position(|&b| b == b'{') else { return };
    let Some(close) = c[open..].iter().position(|&b| b == b'}').map(|i| open + i) else { return };
    let inner = &c[open + 1..close];
    if !(1..=3).contains(&inner.len()) {
        return;
    }
    if signpost && text::is_printable_ascii(inner) {
        fields.signpost = Some(String::from(core::str::from_utf8(inner).unwrap_or_default()));
    } else if corridor && text::all_digits(inner) {
        let width = text::digits(inner) as u16;
        if let Some(area) = fields.area.as_mut() {
            area.corridor_width_miles = Some(width);
        }
    } else {
        return;
    }
    c.drain(open..=close);
}

/// A PHG, RNG or DFS extension after other comment text instead of straight after the symbol:
/// the first `PHG`, else the first `RNG`, else the first `DFS`, if that one is well formed.
/// Strictly that is free text (UAP 5.15); recognising it is an extra reading the options allow.
fn late_extension(ctx: &mut Context, fields: &mut Positioned, c: &mut Vec<u8>, offset: usize) {
    for tag in [&b"PHG"[..], b"RNG", b"DFS"] {
        let Some(at) = c.windows(3).position(|w| w == tag) else { continue };
        let mut found = Positioned::default();
        let Some(len) = station_extension(&mut found, &c[at..]) else { continue };
        if ctx.allows(
            Code::DataExtensionInComment,
            "a data extension found later in the comment; the spec puts it straight after the symbol (UAP 5.15)",
            Some(offset + at),
        ) {
            (fields.phg, fields.range_miles, fields.dfs) = (found.phg, found.range_miles, found.dfs);
            c.drain(at..at + len);
        }
        return;
    }
}

fn is_course_speed(ext: &[u8]) -> bool {
    let part = |b: &[u8]| b.iter().all(u8::is_ascii_digit) || b.iter().all(|&x| x == b'.') || b.iter().all(|&x| x == b' ');
    ext.len() >= 7 && ext[3] == b'/' && part(&ext[0..3]) && part(&ext[4..7])
}

fn course_speed_values(ext: &[u8]) -> (Option<u32>, Option<u32>) {
    let value = |b: &[u8]| text::all_digits(b).then(|| text::digits(b));
    (value(&ext[0..3]), value(&ext[4..7]))
}

/// Reads a data extension at the start of an uncompressed position's comment. The number of bytes
/// used, or `None` when a defect was not tolerated.
fn extension_at_start(ctx: &mut Context, fields: &mut Positioned, bytes: &[u8], offset: usize) -> Option<usize> {
    let Some(ext) = bytes.get(..7) else { return Some(0) };
    // An area object's Tyy/Cxx looks like course/speed; the area symbol says which it is.
    if is_area_symbol(fields) {
        if let Some(area) = area(ext) {
            fields.area = Some(area);
            return Some(7);
        }
    }
    if is_course_speed(ext) {
        let (course, speed) = course_speed_values(ext);
        match course {
            Some(c) if c > 360 => {
                if !ctx.tolerate(Code::OutOfRangeValue, "a course over 360 degrees was dropped", Some(offset)) {
                    return None;
                }
            }
            Some(c) => fields.course_degrees = Some(c as u16),
            None => {}
        }
        fields.speed_knots = speed.map(f64::from);
        if is_df_symbol(fields) {
            if let Some(b) = bytes.get(7..15).and_then(df_bearing) {
                fields.df_bearing = Some(b);
                return Some(15);
            }
        } else if is_storm_symbol(fields) {
            if let Some((storm, len)) = storm(&bytes[7..]) {
                fields.storm = Some(storm);
                return Some(7 + len);
            }
        }
        return Some(7);
    }
    Some(station_extension(fields, bytes).unwrap_or(0))
}

/// A data extension at the start of a Mic-E status text (PHG, RNG, DFS, or an area on the area
/// symbol; not course and speed, which Mic-E carries itself); its length.
pub(crate) fn mic_e_extension(fields: &mut Positioned, bytes: &[u8]) -> Option<usize> {
    let ext = bytes.get(..7)?;
    if is_course_speed(ext) {
        return None;
    }
    if is_area_symbol(fields) {
        if let Some(area) = area(ext) {
            fields.area = Some(area);
            return Some(7);
        }
    }
    station_extension(fields, bytes)
}

/// PHG, RNG or DFS at the start of `bytes`, read into `fields`; its length.
fn station_extension(fields: &mut Positioned, bytes: &[u8]) -> Option<usize> {
    if let Some((p, len)) = phg(bytes) {
        fields.phg = Some(p);
        return Some(len);
    }
    let ext = bytes.get(..7)?;
    if ext.starts_with(b"RNG") && text::all_digits(&ext[3..7]) {
        fields.range_miles = Some(f64::from(text::digits(&ext[3..7])));
        return Some(7);
    }
    fields.dfs = Some(dfs(ext)?);
    Some(7)
}

/// PHG and DFS codes: digits, except the height, which goes on past 9 through the ASCII table
/// (`:` is 10, 2^10 x 10 feet), up to `~`.
fn is_phg_codes(ext: &[u8]) -> bool {
    ext[3].is_ascii_digit() && (b'0'..=b'~').contains(&ext[4]) && ext[5].is_ascii_digit() && ext[6].is_ascii_digit()
}

/// `PHGphgd`, or PHGR `PHGphgdR/`.
fn phg(bytes: &[u8]) -> Option<(Phg, usize)> {
    let ext = bytes.get(..7)?;
    if !ext.starts_with(b"PHG") || !is_phg_codes(ext) {
        return None;
    }
    let mut phg =
        Phg { power: ext[3] - b'0', height: ext[4] - b'0', gain: ext[5] - b'0', directivity: ext[6] - b'0', beacons_per_hour: None };
    let mut len = 7;
    if bytes.len() >= 9 && bytes[8] == b'/' {
        if let Some(rate) = beacon_rate(bytes[7]) {
            phg.beacons_per_hour = Some(rate);
            len = 9;
        }
    }
    Some((phg, len))
}

/// `DFSshgd`.
fn dfs(ext: &[u8]) -> Option<DfSignalStrength> {
    (ext.starts_with(b"DFS") && is_phg_codes(ext)).then(|| DfSignalStrength {
        strength: ext[3] - b'0',
        height: ext[4] - b'0',
        gain: ext[5] - b'0',
        directivity: ext[6] - b'0',
    })
}

/// PHGR beacon rate: 1-9, then A = 10 up to Z = 35 per hour.
fn beacon_rate(b: u8) -> Option<u8> {
    match b {
        b'1'..=b'9' => Some(b - b'0'),
        b'A'..=b'Z' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub(crate) fn beacon_rate_char(rate: u8) -> Option<u8> {
    match rate {
        1..=9 => Some(b'0' + rate),
        10..=35 => Some(b'A' + rate - 10),
        _ => None,
    }
}

/// `/BRG/NRQ` after the course and speed of a DF report.
fn df_bearing(bytes: &[u8]) -> Option<DfBearing> {
    if bytes.len() < 8 || bytes[0] != b'/' || bytes[4] != b'/' || !text::all_digits(&bytes[1..4]) || !text::all_digits(&bytes[5..8]) {
        return None;
    }
    if text::digits(&bytes[1..4]) > 360 {
        return None;
    }
    Some(DfBearing {
        bearing_degrees: text::digits(&bytes[1..4]) as u16,
        number: bytes[5] - b'0',
        range: bytes[6] - b'0',
        quality: bytes[7] - b'0',
    })
}

/// `/ST/www^GGG/pppp>RRR&rrr` and optionally `%ggg` (APRS12c ch. 12).
fn storm(bytes: &[u8]) -> Option<(Storm, usize)> {
    if bytes.len() < 24 || bytes[0] != b'/' {
        return None;
    }
    let kind = match &bytes[1..3] {
        b"TS" => StormKind::TropicalStorm,
        b"HC" => StormKind::Hurricane,
        b"TD" => StormKind::TropicalDepression,
        _ => return None,
    };
    let field = |at: usize, marker: u8, width: usize| -> Option<Option<u16>> {
        let b = bytes.get(at..at + 1 + width)?;
        if b[0] != marker {
            return None;
        }
        let v = &b[1..];
        if text::all_digits(v) {
            Some(Some(text::digits(v) as u16))
        } else if v.iter().all(|&x| x == b'.' || x == b' ') {
            Some(None)
        } else {
            None
        }
    };
    let sustained = field(3, b'/', 3)?;
    let gust = field(7, b'^', 3)?;
    let pressure = field(11, b'/', 4)?;
    let hurricane = field(16, b'>', 3)?;
    let tropical = field(20, b'&', 3)?;
    let (gale, len) = match field(24, b'%', 3) {
        Some(g) => (g, 28),
        None => (None, 24),
    };
    Some((
        Storm {
            kind,
            sustained_wind_knots: sustained,
            gust_knots: gust,
            central_pressure_mbar: pressure,
            hurricane_radius_nm: hurricane,
            tropical_storm_radius_nm: tropical,
            whole_gale_radius_nm: gale,
        },
        len,
    ))
}

/// `TyyCCxx` (CC is `/0`-`/9` or `10`-`15`). A line's corridor width `{www}` is read with the
/// comment's braces.
fn area(b: &[u8]) -> Option<AreaObject> {
    if !b[0].is_ascii_digit() || !text::all_digits(&b[1..3]) || !text::all_digits(&b[5..7]) {
        return None;
    }
    let color = match (b[3], b[4]) {
        (b'/', d @ b'0'..=b'9') => d - b'0',
        (b'1', d @ b'0'..=b'5') => 10 + d - b'0',
        _ => return None,
    };
    let shape = [
        AreaShape::OpenCircle,
        AreaShape::LineDownRight,
        AreaShape::OpenEllipse,
        AreaShape::OpenTriangle,
        AreaShape::OpenBox,
        AreaShape::FilledCircle,
        AreaShape::LineDownLeft,
        AreaShape::FilledEllipse,
        AreaShape::FilledTriangle,
        AreaShape::FilledBox,
    ][usize::from(b[0] - b'0')];
    Some(AreaObject {
        shape,
        lat_offset: (text::digits(&b[1..3])) as u8,
        color: AREA_COLORS[usize::from(color)],
        lon_offset: text::digits(&b[5..7]) as u8,
        corridor_width_miles: None,
    })
}

pub(crate) const AREA_COLORS: [AreaColor; 16] = [
    AreaColor::Black,
    AreaColor::Blue,
    AreaColor::Green,
    AreaColor::Cyan,
    AreaColor::Red,
    AreaColor::Violet,
    AreaColor::Yellow,
    AreaColor::Gray,
    AreaColor::BlackLow,
    AreaColor::BlueLow,
    AreaColor::GreenLow,
    AreaColor::CyanLow,
    AreaColor::RedLow,
    AreaColor::VioletLow,
    AreaColor::YellowLow,
    AreaColor::GrayLow,
];

/// `/A=nnnnnn` anywhere in the comment (APRS12c ch. 6), or the de facto `/A=-nnnnn`.
pub(crate) fn find_altitude(c: &[u8]) -> Option<(usize, i32)> {
    (0..c.len().saturating_sub(8)).find_map(|i| {
        let w = &c[i..i + 9];
        if &w[..3] != b"/A=" {
            return None;
        }
        if text::all_digits(&w[3..9]) {
            Some((i, text::digits(&w[3..9]) as i32))
        } else if w[3] == b'-' && text::all_digits(&w[4..9]) {
            Some((i, -(text::digits(&w[4..9]) as i32)))
        } else {
            None
        }
    })
}

/// Base-91 comment telemetry `|ss11...|`: 2-7 pairs between bars.
fn find_telemetry(c: &[u8]) -> Option<(core::ops::Range<usize>, CommentTelemetry)> {
    // Only the last two bars are considered: telemetry goes at the end of the comment.
    let bars: Vec<usize> = c.iter().enumerate().filter(|(_, b)| **b == b'|').map(|(i, _)| i).collect();
    if let [.., start, end] = bars[..] {
        let inner = &c[start + 1..end];
        if inner.len() < 4 || inner.len() > 14 || inner.len() % 2 != 0 || !inner.iter().all(|&b| base91::is_digit(b)) {
            return None;
        }
        let values: Vec<u16> = inner.chunks(2).map(|p| base91::decode(p).unwrap_or(0) as u16).collect();
        let (sequence, rest) = values.split_first()?;
        let (analog, digital) = if rest.len() == 6 { (&rest[..5], Some(rest[5] as u8)) } else { (rest, None) };
        return Some((start..end + 1, CommentTelemetry { sequence: *sequence, analog: analog.to_vec(), digital }));
    }
    None
}

/// A `!DAO!` and the extra minutes of latitude and longitude it adds.
#[derive(Clone, Copy)]
struct FoundDao {
    dao: Dao,
    lat: f64,
    lon: f64,
}

/// The last `!DAO!` outside the telemetry block: `!Wdd!` (an upper-case datum, a digit each:
/// thousandths of a minute), `!wBB!` (a lower-case datum, a base-91 character each: v/91
/// hundredths of a minute) or `!D  !` (datum only).
fn find_dao(c: &[u8], telemetry: Option<core::ops::Range<usize>>) -> Option<(core::ops::Range<usize>, FoundDao)> {
    (0..c.len().saturating_sub(4)).rev().find_map(|i| {
        if telemetry.as_ref().is_some_and(|t| i + 4 >= t.start && i < t.end) {
            return None;
        }
        let w = &c[i..i + 5];
        if w[0] != b'!' || w[4] != b'!' {
            return None;
        }
        let (d, a, o) = (w[1], w[2], w[3]);
        let found = if a == b' ' && o == b' ' && d.is_ascii_alphanumeric() {
            FoundDao { dao: Dao { datum: d.to_ascii_uppercase() as char, precision: DaoPrecision::None }, lat: 0.0, lon: 0.0 }
        } else if d.is_ascii_uppercase() && a.is_ascii_digit() && o.is_ascii_digit() {
            let extra = |b: u8| f64::from(b - b'0') * 0.001;
            FoundDao { dao: Dao { datum: d as char, precision: DaoPrecision::Thousandths }, lat: extra(a), lon: extra(o) }
        } else if d.is_ascii_lowercase() && base91::is_digit(a) && base91::is_digit(o) {
            let extra = |b: u8| f64::from(b - 33) / 91.0 * 0.01;
            FoundDao { dao: Dao { datum: d.to_ascii_uppercase() as char, precision: DaoPrecision::Base91 }, lat: extra(a), lon: extra(o) }
        } else {
            return None;
        };
        Some((i..i + 5, found))
    })
}

fn apply_dao(ctx: &mut Context, fields: &mut Positioned, found: FoundDao, offset: usize) -> bool {
    fields.dao = Some(found.dao);
    if found.dao.precision == DaoPrecision::None || fields.compressed {
        return true;
    }
    if fields.position.ambiguity > 0 {
        // Extra precision on an ambiguous position contradicts it; the position stays as it was.
        return ctx.tolerate(Code::DaoWithAmbiguity, "a !DAO! adds precision to an ambiguous position", Some(offset));
    }
    let p = &mut fields.position;
    p.latitude += if p.latitude.is_sign_negative() { -found.lat / 60.0 } else { found.lat / 60.0 };
    p.longitude += if p.longitude.is_sign_negative() { -found.lon / 60.0 } else { found.lon / 60.0 };
    true
}

/// An APRS 1.2 voice frequency at the start of `c`: `FFF.FFFMHz` or `FFF.FF MHz`, then optional
/// tone, offset and range fields, each after a space. Its length.
pub(crate) fn frequency(c: &[u8]) -> Option<(VoiceFrequency, usize)> {
    let head = c.get(..10)?;
    let mhz_ok = |b: &[u8]| b.eq_ignore_ascii_case(b"MHz");
    // Above 999 MHz the first digit is a letter standing for the leading digits (APRS12c ch. 18).
    let whole = if text::all_digits(&head[0..3]) {
        text::digits(&head[0..3])
    } else if text::all_digits(&head[1..3]) {
        let prefix = MICROWAVE_PREFIXES.iter().find(|(l, _)| *l == head[0])?.1;
        prefix * 100 + text::digits(&head[1..3])
    } else {
        return None;
    };
    let (mhz, ten_khz) = if head[3] == b'.' && text::all_digits(&head[4..7]) && mhz_ok(&head[7..10]) {
        (f64::from(whole) + f64::from(text::digits(&head[4..7])) / 1000.0, false)
    } else if head[3] == b'.' && text::all_digits(&head[4..6]) && head[6] == b' ' && mhz_ok(&head[7..10]) {
        (f64::from(whole) + f64::from(text::digits(&head[4..6])) / 100.0, true)
    } else {
        return None;
    };
    let mut f = VoiceFrequency {
        mhz,
        tone: None,
        tone_value: None,
        offset_khz: None,
        range: None,
        range_km: false,
        narrow: false,
        ten_khz_resolution: ten_khz,
    };
    let mut at = 10;
    // Each field is a space and four characters, then a space or the end of the text.
    let field = |at: usize| -> Option<&[u8]> {
        let b = c.get(at..at + 5)?;
        (b[0] == b' ' && c.get(at + 5).is_none_or(|&n| n == b' ')).then_some(&b[1..5])
    };

    if let Some(b) = field(at) {
        let tone = match b {
            b"Toff" | b"toff" => Some((Tone::Off, None)),
            b"1750" | b"l750" => Some((Tone::ToneBurst, None)),
            [t @ (b'T' | b't' | b'C' | b'c' | b'D' | b'd'), rest @ ..] if text::all_digits(rest) => {
                let kind = match t.to_ascii_uppercase() {
                    b'T' => Tone::Tone,
                    b'C' => Tone::Ctcss,
                    _ => Tone::Dcs,
                };
                Some((kind, Some(text::digits(rest) as u16)))
            }
            _ => None,
        };
        if let Some((kind, value)) = tone {
            f.tone = Some(kind);
            f.tone_value = value;
            f.narrow = b[0].is_ascii_lowercase();
            at += 5;
        }
    }
    if let Some(b) = field(at) {
        if matches!(b[0], b'+' | b'-') && text::all_digits(&b[1..4]) {
            let v = text::digits(&b[1..4]) as i32 * 10;
            f.offset_khz = Some(if b[0] == b'-' { -v } else { v });
            at += 5;
        }
    }
    if let Some(b) = field(at) {
        if b[0] == b'R' && text::all_digits(&b[1..3]) && matches!(b[3], b'm' | b'k') {
            f.range = Some(text::digits(&b[1..3]) as u16);
            f.range_km = b[3] == b'k';
            at += 5;
        }
    }
    Some((f, at))
}

/// The letters that stand for the leading digits of frequencies above 999 MHz (APRS12c ch. 18).
pub(crate) const MICROWAVE_PREFIXES: [(u8, u32); 15] = [
    (b'A', 12),
    (b'B', 23),
    (b'C', 24),
    (b'D', 34),
    (b'E', 56),
    (b'F', 57),
    (b'G', 58),
    (b'H', 101),
    (b'I', 102),
    (b'J', 103),
    (b'K', 104),
    (b'L', 105),
    (b'M', 240),
    (b'N', 241),
    (b'O', 242),
];
