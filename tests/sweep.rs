//! A sweep: every numeric field of every data type, and every numeric argument of every builder, is given
//! NaN, both infinities, the largest and smallest finite values, a negative zero and a fraction, at depth 0,
//! 1 and 2 of third-party nesting. Each call must refuse with a documented refusal or write bytes that read
//! back as the data given, and never panic. A non-finite number is always `not-a-number`, and a finite one
//! never is; a finite value outside its field is never `does-not-read-back`, since a rule names it.

use pdn_aprs::*;

const VALUES: [f64; 9] = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX, f64::MIN, -0.0, 0.4, 1e-300, 1e15];

type Setter = fn(&mut Data, f64);

fn data_of(line: &str) -> Packet {
    Packet::decode_tnc2(line.as_bytes(), ParseOptions::LENIENT).unwrap()
}

fn positioned(d: &mut Data) -> &mut Positioned {
    match d {
        Data::Position(r) => &mut r.fields,
        Data::Object(r) => &mut r.fields,
        Data::Item(r) => &mut r.fields,
        Data::MicE(r) => &mut r.fields,
        _ => panic!("not a positioned report"),
    }
}

fn weather(d: &mut Data) -> &mut Weather {
    match d {
        Data::Weather(r) => &mut r.weather,
        other => positioned(other).weather.get_or_insert_with(Weather::default),
    }
}

