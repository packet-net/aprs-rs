//! Weather data (APRS12c ch. 12): the field run shared by positioned and positionless reports.

use alloc::string::String;
use alloc::vec::Vec;

use crate::context::Context;
use crate::{Code, EncodeError, Weather, WeatherField};

/// How the run starts.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Start {
    /// A positionless report: `cccsssggg...`, wind first as fields.
    Positionless,
    /// After a position whose wind came from the `DDD/SSS` extension or the compressed cs bytes.
    WindKnown,
    /// After a position with no wind extension: `c`/`s` fields there are tolerated.
    WindMissing,
}

/// The result of reading a field run.
pub(crate) struct Run {
    /// Bytes used by the fields.
    pub(crate) len: usize,
    /// Which mandatory fields were present (even as unknown): wind direction, wind speed, gust, temperature.
    pub(crate) has_gust: bool,
    pub(crate) has_temperature: bool,
    pub(crate) has_wind: bool,
    /// A positionless report's `s` wind speed field was present.
    pub(crate) has_wind_speed: bool,
}

/// Reads weather fields from the start of `bytes` into `weather`. `None` when a defect was not tolerated.
pub(crate) fn fields(ctx: &mut Context, bytes: &[u8], offset: usize, weather: &mut Weather, start: Start) -> Option<Run> {
    let mut at = 0;
    let mut run = Run { len: 0, has_gust: false, has_temperature: false, has_wind: start == Start::WindKnown, has_wind_speed: false };
    let mut first = true;
    let mut after_wind_direction = false;
    while at < bytes.len() {
        let letter = bytes[at];
        // `c` is wind direction only as the first field of a run that has no wind yet, and `s` is
        // wind speed only straight after it; any other `s` is snowfall.
        let wind_direction = letter == b'c' && first && start != Start::WindKnown;
        let wind_speed = letter == b's' && after_wind_direction;
        after_wind_direction = false;
        let (width, signed) = match letter {
            b'c' if wind_direction => (3, false),
            b's' | b'g' | b'r' | b'p' | b'P' | b'L' | b'l' | b'#' => (3, false),
            b't' => (3, true),
            b'h' => (2, false),
            b'b' => (5, false),
            _ if letter.is_ascii_alphabetic() => (0, false),
            _ => break,
        };
        let value_start = at + 1;
        if width == 0 {
            // A letter the spec does not define, with a number after it: kept as it came.
            // Three digits at least, as every defined field has.
            let len = bytes[value_start..].iter().take_while(|b| b.is_ascii_digit()).count();
            if len < 3 {
                break;
            }
            weather.extra.push(WeatherField {
                letter: letter as char,
                value: String::from(core::str::from_utf8(&bytes[value_start..value_start + len]).unwrap_or_default()),
            });
            at = value_start + len;
            first = false;
            continue;
        }

        let Some((value, len)) = value(ctx, &bytes[value_start..], offset + value_start, width, signed)? else {
            break;
        };
        match letter {
            b'c' if wind_direction => {
                if start == Start::WindMissing
                    && !ctx.tolerate(
                        Code::WindFieldsInsteadOfExtension,
                        "wind sent as c/s fields instead of the DDD/SSS extension",
                        Some(offset + at),
                    )
                {
                    return None;
                }
                after_wind_direction = true;
                run.has_wind = true;
                match value {
                    Some(v) if v > 360.0 => {
                        if !ctx.tolerate(Code::OutOfRangeValue, "a wind direction over 360 degrees was dropped", Some(offset + at)) {
                            return None;
                        }
                    }
                    v => weather.wind_direction_degrees = v.map(|v| v as u16),
                }
            }
            b's' if wind_speed => {
                run.has_wind_speed = true;
                weather.wind_speed_mph = value;
            }
            b's' => weather.snow_24h_in = value,
            b'g' => {
                run.has_gust = true;
                weather.wind_gust_mph = value;
            }
            b't' => {
                run.has_temperature = true;
                weather.temperature_f = value;
            }
            b'r' => weather.rain_1h_in = value.map(|v| v / 100.0),
            b'p' => weather.rain_24h_in = value.map(|v| v / 100.0),
            b'P' => weather.rain_midnight_in = value.map(|v| v / 100.0),
            b'h' => match value {
                Some(v) if v > 100.0 => {
                    if !ctx.tolerate(Code::OutOfRangeValue, "a humidity over 100% was dropped", Some(offset + at)) {
                        return None;
                    }
                }
                v => weather.humidity_percent = v.map(|v| if v == 0.0 { 100 } else { v as u8 }),
            },
            b'b' => weather.pressure_mbar = value.map(|v| v / 10.0),
            b'L' => weather.luminosity_w_m2 = value.map(|v| v as u16),
            b'l' => weather.luminosity_w_m2 = value.map(|v| v as u16 + 1000),
            b'#' => weather.rain_raw = value.map(|v| v as u32),
            _ => {}
        }
        at = value_start + len;
        first = false;
    }
    run.len = at;
    Some(run)
}

