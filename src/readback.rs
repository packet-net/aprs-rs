//! The read-back: every byte an encoder writes is decoded again and compared with the data it was
//! given before it is returned (vectors README, Rules, Read-back).
//!
//! The one way out of the encoder is [`seal`]. A writer does not return bytes: it returns [`Forms`],
//! the equivalent forms the Encoding rule allows for the data, best first, and `seal` decodes each in
//! turn, leniently, under a well-formed header, and compares it with the data given. The first form that
//! matches is returned as [`Sealed`], a type only this module can make, so no public function can hand
//! back bytes that did not pass. When none matches, the data is refused. The size check comes last.
//!
//! "Matches" is [`matches`], one function used for every report type: the decode has no warning or
//! error and an info only from a short list; and, in the neutral data form, every field given is in the
//! decode with the same value and the decode has no field not given. [`expected`] says what the decode
//! must hold: the data given, with each number a form holds in steps rounded once to the step of the
//! form the decode shows was written (written out here, apart from the writers), a value the form must
//! carry filled in, and empty and absent made one.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::{boxed::Box, format};

use crate::comment::{KNOTS_TO_MPH, METRES_TO_FEET};
use crate::context::Context;
use crate::steps::{round, same_number};
use crate::{
    Address, Capabilities, Code, CompressionFix, CompressionOrigin, CompressionSource, CompressionType, DaoPrecision, Data, EncodeError,
    Footprint, MicEReport, Nmea, ParseOptions, Position, Positioned, Query, RefusalCode, Severity, Telemetry, TelemetryCoefficients,
    Weather,
};

/// The forms one piece of data may be written in, best first, and the Mic-E destination that goes with
/// them. Only [`seal`] turns them into bytes a caller can have.
pub(crate) struct Forms {
    destination: Option<Address>,
    candidates: Vec<Vec<u8>>,
    /// The refusal of a rule the read-back would catch a fault of, given when no form reads back.
    unread: Option<EncodeError>,
}

impl Forms {
    /// A first form.
    pub(crate) fn new(first: Vec<u8>) -> Forms {
        Forms { destination: None, candidates: alloc::vec![first], unread: None }
    }

    /// The next form to try if the ones before it do not read back.
    pub(crate) fn then(mut self, next: Vec<u8>) -> Forms {
        self.candidates.push(next);
        self
    }

    /// The rule's refusal to give when no form reads back, where the fault is one a named rule covers
    /// (Refusal reasons: the read-back's code is the named rule's, where it caught a fault one covers).
    pub(crate) fn unless_read_back(mut self, refusal: EncodeError) -> Forms {
        self.unread = Some(refusal);
        self
    }

    /// The destination address these forms are written for (Mic-E computes its own).
    pub(crate) fn to(mut self, destination: Address) -> Forms {
        self.destination = Some(destination);
        self
    }
}

/// Bytes that passed the read-back and the size check. Only [`seal`] makes one.
pub(crate) struct Sealed {
    destination: Option<Address>,
    bytes: Vec<u8>,
}

impl Sealed {
    /// The information field, and the destination computed for it (Mic-E).
    pub(crate) fn into_parts(self) -> (Option<Address>, Vec<u8>) {
        (self.destination, self.bytes)
    }

