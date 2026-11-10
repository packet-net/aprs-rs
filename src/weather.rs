//! Weather data (APRS12c ch. 12): the field run shared by positioned and positionless reports.

use alloc::string::String;
use alloc::vec::Vec;

use crate::context::Context;
use crate::{Code, EncodeError, Weather, WeatherField};

/// How the wind is read from the weather fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Wind {
    /// A positionless report: `c` and `s` are the wind, wherever they come, until each is read.
    Positionless,
    /// Already known, from the `DDD/SSS` extension or the compressed cs bytes: `c` ends the
    /// fields and `s` is snowfall.
    Known,
    /// A positioned report without the extension: a `c` field, wherever it comes, is the wind
    /// direction, and once it is read an `s` is the wind speed until that is read
    /// (`wind-fields-instead-of-extension`).
    AsFields,
}

/// What a run of weather fields held.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Run {
    /// Bytes used.
    pub(crate) len: usize,
    /// A wind direction was read from a `c` field.
    pub(crate) direction: bool,
    /// A wind speed was read from an `s` field.
    pub(crate) speed: bool,
}

/// Reads letter-and-value weather fields from the start of `bytes` into `weather`; `None` when a
/// defect was not tolerated. Which field a letter is depends on what has been read (vectors
/// interpretations.md, "Which weather field a letter is"): `c` is the wind direction until the
/// wind is known, but only with a value after it; when the wind comes as fields, `s` is the wind
/// speed until the speed is known (see [`Wind`]), and snowfall otherwise; `L` and `l` are one
/// field, luminosity. The run stops at the first thing that is not a field, at a defined field
/// already read, and at `c` once the wind direction is known. Extra fields are kept as a list, so
/// a repeated extra letter does not stop it.
pub(crate) fn fields(ctx: &mut Context, bytes: &[u8], offset: usize, weather: &mut Weather, wind: Wind) -> Option<Run> {
    let mut at = 0;
    // Fields read so far, by the field rather than the letter: snowfall is `S`, to keep it apart
    // from the wind speed `s`, and both luminosity letters are `L`.
    let mut seen: Vec<u8> = Vec::new();
    let mut extra = Vec::new();
    let known = wind == Wind::Known;
    let (mut direction_known, mut speed_known) = (known, known);
    let mut run = Run { len: 0, direction: false, speed: false };
    while at < bytes.len() {
        let letter = bytes[at];
        if letter == b'c' && direction_known {
            break;
        }
        // In a positioned report the wind comes as fields once a c field is read; an s before
        // that is snowfall (vectors differential/weather-snowfall-keeps-its-width).
        let snow = letter == b's' && (speed_known || (wind == Wind::AsFields && !run.direction));
        let width = match letter {
            b'c' | b's' | b'g' | b't' | b'r' | b'p' | b'P' | b'L' | b'l' | b'#' => 3,
            b'h' => 2,
            b'b' => 5,
            _ => 0,
        };
        let key = match letter {
            b's' if snow => b'S',
            b'l' => b'L',
            other => other,
        };
        if width > 0 && seen.contains(&key) {
            break;
        }
        let field = if width > 0 { value(ctx, bytes, at, offset, width, letter, snow)? } else { None };
        if let Some((v, len)) = field {
            seen.push(key);
            if !assign(ctx, weather, letter, snow, v, offset + at) {
                return None;
            }
            match letter {
                b'c' => (direction_known, run.direction) = (true, true),
                b's' if !snow => (speed_known, run.speed) = (true, true),
                _ => {}
            }
            at += 1 + len;
            continue;
        }
        // A letter the spec does not define with a number after it: kept as it came. At least
        // two characters (as every defined field has more), ending in a digit.
        if letter.is_ascii_alphabetic() && !is_known(letter) {
            let len = bytes[at + 1..].iter().take_while(|&&b| b.is_ascii_digit() || b == b'.' || b == b'-').count();
            if len >= 2 && bytes[at + len].is_ascii_digit() {
                extra.push(WeatherField {
                    letter: letter as char,
                    value: String::from(core::str::from_utf8(&bytes[at + 1..at + 1 + len]).unwrap_or_default()),
                });
                at += 1 + len;
                continue;
            }
        }
        break;
    }
    if !extra.is_empty() {
        weather.extra = extra;
    }
    let positionless = wind == Wind::Positionless;
    let complete =
        if positionless { [b'c', b's', b'g', b't'].iter().all(|k| seen.contains(k)) } else { seen.contains(&b'g') && seen.contains(&b't') };
    if !complete
        && !ctx.tolerate(
            Code::IncompleteWeather,
            if positionless {
                "a positionless weather report starts with the c, s, g and t fields (APRS12c ch. 12)"
            } else {
                "a weather report has the gust (g) and temperature (t) fields (APRS12c ch. 12)"
            },
            Some(offset),
        )
    {
        return None;
    }
    run.len = at;
    Some(run)
}