/// A field value: digits, or all dots or spaces for unknown. The spec width is tried first; a
/// run of digits one shorter or longer is a tolerated defect. `Some(None)` when there is no value
/// here (the run ends); `None` when a defect was not tolerated.
#[allow(clippy::type_complexity)]
fn value(ctx: &mut Context, bytes: &[u8], offset: usize, width: usize, signed: bool) -> Option<Option<(Option<f64>, usize)>> {
    if bytes.len() >= width && (bytes[..width].iter().all(|&b| b == b'.') || bytes[..width].iter().all(|&b| b == b' ')) {
        return Some(Some((None, width)));
    }
    let dots = bytes.iter().take_while(|&&b| b == b'.').count();
    if dots > 0 {
        if dots > width + 1 {
            return Some(None);
        }
        if !ctx.tolerate(Code::NonStandardWeatherFieldWidth, "a weather field is not its fixed width (UAP 5.31)", Some(offset)) {
            return None;
        }
        return Some(Some((None, dots)));
    }
    let negative = signed && bytes.first() == Some(&b'-');
    let digits_start = usize::from(negative);
    let digits = bytes[digits_start..].iter().take_while(|b| b.is_ascii_digit()).count();
    let total = digits_start + digits;
    if digits == 0 {
        return Some(None);
    }
    if total > width + 1 {
        // More digits than any field has: read the spec width and let the rest end the run.
        return Some(Some((Some(number(&bytes[..width])), width)));
    }
    if total != width
        && !ctx.tolerate(Code::NonStandardWeatherFieldWidth, "a weather field is not its fixed width (UAP 5.31)", Some(offset))
    {
        return None;
    }
    Some(Some((Some(number(&bytes[..total])), total)))
}

fn number(bytes: &[u8]) -> f64 {
    let (negative, digits) = match bytes.split_first() {
        Some((b'-', rest)) => (true, rest),
        _ => (false, bytes),
    };
    let n = digits.iter().fold(0.0, |n, &b| n * 10.0 + f64::from(b - b'0'));
    if negative { -n } else { n }
}

/// Reads the software type and unit after the fields, when the rest is exactly that: one
/// character and a 2-4 character unit, e.g. `wRSW` or `eMB64`.
pub(crate) fn software_and_unit(rest: &[u8], weather: &mut Weather) -> bool {
    if (3..=5).contains(&rest.len()) && rest.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'-') {
        weather.software = Some(rest[0] as char);
        weather.unit = Some(String::from(core::str::from_utf8(&rest[1..]).unwrap_or_default()));
        true
    } else {
        false
    }
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
    optional(out, b's', w.snow_24h_in, 3)?;
    if let Some(r) = w.rain_raw {
        if r > 999 {
            return Err(EncodeError::new("the raw rain counter must be 0-999"));
        }
        out.push(b'#');
        push_digits(out, r, 3);
    }
    for extra in &w.extra {
        if !extra.letter.is_ascii_alphabetic() || extra.value.is_empty() || !extra.value.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
            return Err(EncodeError::new("an extra weather field is a letter and a number"));
        }
        out.push(extra.letter as u8);
        out.extend_from_slice(extra.value.as_bytes());
    }
    match (w.software, &w.unit) {
        (Some(s), Some(u))
            if s.is_ascii_alphanumeric() && (2..=4).contains(&u.len()) && u.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') =>
        {
            out.push(s as u8);
            out.extend_from_slice(u.as_bytes());
        }
        (None, None) => {}
        _ => return Err(EncodeError::new("weather software is one character and the unit 2-4 letters or digits")),
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

pub(crate) fn push_digits(out: &mut Vec<u8>, value: u32, width: usize) {
    let text = alloc::format!("{:0width$}", value, width = width);
    out.extend_from_slice(text.as_bytes());
}