    /// The information field.
    pub(crate) fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

/// Why a form did not read back, as far as the refusal needs to know.
struct Mismatch {
    /// Free text read back changed, or free text is what the data has to be changed by.
    text: bool,
    what: &'static str,
}

/// Reads back each form and returns the first that matches the data, or refuses. The refusal is
/// `text-reads-as-field` when free text reads back changed and `does-not-read-back` otherwise.
pub(crate) fn seal(given: &Data, forms: Forms) -> Result<Sealed, EncodeError> {
    let Forms { destination, candidates, unread } = forms;
    let mut first = None;
    for bytes in candidates {
        match matches(given, destination.as_ref(), &bytes) {
            Ok(()) => {
                crate::refusal::check_size(bytes.len())?;
                return Ok(Sealed { destination, bytes });
            }
            Err(m) => {
                first.get_or_insert(m);
            }
        }
    }
    if let Some(named) = unread {
        return Err(named);
    }
    Err(match first {
        Some(Mismatch { text: true, what }) => EncodeError::new(
            RefusalCode::TextReadsAsField,
            format!("the text would read back as something else: the {what} reads back differently; set the property instead"),
        ),
        Some(Mismatch { text: false, what }) => {
            EncodeError::new(RefusalCode::DoesNotReadBack, format!("the data does not read back as given: its {what} differs"))
        }
        None => EncodeError::new(RefusalCode::NotEncodable, "there is no form to write"),
    })
}

/// The header `N0CALL>APZ001` a form is read back under, or `N0CALL>` and the destination computed.
fn well_formed_header(destination: Option<&Address>) -> (Address, Address) {
    (
        Address::unchecked("N0CALL".to_string()),
        destination.cloned().unwrap_or_else(|| Address::unchecked(crate::builder::DEFAULT_DESTINATION.to_string())),
    )
}

/// Whether `bytes` decode, leniently, to what `given` says (Read-back, "The bytes match the data when").
fn matches(given: &Data, destination: Option<&Address>, bytes: &[u8]) -> Result<(), Mismatch> {
    let (source, destination) = well_formed_header(destination);
    let mut ctx = Context::new(ParseOptions::LENIENT);
    let back = crate::decode::information(&mut ctx, &source, &destination, &[], bytes);
    let clean = diagnostics_ok(&ctx.diagnostics);
    let want = expected(given, &back);
    if clean && want == back {
        return Ok(());
    }
    let text = text_changed(given, &back, !clean);
    Err(Mismatch { text, what: if clean { first_difference(&want, &back) } else { "diagnostics" } })
}

/// No warning, no error, and an info only where the rules attach it to the form the data itself asks for.
fn diagnostics_ok(diagnostics: &[crate::Diagnostic]) -> bool {
    diagnostics.iter().all(|d| match d.severity {
        Severity::Info => {
            matches!(d.code, Code::ObsoleteFormat | Code::MicEMissingDeviceType | Code::InvalidQuery | Code::InvalidTelemetryMetadata)
        }
        Severity::Warning | Severity::Error => false,
    })
}

/// Whether two packets' diagnostics are the same warnings and errors, as a multiset: a third-party
/// packet's inner `diagnostics` are part of its data, and its infos are not compared.
fn same_diagnostics(a: &[crate::Diagnostic], b: &[crate::Diagnostic]) -> bool {
    let mut left: Vec<_> = a.iter().filter(|d| d.severity != Severity::Info).map(|d| (d.severity, d.code)).collect();
    let mut right: Vec<_> = b.iter().filter(|d| d.severity != Severity::Info).map(|d| (d.severity, d.code)).collect();
    left.sort();
    right.sort();
    left == right
}

// ------------------------------------------------------------------ what the decode must hold

/// `x` if it is the value `given` stands for within 1e-9, else `given`: a number the decode holds is
/// compared with the value expected, not with a tolerance wider than the steps of the form.
fn snap(want: f64, back: f64) -> f64 {
    if same_number(want, back) { back } else { want }
}

fn snap_opt(want: Option<f64>, back: Option<f64>) -> Option<f64> {
    match (want, back) {
        (Some(w), Some(b)) => Some(snap(w, b)),
        (w, _) => w,
    }
}

/// An empty string is no string (Read-back, allowance 3).
fn absent(v: &Option<String>) -> Option<String> {
    v.clone().filter(|s| !s.is_empty())
}

/// What the decode of what was written must hold for `given`: the data given, with a number its form
/// holds in steps rounded once to the step of the form written (as `back` shows it), a value the form
/// must carry filled in, and empty text made absent. A decode of another kind is left as the data given,
/// which it differs from.
fn expected(given: &Data, back: &Data) -> Data {
    match (given, back) {
        (Data::Position(g), Data::Position(b)) => {
            let mut e = g.clone();
            e.fields = positioned(&g.fields, &b.fields, Form::Plain);
            Data::Position(e)
        }
        (Data::Object(g), Data::Object(b)) => {
            let mut e = g.clone();
            e.fields = positioned(&g.fields, &b.fields, Form::Plain);
            Data::Object(e)
        }
        (Data::Item(g), Data::Item(b)) => {
            let mut e = g.clone();
            e.fields = positioned(&g.fields, &b.fields, Form::Plain);
            Data::Item(e)
        }
        (Data::MicE(g), Data::MicE(b)) => Data::MicE(mic_e(g, b)),
        (Data::Message(g), Data::Message(_)) => {
            let mut e = g.clone();
            e.message_id = absent(&g.message_id);
            Data::Message(e)
        }
        (Data::Ack(g), Data::Ack(_)) => {
            let mut e = g.clone();
            e.message_id = absent(&g.message_id);
            Data::Ack(e)
        }
        (Data::Reject(g), Data::Reject(_)) => {
            let mut e = g.clone();
            e.message_id = absent(&g.message_id);
            Data::Reject(e)
        }
        (Data::Bulletin(g), Data::Bulletin(_)) => {
            let mut e = g.clone();
            e.message_id = absent(&g.message_id);
            Data::Bulletin(e)
        }
        (Data::NwsBulletin(g), Data::NwsBulletin(_)) => {
            let mut e = g.clone();
            e.message_id = absent(&g.message_id);
            Data::NwsBulletin(e)
        }
        (Data::TelemetryNames(g), Data::TelemetryNames(_)) => {
            let mut e = g.clone();
            e.message_id = absent(&g.message_id);
            Data::TelemetryNames(e)
        }
        (Data::TelemetryUnits(g), Data::TelemetryUnits(_)) => {
            let mut e = g.clone();
            e.message_id = absent(&g.message_id);
            Data::TelemetryUnits(e)
        }
        (Data::TelemetryBits(g), Data::TelemetryBits(_)) => {
            let mut e = g.clone();
            e.message_id = absent(&g.message_id);
            Data::TelemetryBits(e)
        }
        (Data::TelemetryCoefficients(g), Data::TelemetryCoefficients(b)) => {
            let mut e = TelemetryCoefficients { message_id: absent(&g.message_id), ..g.clone() };
            if e.coefficients.len() == b.coefficients.len() {
                for (w, x) in e.coefficients.iter_mut().zip(&b.coefficients) {
                    *w = snap(*w, *x);
                }
            }
            Data::TelemetryCoefficients(e)
        }
        (Data::DirectedQuery(g), Data::DirectedQuery(_)) => {
            let mut e = g.clone();
            e.target = absent(&g.target);
            Data::DirectedQuery(e)
        }
        (Data::Telemetry(g), Data::Telemetry(b)) => {
            let mut e: Telemetry = g.clone();
            if e.analog.len() == b.analog.len() {
                for (w, x) in e.analog.iter_mut().zip(&b.analog) {
                    *w = snap_opt(*w, *x);
                }
            }
            Data::Telemetry(e)
        }
        (Data::Weather(g), Data::Weather(_)) => {
            let mut e = g.clone();
            e.weather = weather(&g.weather, false);
            Data::Weather(e)
        }
        (Data::Nmea(g), Data::Nmea(b)) => Data::Nmea(nmea(g, b)),
        (Data::Query(g), Data::Query(b)) => {
            let mut e: Query = g.clone();
            if let (Some(f), Some(x)) = (&mut e.footprint, &b.footprint) {
                *f = footprint(f, x);
            }
            Data::Query(e)
        }
        (Data::Capabilities(g), Data::Capabilities(_)) => Data::Capabilities(Capabilities { capabilities: g.capabilities.clone() }),
        (Data::ThirdParty(g), Data::ThirdParty(b)) => {
            let mut e = (**g).clone();
            e.information = b.information.clone();
            if same_diagnostics(&g.diagnostics, &b.diagnostics) {
                e.diagnostics = b.diagnostics.clone();
            }
            e.third_party = b.third_party;
            e.data = expected(&g.data, &b.data);
            Data::ThirdParty(Box::new(e))
        }
        (given, _) => given.clone(),
    }
}

/// A footprint: its radius is whole miles, its latitude and longitude are numbers as sent.
fn footprint(g: &Footprint, b: &Footprint) -> Footprint {
    Footprint {
        latitude: snap(g.latitude, b.latitude),
        longitude: snap(g.longitude, b.longitude),
        radius_miles: snap(round(g.radius_miles), b.radius_miles),
        latitude_text: g.latitude_text.clone(),
        longitude_text: g.longitude_text.clone(),
    }
}

/// NMEA data is its sentence: a field the data leaves out is the sentence's.
fn nmea(g: &Nmea, b: &Nmea) -> Nmea {
    let fill = |w: Option<f64>, x: Option<f64>| match w {
        None => x,
        w => snap_opt(w, x),
    };
    Nmea {
        sentence: g.sentence.clone(),
        has_checksum: g.has_checksum,
        latitude: fill(g.latitude, b.latitude),
        longitude: fill(g.longitude, b.longitude),
        fix_valid: g.fix_valid.or(b.fix_valid),
        course_degrees: fill(g.course_degrees, b.course_degrees),
        speed_knots: fill(g.speed_knots, b.speed_knots),
        altitude_m: fill(g.altitude_m, b.altitude_m),
        time: g.time.clone().or_else(|| b.time.clone()),
        waypoint: g.waypoint.clone().or_else(|| b.waypoint.clone()),
        comment: g.comment.clone(),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Form {
    Plain,
    /// A Mic-E report, whose altitude is in `/A=` text (feet) or in the altitude bytes (metres).
    MicE {
        altitude_in_text: bool,
    },
}

fn mic_e(g: &MicEReport, b: &MicEReport) -> MicEReport {
    let mut e = g.clone();
    e.altitude_in_comment = b.altitude_in_comment;
    e.fields = positioned(&g.fields, &b.fields, Form::MicE { altitude_in_text: b.altitude_in_comment });
    e
}

/// The default compression type: a current fix, from another source, by software.
const DEFAULT_COMPRESSION: CompressionType =
    CompressionType { fix: CompressionFix::Current, source: CompressionSource::Other, origin: CompressionOrigin::Software };

/// The position, course, speed, altitude, range, frequency and weather of a positioned report as the
/// form shown by `b` holds them.
fn positioned(g: &Positioned, b: &Positioned, form: Form) -> Positioned {
    let mut e = g.clone();
    let cs = b.compressed && b.compression.is_some();
    e.position = position(g.position, b);
    // A value the form must carry where the data leaves it out: the default type on cs bytes that carry a value.
    if g.compression.is_none() && b.compression.is_some() {
        e.compression = Some(DEFAULT_COMPRESSION);
    }
    let mic_e = form != Form::Plain;
    if cs {
        // The cs bytes: a course in 4-degree steps (north is 360), a speed, a range and a GGA altitude in the steps of 1.08 or 1.002.
        e.course_degrees = g.course_degrees.map(|c| {
            let steps = round(c / 4.0) as i64 % 90;
            if steps == 0 { 360.0 } else { (steps * 4) as f64 }
        });
        e.speed_knots = g.speed_knots.map(compressed_speed);
        e.range_miles = g.range_miles.map(|r| 2.0 * libm::pow(1.08, round(libm::log(r / 2.0) / libm::log(1.08))));
    } else if mic_e {
        e.course_degrees = g.course_degrees.map(round);
        // Mic-E always carries a speed.
        e.speed_knots = Some(g.speed_knots.map_or(0.0, round));
        e.range_miles = g.range_miles.map(round);
    } else {
        e.course_degrees = g.course_degrees.map(round);
        e.speed_knots = g.speed_knots.map(round);
        e.range_miles = g.range_miles.map(round);
    }
    e.altitude_feet = g.altitude_feet.map(|feet| match form {
        Form::MicE { altitude_in_text: false } => round(feet / METRES_TO_FEET) * METRES_TO_FEET,
        _ if cs && b.compression.is_some_and(|t| t.source == CompressionSource::Gga) => {
            let cs_steps = if feet <= 1.0 { 0.0 } else { round(libm::log(feet) / libm::log(1.002)) };
            let carried = libm::pow(1.002, cs_steps);
            // A weather report has no comment, so its altitude is only what the cs bytes carry; any other
            // whose cs bytes carry it exactly writes no /A=, and else the /A= holds it in whole feet.
            if g.weather.is_some() || (carried - feet).abs() <= 1e-9 * feet.abs() { carried } else { round(feet) }
        }
        _ => round(feet),
    });
    if let Some(f) = &mut e.frequency {
        let per_mhz = if f.ten_khz_resolution { 100.0 } else { 1000.0 };
        f.mhz = round(f.mhz * per_mhz) / per_mhz;
    }
    if let Some(w) = &g.weather {
        e.weather = Some(weather(w, cs));
    }
    snap_positioned(&mut e, b);
    e
}

fn compressed_speed(knots: f64) -> f64 {
    libm::pow(1.08, round(libm::log(knots + 1.0) / libm::log(1.08))) - 1.0
}

/// Numbers within 1e-9 of what the decode holds are the decode's (the rounded value, up to the last bits).
fn snap_positioned(e: &mut Positioned, b: &Positioned) {
    e.position.latitude = snap(e.position.latitude, b.position.latitude);
    e.position.longitude = snap(e.position.longitude, b.position.longitude);
    e.course_degrees = snap_opt(e.course_degrees, b.course_degrees);
    e.speed_knots = snap_opt(e.speed_knots, b.speed_knots);
    e.altitude_feet = snap_opt(e.altitude_feet, b.altitude_feet);
    e.range_miles = snap_opt(e.range_miles, b.range_miles);
    if let (Some(f), Some(x)) = (&mut e.frequency, &b.frequency) {
        f.mhz = snap(f.mhz, x.mhz);
    }
    if let (Some(w), Some(x)) = (&mut e.weather, &b.weather) {
        snap_weather(w, x);
    }
}

fn snap_weather(e: &mut Weather, b: &Weather) {
    e.wind_direction_degrees = snap_opt(e.wind_direction_degrees, b.wind_direction_degrees);
    e.wind_speed_mph = snap_opt(e.wind_speed_mph, b.wind_speed_mph);
    e.wind_gust_mph = snap_opt(e.wind_gust_mph, b.wind_gust_mph);
    e.temperature_f = snap_opt(e.temperature_f, b.temperature_f);
    e.rain_1h_in = snap_opt(e.rain_1h_in, b.rain_1h_in);
    e.rain_24h_in = snap_opt(e.rain_24h_in, b.rain_24h_in);
    e.rain_midnight_in = snap_opt(e.rain_midnight_in, b.rain_midnight_in);
    e.humidity_percent = snap_opt(e.humidity_percent, b.humidity_percent);
    e.pressure_mbar = snap_opt(e.pressure_mbar, b.pressure_mbar);
    e.luminosity_w_m2 = snap_opt(e.luminosity_w_m2, b.luminosity_w_m2);
    e.snow_24h_in = snap_opt(e.snow_24h_in, b.snow_24h_in);
}

/// The position as the form shown by `b` holds it: the step of a compressed position, the centre of
/// the box an ambiguous one blanks, or hundredths of a minute (thousandths, or base-91 steps, with a `!DAO!`).
fn position(g: Position, b: &Positioned) -> Position {
    let latitude = coordinate(g.latitude, g.ambiguity, b, 90.0, 380_926.0);
    let longitude = coordinate(g.longitude, g.ambiguity, b, 180.0, 190_463.0);
    Position { latitude, longitude, ambiguity: g.ambiguity }
}

fn coordinate(degrees: f64, ambiguity: u8, b: &Positioned, limit: f64, compressed_per_degree: f64) -> f64 {
    if b.compressed {
        let steps = round(compressed_per_degree * if limit == 90.0 { 90.0 - degrees } else { 180.0 + degrees });
        let steps = steps.min(91.0 * 91.0 * 91.0 * 91.0 - 1.0);
        return if limit == 90.0 { 90.0 - steps / compressed_per_degree } else { -180.0 + steps / compressed_per_degree };
    }
    if (1..=4).contains(&ambiguity) {
        // The box the value is in (its digits cut, not rounded), then its centre.
        let size = [1.0, 10.0, 100.0, 1000.0, 6000.0][usize::from(ambiguity)];
        let start = libm::floor(degrees.abs() * 6000.0 / size + 1e-6) * size;
        let centre = (start + size / 2.0) / 6000.0;
        return if degrees.is_sign_negative() { -centre } else { centre };
    }
    let per_minute = match b.dao.map(|d| d.precision) {
        Some(DaoPrecision::Thousandths) => 1000.0,
        Some(DaoPrecision::Base91) => 9100.0,
        _ => 100.0,
    };
    let value = round(degrees.abs() * 60.0 * per_minute) / (60.0 * per_minute);
    if degrees.is_sign_negative() { -value } else { value }
}

/// Weather as the form shown holds it. `cs` says the wind is in a compressed report's cs bytes.
fn weather(g: &Weather, cs: bool) -> Weather {
    let mut e = g.clone();
    if cs {
        e.wind_direction_degrees = g.wind_direction_degrees.map(|d| (round(d / 4.0) as i64 % 90 * 4) as f64);
        e.wind_speed_mph = g.wind_speed_mph.map(|mph| compressed_speed(mph / KNOTS_TO_MPH) * KNOTS_TO_MPH);
    } else {
        e.wind_direction_degrees = g.wind_direction_degrees.map(round);
        e.wind_speed_mph = g.wind_speed_mph.map(round);
    }
    e.wind_gust_mph = g.wind_gust_mph.map(round);
    e.temperature_f = g.temperature_f.map(round);
    let hundredths = |v: Option<f64>| v.map(|v| round(v * 100.0) / 100.0);
    e.rain_1h_in = hundredths(g.rain_1h_in);
    e.rain_24h_in = hundredths(g.rain_24h_in);
    e.rain_midnight_in = hundredths(g.rain_midnight_in);
    e.humidity_percent = g.humidity_percent.map(round);
    e.pressure_mbar = g.pressure_mbar.map(|v| round(v * 10.0) / 10.0);
    e.luminosity_w_m2 = g.luminosity_w_m2.map(round);
    e.snow_24h_in = g.snow_24h_in.map(snowfall);
    e.unit = absent(&g.unit);
    e
}

/// Snowfall in its three forms: hundredths under 1 inch, tenths under 10, else whole inches.
fn snowfall(inches: f64) -> f64 {
    let hundredths = round(inches * 100.0);
    let tenths = round(inches * 10.0);
    if hundredths < 100.0 {
        hundredths / 100.0
    } else if tenths < 100.0 {
        tenths / 10.0
    } else {
        round(inches)
    }
}

// ------------------------------------------------------------------ why a form did not match

/// The free text of data: what a decode that changes it has read as something else.
fn free_text(data: &Data, out: &mut Vec<String>) {
    let positioned = |p: &Positioned, out: &mut Vec<String>| {
        out.push(p.comment.clone());
        if let Some(w) = &p.weather {
            weather_text(w, out);
        }
    };
    match data {
        Data::Position(r) => positioned(&r.fields, out),
        Data::Object(r) => positioned(&r.fields, out),
        Data::Item(r) => positioned(&r.fields, out),
        Data::MicE(r) => {
            positioned(&r.fields, out);
            out.push(r.device_suffix.clone());
        }
        Data::Message(m) => out.push(m.text.clone()),
        Data::Bulletin(b) | Data::NwsBulletin(b) => out.push(b.text.clone()),
        Data::TelemetryNames(t) => out.extend(t.names.iter().cloned()),
        Data::TelemetryUnits(t) => out.extend(t.units.iter().cloned()),
        Data::TelemetryBits(t) => out.push(t.project.clone()),
        Data::Status(s) => out.push(s.text.clone()),
        Data::Telemetry(t) => out.push(t.comment.clone()),
        Data::Weather(w) => {
            weather_text(&w.weather, out);
            out.push(w.comment.clone());
        }
        Data::Nmea(n) => out.push(n.comment.clone()),
        Data::MaidenheadBeacon(m) => out.push(m.comment.clone()),
        Data::Capabilities(c) => out.extend(c.capabilities.iter().filter_map(|(_, v)| v.clone())),
        Data::UserDefined(u) => out.push(u.data.iter().map(|&b| b as char).collect()),
        Data::Test(t) => out.push(t.data.clone()),
        Data::ThirdParty(p) => free_text(&p.data, out),
        _ => {}
    }
}

fn weather_text(w: &Weather, out: &mut Vec<String>) {
    out.extend(w.software.map(|c| c.to_string()));
    out.extend(w.unit.clone());
    out.extend(w.extra.iter().map(|f| format!("{}{}", f.letter, f.value)));
}

/// Whether the data has free text that the decode changed or read as something else. A decode that
/// has a diagnostic where the data has free text counts too.
fn text_changed(given: &Data, back: &Data, diagnosed: bool) -> bool {
    let (mut a, mut b) = (Vec::new(), Vec::new());
    free_text(given, &mut a);
    free_text(back, &mut b);
    let has_text = a.iter().any(|t| !t.is_empty());
    if core::mem::discriminant(given) != core::mem::discriminant(back) || diagnosed {
        return has_text;
    }
    a != b
}

/// The first field that differs, for the refusal's message.
fn first_difference(given: &Data, back: &Data) -> &'static str {
    let (g, b) = match (given, back) {
        (Data::Position(g), Data::Position(b)) => (&g.fields, &b.fields),
        (Data::Object(g), Data::Object(b)) => (&g.fields, &b.fields),
        (Data::Item(g), Data::Item(b)) => (&g.fields, &b.fields),
        (Data::MicE(g), Data::MicE(b)) => (&g.fields, &b.fields),
        _ => return if core::mem::discriminant(given) == core::mem::discriminant(back) { "data" } else { "report type" },
    };
    let checks = [
        (g.position != b.position, "position"),
        (g.comment != b.comment, "comment"),
        (g.altitude_feet != b.altitude_feet, "altitude"),
        (g.phg != b.phg, "PHG"),
        (g.dfs != b.dfs, "DFS"),
        (g.range_miles != b.range_miles, "range"),
        (g.area != b.area, "area"),
        (g.df_bearing != b.df_bearing, "DF bearing"),
        (g.storm != b.storm, "storm data"),
        (g.telemetry != b.telemetry, "telemetry"),
        (g.frequency != b.frequency, "frequency"),
        (g.signpost != b.signpost, "signpost"),
        (g.dao != b.dao, "DAO"),
        (g.weather != b.weather, "weather"),
        (g.course_degrees != b.course_degrees || g.speed_knots != b.speed_knots, "course or speed"),
    ];
    checks.iter().find(|(differs, _)| *differs).map_or("data", |(_, what)| what)
}

// ------------------------------------------------------------------ the header

/// What a header says, in the neutral header form: the addresses and each path entry with whether it is used.
type Header = (Address, Address, Vec<(Address, bool)>);

/// The header as given: each entry used or not as it says.
fn header_given(source: &Address, destination: &Address, path: &[crate::PathEntry]) -> Header {
    (source.clone(), destination.clone(), path.iter().map(|e| (e.address.clone(), e.used)).collect())
}

/// The header as read: an entry is used when it is marked, or comes before one that is.
fn header_read(source: &Address, destination: &Address, path: &[crate::PathEntry]) -> Header {
    let last = path.iter().rposition(|e| e.used);
    (source.clone(), destination.clone(), path.iter().enumerate().map(|(i, e)| (e.address.clone(), last.is_some_and(|l| i <= l))).collect())
}

/// Whether the path has an entry not marked used before one that is: it reads back with that entry used.
fn used_after_unused(path: &[crate::PathEntry]) -> bool {
    path.iter().rposition(|e| e.used).is_some_and(|last| path[..last].iter().any(|e| !e.used))
}

/// The TNC2 header `SOURCE>DEST,PATH:`, with `*` on the last used entry only, once it has been read
/// back: the same source, destination and path with each used entry used, and no diagnostic.
pub(crate) fn tnc2_header(
    source: &Address,
    destination: &Address,
    path: &[crate::PathEntry],
    third_party: bool,
) -> Result<Vec<u8>, EncodeError> {
    let mut out = Vec::with_capacity(24 + 11 * path.len());
    out.extend_from_slice(source.as_str().as_bytes());
    out.push(b'>');
    out.extend_from_slice(destination.as_str().as_bytes());
    let last_used = path.iter().rposition(|e| e.used);
    for (i, entry) in path.iter().enumerate() {
        out.push(b',');
        out.extend_from_slice(entry.address.as_str().as_bytes());
        if Some(i) == last_used {
            out.push(b'*');
        }
    }
    out.push(b':');
    let mut ctx = Context::new(ParseOptions::LENIENT);
    let read = crate::packet::tnc2_header(&mut ctx, &out, third_party);
    let reads_back = match read {
        // No diagnostic, but the one an empty destination in the header given (a decoded packet's, tolerated on the way in) has again.
        Ok(((s, d, p), _)) => {
            let own = |c: Code| c == Code::EmptyDestination && destination.as_str().is_empty();
            ctx.diagnostics.iter().all(|d| own(d.code)) && header_read(&s, &d, &p) == header_given(source, destination, path)
        }
        Err(_) => false,
    };
    if reads_back {
        return Ok(out);
    }
    Err(if used_after_unused(path) {
        EncodeError::new(
            RefusalCode::Tnc2CannotCarry,
            "a path entry not marked used before one that is cannot be written in a TNC2 line: it reads back used",
        )
    } else {
        EncodeError::new(
            RefusalCode::InvalidAddress,
            "the header does not read back as given: a header address is not 1-9 letters, digits or -",
        )
    })
}

/// The AX.25 addresses, destination first, with the H bit on every used entry, once they have been
/// read back: the same source, destination and path with each used entry used, and no diagnostic.
pub(crate) fn ax25_header(
    source: &Address,
    destination: &Address,
    path: &[crate::PathEntry],
    write: impl Fn(&mut Vec<u8>, &Address, u8) -> Result<(), EncodeError>,
) -> Result<Vec<u8>, EncodeError> {
    if path.len() > 8 {
        return Err(EncodeError::new(RefusalCode::Ax25CannotCarry, "an AX.25 frame has room for at most 8 digipeaters"));
    }
    let mut out = Vec::with_capacity(16 + 7 * path.len());
    if destination.as_str().is_empty() {
        // An empty destination a held packet carries is six spaces, and reads back empty with its diagnostic.
        out.extend_from_slice(&[0x40; 6]);
        out.push(0xE0);
    } else {
        write(&mut out, destination, 0xE0)?;
    }
    write(&mut out, source, 0x60)?;
    for entry in path {
        write(&mut out, &entry.address, if entry.used { 0xE0 } else { 0x60 })?;
    }
    let last = out.len() - 1;
    out[last] |= 1;
    out.push(0x03);
    out.push(0xF0);
    let mut ctx = Context::new(ParseOptions::LENIENT);
    let reads_back = match crate::packet::ax25_header(&mut ctx, &out) {
        Ok(((s, d, p), _)) => {
            // An entry is used when its H bit is set or it comes before one that is.
            let own = |c: Code| c == Code::EmptyDestination && destination.as_str().is_empty();
            ctx.diagnostics.iter().all(|d| own(d.code)) && header_read(&s, &d, &p) == header_given(source, destination, path)
        }
        Err(_) => false,
    };
    if reads_back {
        return Ok(out);
    }
    Err(if used_after_unused(path) {
        EncodeError::new(
            RefusalCode::Ax25CannotCarry,
            "a path entry not marked used before one that is cannot be written in an AX.25 frame: it reads back used",
        )
    } else {
        EncodeError::new(RefusalCode::Ax25CannotCarry, "the addresses do not read back as given")
    })
}

#[cfg(test)]
mod tests {
    //! The read-back covers every report type: for each, the data a decoder gives for a packet, and
    //! the bytes of that packet with one field's byte changed (a writer's mistake). The right bytes
    //! seal; the wrong ones are refused. A type the read-back skipped would let its wrong bytes through.

    use alloc::vec::Vec;

    use super::{Forms, seal};
    use crate::{Address, Data, Packet, ParseOptions, RefusalCode};

    /// (what, the TNC2 line, the same information field with one byte changed).
    const SAMPLES: &[(&str, &str, &str)] = &[
        ("position", "N0CALL>APZ001:!5127.26N/00058.69W>088/036Mobile", "!5127.26N/00058.69W>088/037Mobile"),
        ("position, compressed", "N0CALL>APZ001:!/5L!!<*e7>7P[", "!/5L!!<*e7>7P\\"),
        ("position, weather", "N0CALL>APZ001:!5127.00N/00058.80W_220/004g005t077", "!5127.00N/00058.80W_220/004g005t078"),
        (
            "object",
            "N0CALL>APZ001:;MYRPTR   *251830z5127.00N/00058.80Wr145.725MHz T118 -060",
            ";MYRPTR   *251830z5127.00N/00058.81Wr145.725MHz T118 -060",
        ),
        ("item", "N0CALL>APZ001:)MARKER!4903.50N/07201.75W-hello", ")MARKER!4903.50N/07201.75W-hellp"),
        ("message", "N0CALL>APZ001::G3NRW    :Hi Ian{01", ":G3NRW    :Hi Ibn{01"),
        ("ack", "N0CALL>APZ001::G3NRW    :ack01", ":G3NRW    :ack02"),
        ("reject", "N0CALL>APZ001::G3NRW    :rej01", ":G3NRW    :rej02"),
        ("bulletin", "N0CALL>APZ001::BLN1     :weather alert", ":BLN1     :weather alerx"),
        ("NWS bulletin", "N0CALL>APZ001::NWS-12345:storm", ":NWS-12345:storn"),
        ("telemetry names", "N0CALL>APZ001::N0CALL   :PARM.Volts,Amps", ":N0CALL   :PARM.Volts,Ampz"),
        ("telemetry units", "N0CALL>APZ001::N0CALL   :UNIT.V,A", ":N0CALL   :UNIT.V,B"),
        ("telemetry coefficients", "N0CALL>APZ001::N0CALL   :EQNS.0,1,0,0,2,0", ":N0CALL   :EQNS.0,1,0,0,3,0"),
        ("telemetry bits", "N0CALL>APZ001::N0CALL   :BITS.11111111,Project", ":N0CALL   :BITS.11111110,Project"),
        ("directed query", "N0CALL>APZ001::G3NRW    :?APRSP", ":G3NRW    :?APRSD"),
        ("status", "N0CALL>APZ001:>hello", ">hellp"),
        ("telemetry", "N0CALL>APZ001:T#005,199,000,255,073,123,01101001", "T#005,199,000,255,073,123,01101000"),
        (
            "positionless weather",
            "N0CALL>APZ001:_10090556c220s004g005t077r001p002P003h50b09720",
            "_10090556c220s004g005t077r001p002P003h50b09721",
        ),
        ("raw weather", "N0CALL>APZ001:#abc", "#abd"),
        ("NMEA", "N0CALL>APZ001:$GPGLL,4916.45,N,12311.12,W,225444,A", "$GPGLL,4916.45,N,12311.12,W,225444,V"),
        ("Maidenhead beacon", "N0CALL>APZ001:[IO91SX]hi", "[IO91SY]hi"),
        ("query", "N0CALL>APZ001:?APRS?", "?IGATE?"),
        ("query with a footprint", "N0CALL>APZ001:?APRS?34.00,-118.00,0100", "?APRS?34.00,-118.00,0101"),
        ("capabilities", "N0CALL>APZ001:<IGATE,MSG_CNT=30", "<IGATE,MSG_CNT=31"),
        ("third-party", "N0CALL>APZ001:}G4ABC>APZ001,TCPIP,N0CALL*:>hi", "}G4ABC>APZ001,TCPIP,N0CALL*:>hj"),
        ("user-defined", "N0CALL>APZ001:{AB123", "{AB124"),
        ("test", "N0CALL>APZ001:,hello", ",hellp"),
        ("Agrelo DF", "N0CALL>APZ001:%123/4", "%123/5"),
    ];

    /// Mic-E, which needs its destination: the same, with the packet made by `Packet::create`.
    fn mic_e() -> (Data, Address, Vec<u8>) {
        let line = b"N0CALL>S32UVT:`(_fn\"Oj/`IO91/G|!:&0'p| DG2MM";
        let packet = Packet::decode_tnc2(line, ParseOptions::LENIENT).unwrap();
        let sealed = packet.data.seal().expect("the data of a decoded Mic-E report seals");
        let (destination, bytes) = sealed.into_parts();
        (packet.data, destination.unwrap(), bytes)
    }

    #[test]
    fn every_report_type_is_read_back() {
        for (what, line, wrong) in SAMPLES {
            let packet = Packet::decode_tnc2(line.as_bytes(), ParseOptions::LENIENT).unwrap();
            let right = &line.as_bytes()[line.find(':').unwrap() + 1..];
            let good =
                seal(&packet.data, Forms::new(right.to_vec())).unwrap_or_else(|e| panic!("{what}: the right bytes are refused: {e}"));
            assert_eq!(good.into_bytes(), right, "{what}");
            let refused = seal(&packet.data, Forms::new(wrong.as_bytes().to_vec()));
            let refused = refused.err().unwrap_or_else(|| panic!("{what}: wrong bytes {wrong:?} were let through"));
            assert!(
                matches!(refused.code(), RefusalCode::DoesNotReadBack | RefusalCode::TextReadsAsField),
                "{what}: refused as {} not by the read-back",
                refused.code()
            );
        }
    }

    #[test]
    fn mic_e_is_read_back_with_the_destination_computed() {
        let (data, destination, bytes) = mic_e();
        // The destination is part of what is read: the latitude digits are in it.
        let elsewhere = Address::new("S32UVU").unwrap();
        assert!(seal(&data, Forms::new(bytes.clone()).to(elsewhere)).is_err());
        let mut wrong = bytes.clone();
        wrong[4] += 1;
        assert!(seal(&data, Forms::new(wrong).to(destination.clone())).is_err(), "a wrong speed byte");
        assert!(seal(&data, Forms::new(bytes).to(destination)).is_ok());
    }

    #[test]
    fn a_form_that_fails_is_skipped_for_the_next_and_none_means_a_refusal() {
        let packet = Packet::decode_tnc2(b"N0CALL>APZ001:>hello", ParseOptions::LENIENT).unwrap();
        let sealed = seal(&packet.data, Forms::new(b">hellp".to_vec()).then(b">hello".to_vec())).unwrap();
        assert_eq!(sealed.into_bytes(), b">hello");
        let none = seal(&packet.data, Forms::new(b">hellp".to_vec()).then(b">hellq".to_vec()));
        assert!(none.is_err());
    }

    #[test]
    fn bytes_over_256_are_read_back_before_they_are_measured() {
        let packet = Packet::decode_tnc2(b"N0CALL>APZ001:>hello", ParseOptions::LENIENT).unwrap();
        // Wrong and over 256: the read-back's code, never the size's.
        let mut wrong = alloc::vec![b'>'];
        wrong.extend(core::iter::repeat_n(b'x', 300));
        assert_eq!(seal(&packet.data, Forms::new(wrong)).err().unwrap().code(), RefusalCode::TextReadsAsField);
        // Right and over 256: the size's.
        let big = Data::Status(crate::Status { text: "x".repeat(300), ..Default::default() });
        let mut right = alloc::vec![b'>'];
        right.extend(core::iter::repeat_n(b'x', 300));
        assert_eq!(seal(&big, Forms::new(right)).err().unwrap().code(), RefusalCode::InformationFieldTooLong);
    }
}