fn is_known(letter: u8) -> bool {
    matches!(letter, b'c' | b's' | b'g' | b't' | b'r' | b'p' | b'P' | b'h' | b'b' | b'L' | b'l' | b'#')
}

/// The value of the field whose letter is at `at`: its spec width, or (tolerated) a run of 1 to
/// width+1 digits or dots ending at a non-digit (`t45`, `h070`, `b...`). A spec-width value that
/// runs on into another digit is read at the run's width if that fits. `Some(None)` when there is
/// no field here; `None` when a defect was not tolerated.
#[allow(clippy::type_complexity)]
fn value(
    ctx: &mut Context,
    bytes: &[u8],
    at: usize,
    offset: usize,
    width: usize,
    letter: u8,
    snow: bool,
) -> Option<Option<(Option<f64>, usize)>> {
    let avail = bytes.len() - at - 1;
    let exact = if avail >= width { exact_value(&bytes[at + 1..at + 1 + width], letter, snow) } else { None };
    let followed_by_digit = avail > width && bytes[at + 1 + width].is_ascii_digit();
    // Snowfall keeps its width: a value of three characters that holds a digit is a number, and
    // a digit after it is not a fourth figure (vectors interpretations.md, "Which weather field a
    // letter is").
    if let Some(v) = exact {
        if !followed_by_digit || snow {
            return Some(Some((v, width)));
        }
    }
    let dots = avail > 0 && bytes[at + 1] == b'.';
    let minus = letter == b't' && avail > 0 && bytes[at + 1] == b'-';
    let from = at + 1 + usize::from(minus);
    let run = bytes[from..].iter().take_while(|&&b| if dots { b == b'.' } else { b.is_ascii_digit() }).count();
    // A short run of dots is an unknown snowfall only when no digit follows it: `s..6` and
    // `s.0.050` are not fields (vectors README, Weather).
    if snow && dots && run < width && bytes.get(from + run).is_some_and(u8::is_ascii_digit) {
        return Some(None);
    }
    let len = run + usize::from(minus);
    // A snowfall value (which may have a decimal point) keeps its width unless it is unknown.
    if run >= 1 && len != width && len <= width + 1 && (dots || !snow) {
        if !ctx.tolerate(
            Code::NonStandardWeatherFieldWidth,
            "a weather field is not its fixed width (APRS12c ch. 12, UAP 5.31)",
            Some(offset + at),
        ) {
            return None;
        }
        let v = if dots {
            None
        } else {
            let n = number(&bytes[from..from + run]);
            Some(if minus { -n } else { n })
        };
        return Some(Some((v, len)));
    }
    Some(exact.map(|v| (v, width)))
}

/// A value of exactly the spec width: all dots or all spaces (unknown), digits, a negative
/// temperature, or a snowfall with one decimal point.
fn exact_value(v: &[u8], letter: u8, snow: bool) -> Option<Option<f64>> {
    if v.iter().all(|&b| b == b'.') || v.iter().all(|&b| b == b' ') {
        return Some(None);
    }
    if v.iter().all(u8::is_ascii_digit) {
        return Some(Some(number(v)));
    }
    if letter == b't' && v[0] == b'-' && v[1..].iter().all(u8::is_ascii_digit) {
        return Some(Some(-number(&v[1..])));
    }
    if snow && v.iter().filter(|&&b| b == b'.').count() == 1 && v.iter().all(|&b| b == b'.' || b.is_ascii_digit()) {
        return core::str::from_utf8(v).ok().and_then(|t| t.parse::<f64>().ok()).map(Some);
    }
    None
}

