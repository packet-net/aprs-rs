//! Everything after a position: data extensions (APRS12c ch. 7), weather (ch. 12), storm data,
//! and the comment with what can be lifted out of it: altitude, `!DAO!`, base-91 telemetry, a
//! voice frequency and signpost text.

use alloc::string::String;
use alloc::vec::Vec;

use crate::context::Context;
use crate::position::Cs;
use crate::weather;
use crate::{
    AreaColor, AreaObject, AreaShape, Code, CommentTelemetry, CompressionSource, Dao, DaoPrecision, DfBearing, DfSignalStrength,
    ParseOptions, Phg, Positioned, Storm, StormKind, Tone, VoiceFrequency, Weather, base91, text,
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
pub(crate) fn is_storm_symbol(fields: &Positioned) -> bool {
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
        if cs.t.source == CompressionSource::Gga {
            fields.altitude_feet = Some(libm::pow(1.002, f64::from(c * 91 + s)));
        } else if cs.c == b'{' {
            fields.range_miles = Some(2.0 * libm::pow(1.08, f64::from(s)));
        } else if weather_symbol {
            weather.wind_direction_degrees = Some(f64::from(c * 4));
            weather.wind_speed_mph = Some((libm::pow(1.08, f64::from(s)) - 1.0) * KNOTS_TO_MPH);
            cs_is_wind = true;
        } else {
            // The compressed course has no "unknown": 0 is north, reported as 360 (interpretations.md).
            fields.course_degrees = Some(if c == 0 { 360.0 } else { f64::from(c * 4) });
            fields.speed_knots = Some(libm::pow(1.08, f64::from(s)) - 1.0);
        }
    }

    if weather_symbol {
        // A weather station's report is weather, whatever else it holds: the wind, then the fields.
        let mut at = 0;
        let wind = if bytes.len() >= 7 && is_course_speed(&bytes[..7]) {
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
                d => d.map(f64::from),
            };
            weather.wind_speed_mph = speed.map(f64::from);
            at = 7;
            weather::Wind::Known
        } else if cs_is_wind {
            weather::Wind::Known
        } else {
            // The wind is decided here, where the extension belongs, before any field is read
            // (vectors README, "The order of checks"): sent as c and s fields in its place, or
            // missing. After a compressed position the cs bytes say it is unknown, not missing.
            let probe = weather::fields(
                &mut Context::new(ParseOptions::LENIENT),
                bytes,
                offset,
                &mut Weather::default(),
                weather::Wind::AsFields,
                weather_end(&bytes[at..]) + at,
            );
            // The wind comes as fields once a c field is read (an s is only the speed after one).
            let (direction, speed) = probe.map_or((false, false), |r| (r.direction, r.speed));
            if direction
                && !ctx.tolerate(
                    Code::WindFieldsInsteadOfExtension,
                    "wind sent as c/s fields instead of the DDD/SSS extension",
                    Some(offset),
                )
            {
                return false;
            }
            let wind_complete = fields.compressed || (direction && speed);
            if !wind_complete
                && !ctx.tolerate(
                    Code::IncompleteWeather,
                    if direction {
                        "the wind needs both a direction and a speed (APRS12c ch. 12)"
                    } else {
                        "the weather report has no wind direction/speed extension (APRS12c ch. 12)"
                    },
                    Some(offset),
                )
            {
                return false;
            }
            weather::Wind::AsFields
        };
        let Some(run) = weather::fields(ctx, &bytes[at..], offset + at, &mut weather, wind, weather_end(&bytes[at..])) else {
            return false;
        };
        at += run.len;
        // Wind read from the bytes after a compressed position is written back into its cs bytes,
        // under the default type byte.
        if fields.compressed
            && !cs_is_wind
            && fields.compression.is_none()
            && (weather.wind_direction_degrees.is_some() || weather.wind_speed_mph.is_some())
        {
            fields.compression = Some(crate::CompressionType {
                fix: crate::CompressionFix::Current,
                source: CompressionSource::Other,
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
    tail(ctx, fields, &bytes[at..], offset + at, None, false)
}

/// Where the weather text of a positioned report ends in `bytes`: the start of the trailing base-91
/// telemetry or `!DAO!` that is lifted out first, or the end of the bytes.
fn weather_end(bytes: &[u8]) -> usize {
    let (telemetry, dao) = plan_lift(&Text::new(bytes.to_vec(), None));
    telemetry.map(|t| t.0.start).into_iter().chain(dao.map(|d| d.0.start)).min().unwrap_or(bytes.len())
}

/// After a position's weather data: base-91 telemetry and a `!DAO!` are lifted out first, and
/// what is left is the software type and unit, or else text that a weather report should not have.
fn weather_tail(ctx: &mut Context, fields: &mut Positioned, bytes: &[u8], offset: usize) -> bool {
    let mut c = Text::new(bytes.to_vec(), None);
    if !lift_telemetry_and_dao(ctx, fields, &mut c, offset, false) {
        return false;
    }
    // The software type and unit are one run of bytes as sent: one with an element lifted from
    // inside it is text.
    if c.b.is_empty() || (c.cuts.is_empty() && weather::software_and_unit(&c.b, fields.weather.get_or_insert_with(Weather::default))) {
        return true;
    }
    if !ctx.tolerate(Code::WeatherComment, "text after the weather data; a weather report has no comment (UAP 2.7.1)", Some(offset)) {
        return false;
    }
    finish(ctx, fields, c.b, offset)
}

/// Lifts base-91 telemetry and a `!DAO!` out of `c`. The telemetry block is located first, so a
/// DAO-shaped run inside it is not taken for a DAO. Both are bytes as sent: neither is read across
/// a place where something was lifted before.
///
/// `after_locator`: `c` follows a Mic-E locator, whose space is judged on what is left once the
/// elements are out ([`locator_space`]), before the `!DAO!`'s own checks.
fn lift_telemetry_and_dao(ctx: &mut Context, fields: &mut Positioned, c: &mut Text, offset: usize, after_locator: bool) -> bool {
    let (telemetry, dao) = plan_lift(c);
    let mut removals: Vec<core::ops::Range<usize>> = Vec::new();
    removals.extend(telemetry.as_ref().map(|t| t.0.clone()));
    removals.extend(dao.as_ref().map(|d| d.0.clone()));
    removals.sort_by_key(|r| core::cmp::Reverse(r.start));
    for r in removals {
        c.lift(r, false);
    }
    if after_locator && !locator_space(ctx, c, offset) {
        return false;
    }
    if let Some((_, t)) = telemetry {
        fields.telemetry = Some(t);
    }
    if let Some((range, found)) = dao {
        if !apply_dao(ctx, fields, found, offset + range.start) {
            return false;
        }
    }
    true
}

type Lifted<T> = Option<(core::ops::Range<usize>, T)>;

/// The telemetry block and the `!DAO!` that lifting takes out of `c`. The telemetry block is
/// located first.
fn plan_lift(c: &Text) -> (Lifted<CommentTelemetry>, Lifted<FoundDao>) {
    let telemetry = find_telemetry(c);
    let dao = find_dao(c, telemetry.as_ref().map(|t| t.0.clone()));
    (telemetry, dao)
}

/// The text of a comment while elements are lifted out of it, and where they were. Taking an
/// element out leaves a boundary at its place, which no element is read across (vectors
/// interpretations.md, "Lifting an element never joins the text either side of it"); the comment is
/// what is left, the boundaries ignored.
pub(crate) struct Text {
    b: Vec<u8>,
    cuts: Vec<Cut>,
}

/// A boundary before byte `at`. `altitude` when only `/A=` altitudes were lifted from it, which a
/// voice frequency's fields are read across.
#[derive(Clone, Copy)]
struct Cut {
    at: usize,
    altitude: bool,
}

impl Text {
    /// `bytes`, with a boundary at `lifted` when something was already taken out of them there.
    pub(crate) fn new(bytes: Vec<u8>, lifted: Option<usize>) -> Self {
        let mut t = Text { b: bytes, cuts: Vec::new() };
        if let Some(at) = lifted {
            t.cut(at, false);
        }
        t
    }

    fn cut(&mut self, at: usize, altitude: bool) {
        if at == 0 || at >= self.b.len() {
            return;
        }
        match self.cuts.iter_mut().find(|c| c.at == at) {
            Some(c) => c.altitude &= altitude,
            None => self.cuts.push(Cut { at, altitude }),
        }
    }

    /// Takes `range` out, leaving a boundary. `altitude`: it is a `/A=` altitude.
    fn lift(&mut self, range: core::ops::Range<usize>, altitude: bool) {
        let n = range.len();
        self.b.drain(range.clone());
        self.shift(range.start, n);
        self.cut(range.start, altitude);
    }

    /// Takes `n` bytes out at `start` for good (a leading delimiter), with no boundary left.
    fn remove(&mut self, start: usize, n: usize) {
        self.b.drain(start..start + n);
        self.shift(start, n);
    }

    /// Moves the boundaries after `start` back by `n`, merging any that meet.
    fn shift(&mut self, start: usize, n: usize) {
        let cuts = core::mem::take(&mut self.cuts);
        for c in cuts {
            let at = if c.at >= start + n { c.at - n } else { c.at.min(start) };
            self.cut(at, c.altitude);
        }
    }

    /// Whether a boundary falls inside `start..end`, so bytes there are not one run as sent.
    fn splits(&self, start: usize, end: usize) -> bool {
        self.cuts.iter().any(|c| start < c.at && c.at < end)
    }

    /// Where the piece that holds byte `i` ends: the next boundary after it, or the end.
    fn piece_end(&self, i: usize) -> usize {
        self.cuts.iter().map(|c| c.at).filter(|&at| at > i).min().unwrap_or(self.b.len())
    }

    fn cut_at(&self, at: usize) -> Option<Cut> {
        self.cuts.iter().copied().find(|c| c.at == at)
    }
}

/// The space after a Mic-E locator, judged on `c`, the text after it with every lifted element
/// out: nothing left is fine, a space first is the separator and is taken off, and anything else
/// is a tolerated `missing-space-after-locator`.
fn locator_space(ctx: &mut Context, c: &mut Text, offset: usize) -> bool {
    match c.b.first() {
        None => true,
        Some(b' ') => {
            c.remove(0, 1);
            true
        }
        Some(_) => ctx.tolerate(
            Code::MissingSpaceAfterLocator,
            "text straight after the grid locator; a space comes first (APRS12c ch. 10)",
            Some(offset),
        ),
    }
}

/// The comment after any data extension: lifts out telemetry and a `!DAO!` (from the end), then an
/// altitude, signpost or corridor braces, a data extension found later in the text, and a voice
/// frequency at the start, in the order that keeps one from being taken for another; the rest,
/// less one leading delimiter, is the comment. Each element is read only where its bytes stand
/// whole as sent: `lifted` is where bytes were already taken out of the text (a Mic-E altitude),
/// and no element is read across it or across any place lifted here.
pub(crate) fn tail(
    ctx: &mut Context,
    fields: &mut Positioned,
    bytes: &[u8],
    offset: usize,
    lifted: Option<usize>,
    after_locator: bool,
) -> bool {
    let mut c = Text::new(bytes.to_vec(), lifted);
    if !lift_telemetry_and_dao(ctx, fields, &mut c, offset, after_locator) {
        return false;
    }

    // A /A= altitude is read wherever it is, and wins over a compressed or Mic-E one.
    if let Some((i, feet)) = find_altitude(&c) {
        fields.altitude_feet = Some(f64::from(feet));
        c.lift(i..i + 9, true);
    }

    braces(fields, &mut c);

    if fields.phg.is_none() && fields.range_miles.is_none() && fields.dfs.is_none() {
        late_extension(ctx, fields, &mut c, offset);
    }

    // A voice frequency at the start, after at most one delimiter, and one space after it.
    let start = usize::from(matches!(c.b.first(), Some(b' ' | b'/')));
    if let Some((f, len)) = frequency(&c, start) {
        fields.frequency = Some(f);
        let mut end = start + len;
        if c.b.get(end) == Some(&b' ') {
            end += 1;
        }
        c.remove(0, end);
    }

    finish(ctx, fields, c.b, offset)
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
/// the first well-formed braces in the comment, wherever they are (APRS12c ch. 11: "enclosing the
/// 1-3 characters in braces in the comment field"). Well formed is `{`, 1-3 characters that are
/// not braces, and `}`, the characters printable ASCII for a signpost or digits for a corridor;
/// braces that are not are comment text and do not stop the search, so `{5Wm{55}` and `{{5}`
/// hold `55` and `5`.
fn braces(fields: &mut Positioned, c: &mut Text) {
    let signpost = is_signpost_symbol(fields);
    let corridor = fields.area.is_some_and(|a| matches!(a.shape, AreaShape::LineDownRight | AreaShape::LineDownLeft));
    if !signpost && !corridor {
        return;
    }
    let well_formed = |inner: &[u8]| {
        (1..=3).contains(&inner.len())
            && !inner.iter().any(|&b| b == b'{' || b == b'}')
            && if signpost { text::is_printable_ascii(inner) } else { text::all_digits(inner) }
    };
    let found = (0..c.b.len()).filter(|&i| c.b[i] == b'{').find_map(|open| {
        // The closing brace is at most four bytes on (one to three characters between): looking
        // further would make a long run of `{` quadratic.
        let window = &c.b[open + 1..c.b.len().min(open + 5)];
        let close = open + 1 + window.iter().position(|&b| b == b'}')?;
        (well_formed(&c.b[open + 1..close]) && !c.splits(open, close + 1)).then_some((open, close))
    });
    let Some((open, close)) = found else { return };
    let inner = &c.b[open + 1..close];
    if signpost {
        fields.signpost = Some(String::from(core::str::from_utf8(inner).unwrap_or_default()));
    } else if let Some(area) = fields.area.as_mut() {
        area.corridor_width_miles = Some(text::digits(inner) as u16);
    }
    c.lift(open..close + 1, false);
}

/// A PHG, RNG or DFS extension after other comment text instead of straight after the symbol:
/// the first well-formed `PHG`, else the first well-formed `RNG`, else the first well-formed
/// `DFS`. A malformed one such as `PHG12` is comment text and does not stop the search (vectors
/// interpretations.md, "A late data extension is the first well-formed one"). Strictly that is
/// free text (UAP 5.15); recognising it is an extra reading the options allow.
fn late_extension(ctx: &mut Context, fields: &mut Positioned, c: &mut Text, offset: usize) {
    for tag in [&b"PHG"[..], b"RNG", b"DFS"] {
        let mut found = Positioned::default();
        let Some((at, len)) = (0..c.b.len().saturating_sub(2))
            .filter(|&i| &c.b[i..i + 3] == tag)
            .find_map(|i| station_extension(&mut found, &c.b[i..c.piece_end(i)]).map(|len| (i, len)))
        else {
            continue;
        };
        if ctx.allows(
            Code::DataExtensionInComment,
            "a data extension found later in the comment; the spec puts it straight after the symbol (UAP 5.15)",
            Some(offset + at),
        ) {
            (fields.phg, fields.range_miles, fields.dfs) = (found.phg, found.range_miles, found.dfs);
            c.lift(at..at + len, false);
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
            Some(c) => fields.course_degrees = Some(f64::from(c)),
            None => {}
        }
        fields.speed_knots = speed.map(f64::from);
        if is_df_symbol(fields) {
            if let Some(b) = bytes.get(7..15).and_then(df_bearing) {
                // The bearing is degrees: one over 360 drops the whole /BRG/NRQ, as an
                // out-of-range course is dropped (vectors README, Positions).
                if b.bearing_degrees > 360 {
                    if !ctx.tolerate(Code::OutOfRangeValue, "a DF bearing over 360 degrees was dropped", Some(offset + 7)) {
                        return None;
                    }
                } else {
                    fields.df_bearing = Some(b);
                }
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
        // Storm fields are digits: not dots or spaces, unlike a weather value.
        if text::all_digits(v) { Some(Some(text::digits(v) as u16)) } else { None }
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
fn find_altitude(c: &Text) -> Option<(usize, i32)> {
    (0..c.b.len().saturating_sub(8)).find_map(|i| {
        if c.splits(i, i + 9) {
            return None;
        }
        altitude_text(&c.b[i..i + 9]).map(|feet| (i, feet))
    })
}

/// The feet in a `/A=` altitude that is the whole of `w`: `/A=` and six digits, or `/A=-` and five.
fn altitude_text(w: &[u8]) -> Option<i32> {
    if w.len() != 9 || &w[..3] != b"/A=" {
        return None;
    }
    if text::all_digits(&w[3..9]) {
        Some(text::digits(&w[3..9]) as i32)
    } else if w[3] == b'-' && text::all_digits(&w[4..9]) {
        Some(-(text::digits(&w[4..9]) as i32))
    } else {
        None
    }
}

/// Whether `w` is a whole `/A=` altitude (nine bytes), as `find_altitude` reads one.
pub(crate) fn is_whole_altitude_text(w: Option<&[u8]>) -> bool {
    w.and_then(altitude_text).is_some()
}

/// Base-91 comment telemetry `|ss11...|`: 2-7 pairs between bars. The bars are taken in pairs from
/// the start: a bar and the next one that enclose a valid block (4 to 14 characters, an even number,
/// each 0x21 to 0x7B, whole as sent) make a block and both bars are used; otherwise the bar is text and
/// the next one is looked at. The LAST block found is the telemetry (packet-net/aprs-vectors#32), and no
/// block means the bars are text. One pass over the bars, each pair looked at for at most 14 bytes, so it
/// is linear in the comment.
fn find_telemetry(text: &Text) -> Option<(core::ops::Range<usize>, CommentTelemetry)> {
    let c = &text.b[..];
    let bars: Vec<usize> = c.iter().enumerate().filter(|(_, b)| **b == b'|').map(|(i, _)| i).collect();
    let mut last = None;
    let mut i = 0;
    while i + 1 < bars.len() {
        let (start, end) = (bars[i], bars[i + 1]);
        let inner = &c[start + 1..end];
        if (4..=14).contains(&inner.len())
            && inner.len() % 2 == 0
            && inner.iter().all(|&b| base91::is_digit(b))
            && !text.splits(start, end + 1)
        {
            last = Some((start, end));
            i += 2;
        } else {
            i += 1;
        }
    }
    let (start, end) = last?;
    let values: Vec<u16> = c[start + 1..end].chunks(2).map(|p| base91::decode(p).unwrap_or(0) as u16).collect();
    let (sequence, rest) = values.split_first()?;
    let (analog, digital) = if rest.len() == 6 { (&rest[..5], Some(rest[5] as u8)) } else { (rest, None) };
    Some((start..end + 1, CommentTelemetry { sequence: *sequence, analog: analog.to_vec(), digital }))
}

/// A `!DAO!` and the extra minutes of latitude and longitude it adds.
#[derive(Clone, Copy)]
struct FoundDao {
    dao: Dao,
    lat: f64,
    lon: f64,
}

/// The last `!DAO!` outside the telemetry block, whose five bytes stand whole as sent: `!Wdd!` (an upper-case
/// datum, a digit each: thousandths of a minute), `!wBB!` (a lower-case datum, a base-91
/// character each: v/91 hundredths of a minute) or `!D  !` (datum only; a letter or a digit).
fn find_dao(text: &Text, telemetry: Option<core::ops::Range<usize>>) -> Option<(core::ops::Range<usize>, FoundDao)> {
    let c = &text.b[..];
    (0..c.len().saturating_sub(4)).rev().find_map(|i| {
        if telemetry.as_ref().is_some_and(|t| i + 4 >= t.start && i < t.end) || text.splits(i, i + 5) {
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
    if fields.compressed {
        return true;
    }
    if fields.position.ambiguity > 0 {
        // A datum on an ambiguous position contradicts it, whatever its precision (`!W  !` too);
        // the position stays as it was.
        return ctx.tolerate(Code::DaoWithAmbiguity, "a !DAO! on an ambiguous position", Some(offset));
    }
    if found.dao.precision == DaoPrecision::None {
        return true;
    }
    let p = &mut fields.position;
    p.latitude += if p.latitude.is_sign_negative() { -found.lat / 60.0 } else { found.lat / 60.0 };
    p.longitude += if p.longitude.is_sign_negative() { -found.lon / 60.0 } else { found.lon / 60.0 };
    true
}

/// An APRS 1.2 voice frequency at `start` in `text`: `FFF.FFFMHz` or `FFF.FF MHz`, then optional
/// tone, offset and range fields, each after a space. Its length. The frequency and each field
/// stand whole as sent, and a field is not read across a place where something other than a `/A=`
/// altitude was lifted (APRS12c ch. 6 lets an altitude stand anywhere in the comment).
fn frequency(text: &Text, start: usize) -> Option<(VoiceFrequency, usize)> {
    let c = &text.b[..];
    if text.splits(start, start + 10) {
        return None;
    }
    let head = c.get(start..start + 10)?;
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
    let mut at = start + 10;
    // Each field is a space and four characters, then a space, the end of the text or a place
    // something was lifted from.
    let field = |at: usize| -> Option<&[u8]> {
        let b = c.get(at..at + 5)?;
        let reached = text.cut_at(at).is_none_or(|cut| cut.altitude);
        let whole = !text.splits(at, at + 5);
        let ends = c.get(at + 5).is_none_or(|&n| n == b' ') || text.cut_at(at + 5).is_some();
        (reached && whole && b[0] == b' ' && ends).then_some(&b[1..5])
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
    Some((f, at - start))
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
