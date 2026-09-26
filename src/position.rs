//! Latitude, longitude and symbol: uncompressed (APRS12c ch. 8) and compressed (ch. 9).

use alloc::format;
use alloc::vec::Vec;

use crate::context::Context;
use crate::{Code, CompressionOrigin, CompressionType, EncodeError, GpsFix, NmeaSource, Position, Symbol, base91};

/// The compressed course/speed, range or altitude bytes, kept raw: what they mean depends on the
/// symbol (a weather station sends wind in them) and on the type byte.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Cs {
    pub(crate) c: u8,
    pub(crate) s: u8,
    pub(crate) t: CompressionType,
}

pub(crate) struct Decoded {
    pub(crate) position: Position,
    pub(crate) symbol: Symbol,
    /// Bytes used.
    pub(crate) len: usize,
    pub(crate) compressed: bool,
    pub(crate) cs: Option<Cs>,
}

/// Whether a position can start with this byte: a latitude digit (uncompressed) or a symbol table
/// identifier (compressed; overlay digits are written as `a`-`j`).
pub(crate) fn can_start(b: u8) -> bool {
    b.is_ascii_digit() || b == b'/' || b == b'\\' || b.is_ascii_uppercase() || (b'a'..=b'j').contains(&b)
}

/// Decodes a position and symbol at the start of `bytes`. Errors go to `ctx`; `None` when there is
/// no usable position.
pub(crate) fn decode(ctx: &mut Context, bytes: &[u8], offset: usize) -> Option<Decoded> {
    match bytes.first() {
        None => {
            ctx.error(Code::Truncated, "the position is missing", Some(offset));
            None
        }
        Some(b) if b.is_ascii_digit() => uncompressed(ctx, bytes, offset),
        Some(&b) if can_start(b) => compressed(ctx, bytes, offset),
        Some(_) => {
            ctx.error(Code::InvalidPosition, "a position starts with a latitude digit or a symbol table identifier", Some(offset));
            None
        }
    }
}