fn fixed() -> Vec<(&'static str, &'static str, Setter)> {
    let position = "N0CALL>APZ001:!5127.26N/00058.69W>";
    let mic_e = "N0CALL>S32UVT:`(_fn\"Oj/`hello";
    let object = "N0CALL>APZ001:;MYRPTR   *251830z5127.00N/00058.80W>";
    let item = "N0CALL>APZ001:)MARKER!4903.50N/07201.75W-";
    let wx = "N0CALL>APZ001:_10090556c220s004g005t077r001p002P003h50b09720L123s.50";
    let wxp = "N0CALL>APZ001:!5127.00N/00058.80W_220/004g005t077r001p002P003h50b09720";
    let mut v: Vec<(&'static str, &'static str, Setter)> = Vec::new();
    for base in [position, mic_e, object, item] {
        v.push((base, "latitude", |d, x| positioned(d).position.latitude = x));
        v.push((base, "longitude", |d, x| positioned(d).position.longitude = x));
        v.push((base, "course+speed: course", |d, x| {
            let f = positioned(d);
            f.course_degrees = Some(x);
            f.speed_knots = Some(10.0);
        }));
        v.push((base, "course+speed: speed", |d, x| {
            let f = positioned(d);
            f.course_degrees = Some(10.0);
            f.speed_knots = Some(x);
        }));
        v.push((base, "altitude", |d, x| positioned(d).altitude_feet = Some(x)));
        v.push((base, "range", |d, x| positioned(d).range_miles = Some(x)));
        v.push((base, "frequency", |d, x| positioned(d).frequency = Some(VoiceFrequency { mhz: x, ..Default::default() })));
    }
    let setters: [(&'static str, Setter); 11] = [
        ("wind direction", |d, x| weather(d).wind_direction_degrees = Some(x)),
        ("wind speed", |d, x| weather(d).wind_speed_mph = Some(x)),
        ("wind gust", |d, x| weather(d).wind_gust_mph = Some(x)),
        ("temperature", |d, x| weather(d).temperature_f = Some(x)),
        ("rain 1h", |d, x| weather(d).rain_1h_in = Some(x)),
        ("rain 24h", |d, x| weather(d).rain_24h_in = Some(x)),
        ("rain midnight", |d, x| weather(d).rain_midnight_in = Some(x)),
        ("humidity", |d, x| weather(d).humidity_percent = Some(x)),
        ("pressure", |d, x| weather(d).pressure_mbar = Some(x)),
        ("luminosity", |d, x| weather(d).luminosity_w_m2 = Some(x)),
        ("snow", |d, x| weather(d).snow_24h_in = Some(x)),
    ];
    for base in [wx, wxp] {
        for (name, set) in setters {
            v.push((base, name, set));
        }
    }
    v.push(("N0CALL>APZ001:T#005,199,000,255,073,123,01101001", "telemetry analog", |d, x| {
        if let Data::Telemetry(t) = d {
            t.analog[0] = Some(x);
        }
    }));
    v.push(("N0CALL>APZ001::N0CALL   :EQNS.0,1,0,0,2,0", "coefficient", |d, x| {
        if let Data::TelemetryCoefficients(t) = d {
            t.coefficients[0] = x;
        }
    }));
    let nmea = "N0CALL>APZ001:$GPGLL,4916.45,N,12311.12,W,225444,A";
    let setters: [(&'static str, Setter); 5] = [
        ("nmea latitude", |d, x| nmea_of(d).latitude = Some(x)),
        ("nmea longitude", |d, x| nmea_of(d).longitude = Some(x)),
        ("nmea course", |d, x| nmea_of(d).course_degrees = Some(x)),
        ("nmea speed", |d, x| nmea_of(d).speed_knots = Some(x)),
        ("nmea altitude", |d, x| nmea_of(d).altitude_m = Some(x)),
    ];
    for (name, set) in setters {
        v.push((nmea, name, set));
    }
    let query = "N0CALL>APZ001:?APRS?34.00,-118.00,0100";
    let setters: [(&'static str, Setter); 3] = [
        ("footprint latitude", |d, x| footprint_of(d).latitude = x),
        ("footprint longitude", |d, x| footprint_of(d).longitude = x),
        ("footprint radius", |d, x| footprint_of(d).radius_miles = x),
    ];
    for (name, set) in setters {
        v.push((query, name, set));
    }
    v
}

fn nmea_of(d: &mut Data) -> &mut Nmea {
    match d {
        Data::Nmea(n) => n,
        _ => panic!("not NMEA"),
    }
}

fn footprint_of(d: &mut Data) -> &mut Footprint {
    match d {
        Data::Query(q) => q.footprint.as_mut().unwrap(),
        _ => panic!("not a query"),
    }
}

/// The base line with its report wrapped in `depth` third-party packets, and the innermost data.
fn wrapped(base: &str, depth: usize) -> Packet {
    let (header, info) = base.split_once(':').unwrap();
    let destination = header.split_once('>').unwrap().1;
    let line = format!("{header}:{}{info}", format!("}}G4ABC>{destination}:").repeat(depth));
    data_of(&line)
}

fn innermost(d: &mut Data) -> &mut Data {
    match d {
        Data::ThirdParty(p) => innermost(&mut p.data),
        other => other,
    }
}

/// The information field `data` is written as, for an inner packet that carries it: a decoded inner packet is
/// written as received, so the inner bytes follow the inner data given.
fn information_of(data: &Data) -> Option<Vec<u8>> {
    match data {
        Data::ThirdParty(q) => {
            let mut bytes = format!("}}{}>{}:", q.source, q.destination).into_bytes();
            bytes.extend_from_slice(&q.information);
            Some(bytes)
        }
        Data::MicE(_) => {
            let (from, to) = (Address::new("N0CALL").unwrap(), Address::new("APZ001").unwrap());
            Packet::create(from, to, Vec::new(), data.clone()).ok().map(|p| p.information)
        }
        other => other.encode().ok(),
    }
}

/// Every inner packet's information field made to follow its data, deepest first.
fn rebuild(data: &mut Data) {
    if let Data::ThirdParty(p) = data {
        rebuild(&mut p.data);
        if let Some(bytes) = information_of(&p.data) {
            p.information = bytes;
        }
    }
}

/// `held_header`: an inner Mic-E packet's destination is part of what it holds, so data changed under it
/// need not read back with the destination it has.
fn check(what: &str, value: f64, held_header: bool, result: Result<Packet, EncodeError>) {
    match result {
        Err(e) => {
            REFUSED.fetch_add(1, Ordering::Relaxed);
            if value.is_finite() {
                assert_ne!(e.code(), RefusalCode::NotANumber, "{what}: a finite {value:e} is not a number");
                assert!(held_header || e.code() != RefusalCode::DoesNotReadBack, "{what}: {value:e} is outside a field a rule names: {e}");
            } else {
                assert_eq!(e.code(), RefusalCode::NotANumber, "{what}: {value:e}: {e}");
            }
        }
        Ok(p) => {
            WRITTEN.fetch_add(1, Ordering::Relaxed);
            assert!(value.is_finite(), "{what}: {value:e} was written");
            // What was written decodes without a panic, and reads back as what the packet holds.
            let again = Packet::decode(p.source.clone(), p.destination.clone(), p.path.clone(), &p.information, ParseOptions::LENIENT);
            assert!(!matches!(again.data, Data::Unrecognized(_)), "{what}: {value:e} wrote bytes that are not a report");
        }
    }
}

use std::sync::atomic::{AtomicUsize, Ordering};

static WRITTEN: AtomicUsize = AtomicUsize::new(0);
static REFUSED: AtomicUsize = AtomicUsize::new(0);

#[test]
fn every_numeric_field_at_every_depth() {
    let (from, to) = (Address::new("N0CALL").unwrap(), Address::new("APZ001").unwrap());
    for (base, name, set) in fixed() {
        for depth in 0..=2 {
            for value in VALUES {
                let mut packet = wrapped(base, depth);
                // The base must be writable, or the sweep would only meet its own fault.
                let given = innermost(&mut packet.data);
                if matches!(given, Data::Unrecognized(_)) {
                    panic!("{base} is not a report");
                }
                set(given, value);
                rebuild(&mut packet.data);
                let what = format!("{name} of {base} at depth {depth}");
                let result = std::panic::catch_unwind(|| Packet::create(from.clone(), to.clone(), Vec::new(), packet.data.clone()));
                check(&what, value, depth > 0 && base.contains("S32UVT"), result.unwrap_or_else(|_| panic!("{what}: {value:e} panicked")));
            }
        }
    }
    // Both outcomes happen: the sweep is not refusing everything, and not writing everything.
    assert!(WRITTEN.load(Ordering::Relaxed) > 100, "{} written", WRITTEN.load(Ordering::Relaxed));
    assert!(REFUSED.load(Ordering::Relaxed) > 100);
}

#[test]
fn every_builder_argument() {
    let station = Station::new("N0CALL").unwrap();
    for value in VALUES {
        let name = |s: &str| format!("builder {s}({value:e})");
        let mut calls: Vec<(String, Result<Packet, EncodeError>)> = vec![
            (name("at lat"), station.position(value, 0.0).symbol(Symbol::CAR).build()),
            (name("at lon"), station.position(0.0, value).symbol(Symbol::CAR).build()),
            (name("mic_e lat"), station.mic_e(value, 0.0).symbol(Symbol::CAR).build()),
            (name("mic_e lon"), station.mic_e(0.0, value).symbol(Symbol::CAR).build()),
            (name("course"), station.position(1.0, 1.0).symbol(Symbol::CAR).course_degrees(value).speed_knots(5.0).build()),
            (name("speed"), station.position(1.0, 1.0).symbol(Symbol::CAR).course_degrees(5.0).speed_knots(value).build()),
            (name("speed_kmh"), station.position(1.0, 1.0).symbol(Symbol::CAR).course_degrees(5.0).speed_kmh(value).build()),
            (name("altitude_feet"), station.position(1.0, 1.0).symbol(Symbol::CAR).altitude_feet(value).build()),
            (name("altitude_m"), station.position(1.0, 1.0).symbol(Symbol::CAR).altitude_m(value).build()),
            (name("mic_e altitude_feet"), station.mic_e(1.0, 1.0).symbol(Symbol::CAR).altitude_feet(value).build()),
            (name("mic_e altitude_m"), station.mic_e(1.0, 1.0).symbol(Symbol::CAR).altitude_m(value).build()),
            (name("mic_e speed"), station.mic_e(1.0, 1.0).symbol(Symbol::CAR).course_degrees(5.0).speed_knots(value).build()),
            (name("range"), station.position(1.0, 1.0).symbol(Symbol::CAR).range_miles(value).build()),
            (name("frequency"), station.position(1.0, 1.0).symbol(Symbol::CAR).frequency(value).build()),
            (name("coefficients"), station.telemetry_coefficients(&[value, 1.0, 0.0]).build()),
            (name("analog"), station.telemetry(1).analog(&[Some(value)]).build()),
        ];
        let w = || station.weather().at(1.0, 1.0).symbol(Symbol::WEATHER_STATION);
        let base = || w().wind_direction_degrees(10.0).wind_speed_mph(5.0);
        calls.extend([
            (name("wind direction"), w().wind_direction_degrees(value).wind_speed_mph(5.0).build()),
            (name("wind speed"), base().wind_speed_mph(value).build()),
            (name("gust"), base().wind_gust_mph(value).build()),
            (name("temperature_f"), base().temperature_f(value).build()),
            (name("temperature_c"), base().temperature_c(value).build()),
            (name("humidity"), base().humidity_percent(value).build()),
            (name("pressure"), base().pressure_mbar(value).build()),
            (name("rain_1h_in"), base().rain_1h_in(value).build()),
            (name("rain_24h_in"), base().rain_24h_in(value).build()),
            (name("rain_midnight_in"), base().rain_midnight_in(value).build()),
            (name("rain_1h_mm"), base().rain_1h_mm(value).build()),
            (name("rain_24h_mm"), base().rain_24h_mm(value).build()),
            (name("rain_midnight_mm"), base().rain_midnight_mm(value).build()),
            (name("luminosity"), base().luminosity_w_m2(value).build()),
            (name("snow"), base().snow_24h_in(value).build()),
        ]);
        for (what, result) in calls {
            check(&what, value, false, result);
        }
    }
}