fn assign(ctx: &mut Context, w: &mut Weather, letter: u8, snow: bool, v: Option<f64>, offset: usize) -> bool {
    match letter {
        b'c' => match v {
            Some(d) if d > 360.0 => {
                if !ctx.tolerate(Code::OutOfRangeValue, "a wind direction over 360 degrees was dropped", Some(offset)) {
                    return false;
                }
                w.wind_direction_degrees = None;
            }
            d => w.wind_direction_degrees = d.map(|d| d as u16),
        },
        b's' if !snow => w.wind_speed_mph = v,
        b's' => w.snow_24h_in = v,
        b'g' => w.wind_gust_mph = v,
        b't' => w.temperature_f = v,
        b'r' => w.rain_1h_in = v.map(|v| v / 100.0),
        b'p' => w.rain_24h_in = v.map(|v| v / 100.0),
        b'P' => w.rain_midnight_in = v.map(|v| v / 100.0),
        b'h' => match v {
            Some(h) if h > 100.0 => {
                if !ctx.tolerate(Code::OutOfRangeValue, "a humidity over 100% was dropped", Some(offset)) {
                    return false;
                }
                w.humidity_percent = None;
            }
            h => w.humidity_percent = h.map(|h| if h == 0.0 { 100 } else { h as u8 }),
        },
        b'b' => w.pressure_mbar = v.map(|v| v / 10.0),
        b'L' => w.luminosity_w_m2 = v.map(|v| v as u16),
        b'l' => w.luminosity_w_m2 = v.map(|v| v as u16 + 1000),
        b'#' => w.rain_raw = v.map(|v| v as u32),
        _ => {}
    }
    true
}

fn number(bytes: &[u8]) -> f64 {
    bytes.iter().fold(0.0, |n, &b| n * 10.0 + f64::from(b - b'0'))
}