fn uncompressed(ctx: &mut Context, bytes: &[u8], offset: usize) -> Option<Decoded> {
    if bytes.len() < 19 {
        ctx.error(Code::Truncated, "an uncompressed position needs 19 bytes: latitude, table, longitude, symbol", Some(offset));
        return None;
    }
    // Checked in the order the bytes come: latitude, table, longitude, symbol.
    let lat = &bytes[0..8];
    let lon = &bytes[9..18];

    // Ambiguity: trailing digits of the latitude replaced by spaces (APRS12c ch. 6).
    const LAT_DIGITS: [usize; 4] = [6, 5, 3, 2];
    let mut ambiguity = 0u8;
    while (ambiguity as usize) < LAT_DIGITS.len() && lat[LAT_DIGITS[ambiguity as usize]] == b' ' {
        ambiguity += 1;
    }
    let lat_ignored = &LAT_DIGITS[..ambiguity as usize];
    let lat_ok = lat[4] == b'.'
        && lat[0].is_ascii_digit()
        && lat[1].is_ascii_digit()
        && [2, 3, 5, 6].iter().all(|&i| lat[i].is_ascii_digit() || lat_ignored.contains(&i));
    if !lat_ok {
        ctx.error(Code::InvalidLatitude, "the latitude is not DDMM.hh with N or S (APRS12c ch. 6)", Some(offset));
        return None;
    }
    // An ambiguous position is the centre of the area it covers (interpretations.md).
    let half_box = [0.0, 0.05, 0.5, 5.0, 30.0][ambiguity as usize];
    let digit = |b: &[u8], i: usize, ignored: &[usize]| if ignored.contains(&i) || b[i] == b' ' { 0.0 } else { f64::from(b[i] - b'0') };
    let lat_deg = digit(lat, 0, &[]) * 10.0 + digit(lat, 1, &[]);
    let lat_min = digit(lat, 2, lat_ignored) * 10.0
        + digit(lat, 3, lat_ignored)
        + digit(lat, 5, lat_ignored) / 10.0
        + digit(lat, 6, lat_ignored) / 100.0
        + half_box;
    if lat_min >= 60.0 {
        ctx.error(Code::InvalidLatitude, "the latitude minutes are 60 or more", Some(offset));
        return None;
    }
    let north = match lat[7] {
        b'N' => true,
        b'S' => false,
        b'n' | b's' => {
            if !ctx.tolerate(Code::LowercaseHemisphere, "a lower-case latitude hemisphere (UAP 5.9)", Some(offset + 7)) {
                return None;
            }
            lat[7] == b'n'
        }
        _ => {
            ctx.error(Code::InvalidLatitude, "the latitude hemisphere is not N or S", Some(offset + 7));
            return None;
        }
    };
    let latitude = lat_deg + lat_min / 60.0;
    if latitude > 90.0 {
        ctx.error(Code::InvalidLatitude, "the latitude is beyond 90 degrees", Some(offset));
        return None;
    }

    // The symbol table sits between latitude and longitude.
    let table_ok = matches!(bytes[8], b'/' | b'\\' | b'0'..=b'9' | b'A'..=b'Z');
    if !table_ok {
        ctx.error(Code::InvalidSymbolTable, "the symbol table identifier is not /, \\, 0-9 or A-Z", Some(offset + 8));
        return None;
    }

    // The latitude's ambiguity applies to the longitude: those digits are ignored, whatever they are.
    const LON_DIGITS: [usize; 4] = [7, 6, 4, 3];
    let lon_ignored = &LON_DIGITS[..ambiguity as usize];
    let lon_ok = lon[5] == b'.'
        && (0..3).all(|i| lon[i].is_ascii_digit())
        && [3, 4, 6, 7].iter().all(|&i| lon[i].is_ascii_digit() || (lon[i] == b' ' && lon_ignored.contains(&i)));
    if !lon_ok {
        ctx.error(Code::InvalidLongitude, "the longitude is not DDDMM.hh with E or W (APRS12c ch. 6)", Some(offset + 9));
        return None;
    }
    let lon_deg = digit(lon, 0, &[]) * 100.0 + digit(lon, 1, &[]) * 10.0 + digit(lon, 2, &[]);
    let lon_min = digit(lon, 3, lon_ignored) * 10.0
        + digit(lon, 4, lon_ignored)
        + digit(lon, 6, lon_ignored) / 10.0
        + digit(lon, 7, lon_ignored) / 100.0;
    if lon_min >= 60.0 {
        ctx.error(Code::InvalidLongitude, "the longitude minutes are 60 or more", Some(offset + 9));
        return None;
    }
    let east = match lon[8] {
        b'E' => true,
        b'W' => false,
        b'e' | b'w' => {
            if !ctx.tolerate(Code::LowercaseHemisphere, "a lower-case longitude hemisphere (UAP 5.9)", Some(offset + 17)) {
                return None;
            }
            lon[8] == b'e'
        }
        _ => {
            ctx.error(Code::InvalidLongitude, "the longitude hemisphere is not E or W", Some(offset + 17));
            return None;
        }
    };
    let longitude = lon_deg + (lon_min + half_box) / 60.0;
    if longitude > 180.0 {
        ctx.error(Code::InvalidLongitude, "the longitude is beyond 180 degrees", Some(offset + 9));
        return None;
    }

    let symbol = symbol(ctx, bytes[8], bytes[18], offset + 8, offset + 18, false)?;
    Some(Decoded {
        position: Position {
            latitude: if north { latitude } else { -latitude },
            longitude: if east { longitude } else { -longitude },
            ambiguity,
        },
        symbol,
        len: 19,
        compressed: false,
        cs: None,
    })
}

fn compressed(ctx: &mut Context, bytes: &[u8], offset: usize) -> Option<Decoded> {
    if bytes.len() < 13 {
        ctx.error(Code::Truncated, "a compressed position needs 13 bytes (APRS12c ch. 9)", Some(offset));
        return None;
    }
    let (Some(y), Some(x)) = (base91::decode(&bytes[1..5]), base91::decode(&bytes[5..9])) else {
        ctx.error(Code::InvalidCompressedPosition, "the compressed latitude or longitude is not base-91", Some(offset + 1));
        return None;
    };
    let latitude = 90.0 - f64::from(y) / 380_926.0;
    let longitude = -180.0 + f64::from(x) / 190_463.0;
    if !(-90.0..=90.0).contains(&latitude) || !(-180.0..=180.0).contains(&longitude) {
        ctx.error(Code::InvalidCompressedPosition, "the compressed position is out of range", Some(offset + 1));
        return None;
    }
    let symbol = symbol(ctx, bytes[0], bytes[9], offset, offset + 9, true)?;

    let (c, s, t) = (bytes[10], bytes[11], bytes[12]);
    let cs = if c == b' ' {
        None
    } else {
        if !base91::is_digit(c) || !base91::is_digit(s) || !base91::is_digit(t) {
            ctx.error(
                Code::InvalidCompressedPosition,
                "the compressed course/speed, range or altitude bytes are not base-91",
                Some(offset + 10),
            );
            return None;
        }
        let bits = t - 33;
        if bits & 0xC0 != 0
            && !ctx.tolerate(
                Code::CompressionTypeReservedBits,
                "the compression type byte sets its unused high bits (APRS12c ch. 9)",
                Some(offset + 12),
            )
        {
            return None;
        }
        Some(Cs { c, s, t: type_from_bits(bits) })
    };

    Some(Decoded { position: Position { latitude, longitude, ambiguity: 0 }, symbol, len: 13, compressed: true, cs })
}

fn type_from_bits(bits: u8) -> CompressionType {
    CompressionType {
        fix: if bits & 0x20 != 0 { GpsFix::Current } else { GpsFix::Old },
        source: match (bits >> 3) & 3 {
            0 => NmeaSource::Other,
            1 => NmeaSource::Gll,
            2 => NmeaSource::Gga,
            _ => NmeaSource::Rmc,
        },
        origin: match bits & 7 {
            0 => CompressionOrigin::Compressed,
            1 => CompressionOrigin::TncBeaconText,
            2 => CompressionOrigin::Software,
            3 => CompressionOrigin::Reserved3,
            4 => CompressionOrigin::Kpc3,
            5 => CompressionOrigin::Pico,
            6 => CompressionOrigin::OtherTracker,
            _ => CompressionOrigin::DigipeaterConversion,
        },
    }
}

pub(crate) fn type_to_bits(t: CompressionType) -> u8 {
    let fix = match t.fix {
        GpsFix::Old => 0,
        GpsFix::Current => 0x20,
    };
    let source = match t.source {
        NmeaSource::Other => 0,
        NmeaSource::Gll => 1,
        NmeaSource::Gga => 2,
        NmeaSource::Rmc => 3,
    } << 3;
    let origin = match t.origin {
        CompressionOrigin::Compressed => 0,
        CompressionOrigin::TncBeaconText => 1,
        CompressionOrigin::Software => 2,
        CompressionOrigin::Reserved3 => 3,
        CompressionOrigin::Kpc3 => 4,
        CompressionOrigin::Pico => 5,
        CompressionOrigin::OtherTracker => 6,
        CompressionOrigin::DigipeaterConversion => 7,
    };
    fix | source | origin
}

/// A symbol from its table and code bytes.
pub(crate) fn symbol(ctx: &mut Context, table: u8, code: u8, table_offset: usize, code_offset: usize, compressed: bool) -> Option<Symbol> {
    let table = match table {
        b'/' | b'\\' | b'A'..=b'Z' => table,
        b'0'..=b'9' if !compressed => table,
        b'a'..=b'j' if compressed => b'0' + (table - b'a'),
        _ => {
            ctx.error(Code::InvalidSymbolTable, "the symbol table identifier is not /, \\, 0-9 or A-Z", Some(table_offset));
            return None;
        }
    };
    if !(b'!'..=b'~').contains(&code) {
        ctx.error(Code::InvalidSymbolCode, "the symbol code is not printable ASCII", Some(code_offset));
        return None;
    }
    Some(Symbol { table: table as char, code: code as char })
}

pub(crate) fn check_symbol(symbol: Symbol) -> Result<(), EncodeError> {
    let table_ok = matches!(symbol.table, '/' | '\\' | '0'..='9' | 'A'..='Z');
    if !table_ok {
        return Err(EncodeError::new(format!("symbol table '{}' is not /, \\, 0-9 or A-Z", symbol.table)));
    }
    if !('!'..='~').contains(&symbol.code) {
        return Err(EncodeError::new("the symbol code is not printable ASCII"));
    }
    Ok(())
}