/// The software type and unit, when the rest is exactly that: a letter and a 2-4 character unit
/// of letters, digits, `-` or `_`, not all digits (`wRSW`, `eMB64`, `tU2k`). A letter followed
/// only by digits is a malformed weather field instead (`b0990`).
pub(crate) fn is_software_and_unit(rest: &[u8]) -> bool {
    (3..=5).contains(&rest.len())
        && rest[0].is_ascii_alphabetic()
        && !rest[1..].iter().all(u8::is_ascii_digit)
        && rest.iter().all(|&b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Reads the software type and unit (see [`is_software_and_unit`]) into `weather`.
pub(crate) fn software_and_unit(rest: &[u8], weather: &mut Weather) -> bool {
    if !is_software_and_unit(rest) {
        return false;
    }
    weather.software = Some(rest[0] as char);
    weather.unit = Some(String::from(core::str::from_utf8(&rest[1..]).unwrap_or_default()));
    true
}

/// The parts of weather that are text rather than numbers: the software type, the unit and the
/// extra fields. Written back, they must read back the same, and not as fields (`h89b1` as a
/// software type and unit would read back as humidity).
pub(crate) fn text_parts(w: &Weather) -> (Option<char>, Option<&str>, &[WeatherField]) {
    (w.software, w.unit.as_deref(), &w.extra)
}

/// Writes the fields after the wind (gust, temperature, rain, humidity, pressure, luminosity, snow,
/// raw rain, extra fields), then software and unit. Gust and temperature are mandatory, written as
/// dots when unknown.
pub(crate) fn encode_fields(out: &mut Vec<u8>, w: &Weather) -> Result<(), EncodeError> {
    field(out, b'g', w.wind_gust_mph, 3, false)?;
    field(out, b't', w.temperature_f, 3, true)?;
    optional(out, b'r', w.rain_1h_in.map(|v| v * 100.0), 3)?;
    optional(out, b'p', w.rain_24h_in.map(|v| v * 100.0), 3)?;
    optional(out, b'P', w.rain_midnight_in.map(|v| v * 100.0), 3)?;
    if let Some(h) = w.humidity_percent {
        if !(1..=100).contains(&h) {
            return Err(EncodeError::new("humidity must be 1-100 percent"));
        }
        out.push(b'h');
        push_digits(out, u32::from(h % 100), 2);
    }
    optional(out, b'b', w.pressure_mbar.map(|v| v * 10.0), 5)?;
    if let Some(l) = w.luminosity_w_m2 {
        match l {
            0..=999 => {
                out.push(b'L');
                push_digits(out, u32::from(l), 3);
            }
            1000..=1999 => {
                out.push(b'l');
                push_digits(out, u32::from(l - 1000), 3);
            }
            _ => return Err(EncodeError::new("luminosity must be 0-1999 W/m2")),
        }
    }
    if let Some(snow) = w.snow_24h_in {
        out.push(b's');
        snowfall(out, snow)?;
    }
    if let Some(r) = w.rain_raw {
        if r > 999 {
            return Err(EncodeError::new("the raw rain counter must be 0-999"));
        }
        out.push(b'#');
        push_digits(out, r, 3);
    }
    for extra in &w.extra {
        // As they are read: a letter the spec does not define, then two or more digits, dots or
        // '-', ending in a digit.
        let v = extra.value.as_bytes();
        if !extra.letter.is_ascii_alphabetic()
            || is_known(extra.letter as u8)
            || v.len() < 2
            || !v.iter().all(|&b| b.is_ascii_digit() || b == b'.' || b == b'-')
            || !v[v.len() - 1].is_ascii_digit()
        {
            return Err(EncodeError::new("an extra weather field is a letter the spec does not define and a number"));
        }
        out.push(extra.letter as u8);
        out.extend_from_slice(extra.value.as_bytes());
    }
    match (w.software, &w.unit) {
        (Some(s), Some(u)) if s.is_ascii() && is_software_and_unit(alloc::format!("{s}{u}").as_bytes()) => {
            out.push(s as u8);
            out.extend_from_slice(u.as_bytes());
        }
        (None, None) => {}
        _ => return Err(EncodeError::new("weather software is a letter, and the unit 2-4 letters, digits, - or _, not all digits")),
    }
    Ok(())
}

/// A mandatory field: digits, or dots when unknown.
pub(crate) fn field(out: &mut Vec<u8>, letter: u8, value: Option<f64>, width: usize, signed: bool) -> Result<(), EncodeError> {
    out.push(letter);
    match value {
        None => out.extend(core::iter::repeat_n(b'.', width)),
        Some(v) => number_field(out, v, width, signed)?,
    }
    Ok(())
}

fn optional(out: &mut Vec<u8>, letter: u8, value: Option<f64>, width: usize) -> Result<(), EncodeError> {
    if let Some(v) = value {
        out.push(letter);
        number_field(out, v, width, false)?;
    }
    Ok(())
}

fn number_field(out: &mut Vec<u8>, value: f64, width: usize, signed: bool) -> Result<(), EncodeError> {
    let n = libm::round(value);
    if (n - value).abs() > 1e-6 {
        return Err(EncodeError::new("a weather value is outside what its fixed-width field can carry"));
    }
    let max = libm::pow(10.0, width as f64) - 1.0;
    let min = if signed { -(libm::pow(10.0, width as f64 - 1.0) - 1.0) } else { 0.0 };
    if n > max || n < min {
        return Err(EncodeError::new("a weather value is outside what its fixed-width field can carry"));
    }
    if n < 0.0 {
        out.push(b'-');
        push_digits(out, (-n) as u32, width - 1);
    } else {
        push_digits(out, n as u32, width);
    }
    Ok(())
}

/// Snowfall in its three characters, which may include one decimal point (APRS12c ch. 12: "A
/// decimal point is allowed for non-integer values"): a whole number as three digits (`012`), one
/// under 1 as `.` and two digits (`.50`, `.25`), and any other as a digit, `.` and a digit (`1.5`).
/// A value those cannot hold exactly is refused.
fn snowfall(out: &mut Vec<u8>, inches: f64) -> Result<(), EncodeError> {
    let exact = |scale: f64| {
        let n = libm::round(inches * scale);
        ((n - inches * scale).abs() <= 1e-6).then_some(n as u32)
    };
    if !(0.0..=999.0).contains(&inches) {
        return Err(EncodeError::new("snowfall must be 0-999 inches"));
    }
    let text = match (exact(1.0), exact(10.0), exact(100.0)) {
        (Some(n), _, _) => alloc::format!("{n:03}"),
        (None, _, Some(n)) if n < 100 => alloc::format!(".{n:02}"),
        (None, Some(n), _) if n < 100 => alloc::format!("{}.{}", n / 10, n % 10),
        _ => return Err(EncodeError::new("snowfall is three characters with at most one decimal point, which cannot hold this value")),
    };
    out.extend_from_slice(text.as_bytes());
    Ok(())
}

pub(crate) fn push_digits(out: &mut Vec<u8>, value: u32, width: usize) {
    let text = alloc::format!("{:0width$}", value, width = width);
    out.extend_from_slice(text.as_bytes());
}