/// Writes an uncompressed position with its symbol. For a `!DAO!`, `dao_digits` receives the
/// extra latitude and longitude digits (thousandths, or base-91 when `base91`).
pub(crate) fn encode_uncompressed(
    out: &mut Vec<u8>,
    position: &Position,
    symbol: Symbol,
    dao: Option<bool>,
) -> Result<Option<(u8, u8)>, EncodeError> {
    check_symbol(symbol)?;
    if !(-90.0..=90.0).contains(&position.latitude) || position.latitude.is_nan() {
        return Err(EncodeError::new("latitude must be -90 to 90 degrees"));
    }
    if !(-180.0..=180.0).contains(&position.longitude) || position.longitude.is_nan() {
        return Err(EncodeError::new("longitude must be -180 to 180 degrees"));
    }
    if position.ambiguity > 4 {
        return Err(EncodeError::new("position ambiguity is 0-4 digits"));
    }
    if position.ambiguity > 0 && dao.is_some() {
        return Err(EncodeError::new("a !DAO! adds precision to an ambiguous position, which contradicts it"));
    }
    let (lat_text, lat_extra) = coordinate(position.latitude, 2, position.ambiguity, dao);
    let (lon_text, lon_extra) = coordinate(position.longitude, 3, position.ambiguity, dao);
    out.extend_from_slice(&lat_text);
    out.push(if position.latitude.is_sign_negative() { b'S' } else { b'N' });
    out.push(symbol.table as u8);
    out.extend_from_slice(&lon_text);
    out.push(if position.longitude.is_sign_negative() { b'W' } else { b'E' });
    out.push(symbol.code as u8);
    Ok(dao.map(|_| (lat_extra, lon_extra)))
}

/// Degrees and minutes to hundredths (`DDMM.hh` or `DDDMM.hh`), with ambiguity blanks, plus the
/// `!DAO!` digit for the next place when asked for (`Some(base91)`).
fn coordinate(value: f64, degree_digits: usize, ambiguity: u8, dao: Option<bool>) -> (Vec<u8>, u8) {
    let value = value.abs();
    // The value in units of the smallest step written.
    let (per_minute, extra_base) = match dao {
        None => (100.0, 1u64),
        Some(false) => (1000.0, 10),
        Some(true) => (9100.0, 91),
    };
    let half_box = [0.0, 0.05, 0.5, 5.0, 30.0][ambiguity as usize];
    let minutes_total = value * 60.0 - half_box;
    let mut units = libm::round(minutes_total.max(0.0) * per_minute) as u64;
    let extra = (units % extra_base) as u8;
    units /= extra_base;
    // units is now hundredths of a minute.
    let degrees = units / 6000;
    let hundredths = units % 6000;
    let mut text = Vec::with_capacity(9);
    let deg_text = format!("{:0width$}", degrees, width = degree_digits);
    text.extend_from_slice(deg_text.as_bytes());
    text.extend_from_slice(format!("{:02}.{:02}", hundredths / 100, hundredths % 100).as_bytes());
    // Blank the trailing digits: hundredths, tenths, units of minutes, tens of minutes.
    let n = text.len();
    let blanks = [n - 1, n - 2, n - 4, n - 5];
    for &i in &blanks[..ambiguity as usize] {
        text[i] = b' ';
    }
    (text, extra)
}

/// Writes a compressed position: table, latitude, longitude, symbol code. The cs and type bytes follow separately.
pub(crate) fn encode_compressed(out: &mut Vec<u8>, position: &Position, symbol: Symbol) -> Result<(), EncodeError> {
    check_symbol(symbol)?;
    if position.ambiguity != 0 {
        return Err(EncodeError::new("a compressed position cannot be ambiguous"));
    }
    if !(-90.0..=90.0).contains(&position.latitude) || !(-180.0..=180.0).contains(&position.longitude) {
        return Err(EncodeError::new("the position is out of range"));
    }
    // Round, rather than truncate as APRS12c's worked example does (interpretations.md).
    let y = libm::round(380_926.0 * (90.0 - position.latitude)) as u32;
    let x = libm::round(190_463.0 * (180.0 + position.longitude)) as u32;
    out.push(match symbol.table {
        d @ '0'..='9' => b'a' + (d as u8 - b'0'),
        t => t as u8,
    });
    base91::encode(y.min(91u32.pow(4) - 1), 4, out);
    base91::encode(x.min(91u32.pow(4) - 1), 4, out);
    out.push(symbol.code as u8);
    Ok(())
}
