//! The vectors' neutral data form: writing decoded data (for comparison) and reading it back (to
//! build data for the encode cases). The rules are in vectors/README.md.

use packet_aprs::*;
use serde_json::{Map, Value, json};

pub fn diagnostic(d: &Diagnostic) -> String {
    format!("{}:{}", d.severity.name(), d.code.id())
}

pub fn diagnostics(ds: &[Diagnostic]) -> Vec<String> {
    ds.iter().map(diagnostic).collect()
}

pub fn header(p: &Packet) -> Value {
    let mut o = Map::new();
    o.insert("source".into(), json!(p.source.as_str()));
    o.insert("destination".into(), json!(p.destination.as_str()));
    if !p.path.is_empty() {
        o.insert("path".into(), path(&p.path));
    }
    if let Some(q) = p.q_construct() {
        let mut qo = Map::new();
        qo.insert("construct".into(), json!(q.construct));
        if let Some(s) = q.station {
            qo.insert("station".into(), json!(s.as_str()));
        }
        o.insert("q_construct".into(), Value::Object(qo));
    }
    Value::Object(o)
}

fn path(path: &[PathEntry]) -> Value {
    Value::Array(path.iter().map(|e| json!(format!("{}{}", e.address, if e.used { "*" } else { "" }))).collect())
}

/// Writes a key only when it has something to say: no nulls, empty strings or empty lists, and
/// booleans only when true.
struct Obj(Map<String, Value>);

impl Obj {
    fn new(kind: &str) -> Obj {
        let mut m = Map::new();
        if !kind.is_empty() {
            m.insert("type".into(), json!(kind));
        }
        Obj(m)
    }
    fn put(&mut self, k: &str, v: Value) -> &mut Obj {
        let keep = match &v {
            Value::Null => false,
            Value::String(s) => !s.is_empty(),
            Value::Array(a) => !a.is_empty(),
            Value::Bool(b) => *b,
            _ => true,
        };
        if keep {
            self.0.insert(k.into(), v);
        }
        self
    }
    fn opt<T: Into<Value>>(&mut self, k: &str, v: Option<T>) -> &mut Obj {
        if let Some(v) = v {
            self.0.insert(k.into(), v.into());
        }
        self
    }
    fn done(&mut self) -> Value {
        Value::Object(std::mem::take(&mut self.0))
    }
}

fn kebab(name: &str) -> String {
    let mut out = String::new();
    for (i, ch) in name.chars().enumerate() {
        if ch.is_ascii_uppercase() && i > 0 {
            out.push('-');
        }
        out.push(ch.to_ascii_lowercase());
    }
    out
}

fn enum_name<T: std::fmt::Debug>(v: T) -> String {
    kebab(&format!("{v:?}"))
}

fn timestamp(t: &Option<Timestamp>) -> Value {
    t.map_or(Value::Null, |t| json!(String::from_utf8(t.to_bytes()).unwrap()))
}

fn symbol(s: Symbol) -> String {
    format!("{}{}", s.table, s.code)
}

pub fn data(d: &Data) -> Value {
    match d {
        Data::Position(p) => {
            let mut o = Obj::new("position");
            o.put("timestamp", timestamp(&p.timestamp)).put("messaging", json!(p.messaging));
            positioned(&mut o, &p.fields);
            o.done()
        }
        Data::MicE(m) => {
            let mut o = Obj::new("mic-e");
            o.put("mic_e_message", json!(mic_e_message(m.message)))
                .put("old_data", json!(m.old_data))
                .put("type_code", m.type_code.map_or(Value::Null, |c| json!(c.to_string())))
                .put("device_suffix", json!(m.device_suffix))
                .put("locator", json!(m.locator))
                .put("legacy_telemetry", json!(m.legacy_telemetry));
            if m.destination_ssid != 0 {
                o.put("destination_ssid", json!(m.destination_ssid));
            }
            positioned(&mut o, &m.fields);
            o.done()
        }
        Data::Object(x) => {
            let mut o = Obj::new("object");
            o.put("name", json!(x.name)).put("killed", json!(x.killed)).put("timestamp", timestamp(&x.timestamp));
            positioned(&mut o, &x.fields);
            o.done()
        }
        Data::Item(x) => {
            let mut o = Obj::new("item");
            o.put("name", json!(x.name)).put("killed", json!(x.killed));
            positioned(&mut o, &x.fields);
            o.done()
        }
        Data::Message(m) => {
            let mut o = Obj::new("message");
            o.put("addressee", json!(m.addressee)).put("text", json!(m.text)).put("message_id", json!(m.message_id));
            o.opt("reply_ack", m.reply_ack.clone());
            o.done()
        }
        Data::Ack(a) => {
            let mut o = Obj::new("ack");
            o.put("addressee", json!(a.addressee)).put("acked_id", json!(a.acked_id)).put("message_id", json!(a.message_id));
            o.opt("reply_ack", a.reply_ack.clone());
            o.done()
        }
        Data::Reject(a) => {
            let mut o = Obj::new("reject");
            o.put("addressee", json!(a.addressee)).put("rejected_id", json!(a.rejected_id)).put("message_id", json!(a.message_id));
            o.opt("reply_ack", a.reply_ack.clone());
            o.done()
        }
        Data::Bulletin(b) | Data::NwsBulletin(b) => {
            let kind = if matches!(d, Data::Bulletin(_)) { "bulletin" } else { "nws-bulletin" };
            Obj::new(kind).put("addressee", json!(b.addressee)).put("text", json!(b.text)).put("message_id", json!(b.message_id)).done()
        }
        Data::TelemetryNames(t) => Obj::new("telemetry-names").put("addressee", json!(t.addressee)).put("names", json!(t.labels)).done(),
        Data::TelemetryUnits(t) => Obj::new("telemetry-units").put("addressee", json!(t.addressee)).put("units", json!(t.labels)).done(),
        Data::TelemetryCoefficients(t) => Obj::new("telemetry-coefficients")
            .put("addressee", json!(t.addressee))
            .put("coefficients", Value::Array(t.coefficients.iter().map(|c| number_text(c)).collect()))
            .done(),
        Data::TelemetryBits(t) => Obj::new("telemetry-bits")
            .put("addressee", json!(t.addressee))
            .put("bits", json!(t.bits))
            .put("project", json!(t.project))
            .done(),
        Data::DirectedQuery(q) => Obj::new("directed-query")
            .put("addressee", json!(q.addressee))
            .put("query_type", json!(q.query_type))
            .put("target", json!(q.target))
            .done(),
        Data::Status(s) => {
            let mut o = Obj::new("status");
            o.put("timestamp", timestamp(&s.timestamp))
                .put("locator", json!(s.locator))
                .put("symbol", s.symbol.map_or(Value::Null, |s| json!(symbol(s))));
            if let Some(b) = s.beam {
                o.put("beam", json!({"heading_code": b.heading_code.to_string(), "power_code": b.power_code.to_string()}));
            }
            o.put("text", json!(s.text)).done()
        }
        Data::Telemetry(t) => Obj::new("telemetry")
            .put("sequence", json!(t.sequence))
            .put("analog", Value::Array(t.analog.iter().map(|v| v.as_deref().map_or(Value::Null, number_text)).collect()))
            .put("bits", json!(t.bits))
            .put("comment", json!(t.comment))
            .done(),
        Data::Weather(w) => Obj::new("weather")
            .put("timestamp", timestamp(&w.timestamp))
            .put("weather", weather(&w.weather))
            .put("comment", json!(w.comment))
            .done(),
        Data::RawWeather(r) => Obj::new("raw-weather").put("format", json!(enum_name(r.format))).put("data", json!(r.data)).done(),
        Data::Nmea(n) => Obj::new("nmea")
            .put("sentence", json!(n.sentence))
            .put("has_checksum", json!(n.has_checksum))
            .put("latitude", json!(n.latitude))
            .put("longitude", json!(n.longitude))
            .put("fix", n.fix_valid.map_or(Value::Null, |v| json!(if v { "valid" } else { "invalid" })))
            .put("course_degrees", json!(n.course_degrees))
            .put("speed_knots", json!(n.speed_knots))
            .put("altitude_m", json!(n.altitude_m))
            .put("time", json!(n.time))
            .put("waypoint", json!(n.waypoint))
            .done(),
        Data::MaidenheadBeacon(m) => Obj::new("maidenhead-beacon").put("locator", json!(m.locator)).put("comment", json!(m.comment)).done(),
        Data::Query(q) => {
            let mut o = Obj::new("query");
            o.put("query_type", json!(q.query_type));
            if let Some(f) = q.footprint {
                o.put("footprint", json!({"latitude": f.latitude, "longitude": f.longitude, "radius_miles": f.radius_miles}));
            }
            o.done()
        }
        Data::Capabilities(c) => Obj::new("capabilities")
            .put(
                "capabilities",
                Value::Array(
                    c.capabilities
                        .iter()
                        .map(|(t, v)| match v {
                            Some(v) => json!([t, v]),
                            None => json!([t]),
                        })
                        .collect(),
                ),
            )
            .done(),
        Data::ThirdParty(p) => {
            let mut inner = Obj::new("");
            inner.put("source", json!(p.source.as_str())).put("destination", json!(p.destination.as_str())).put("path", path(&p.path));
            inner.put("data", data(&p.data)).put("diagnostics", json!(diagnostics(&p.diagnostics)));
            Obj::new("third-party").put("packet", inner.done()).done()
        }
        Data::UserDefined(u) => Obj::new("user-defined")
            .put("user_id", json!(u.user_id.to_string()))
            .put("packet_type", json!(u.packet_type.to_string()))
            .put("data", json!(u.data.iter().map(|&b| b as char).collect::<String>()))
            .done(),
        Data::Test(t) => Obj::new("test").put("data", json!(t.data)).done(),
        Data::AgreloDf(a) => Obj::new("agrelo-df").put("bearing_degrees", json!(a.bearing_degrees)).put("quality", json!(a.quality)).done(),
        Data::Unrecognized(u) => Obj::new("unrecognized").put("reason", json!(enum_name(*u))).done(),
    }
}

fn number_text(text: &str) -> Value {
    serde_json::from_str::<Value>(&normalise_number(text)).unwrap_or_else(|_| json!(text))
}

/// Numbers as sent (`073`, `.53`, `-.5`, `+1`) as JSON numbers.
fn normalise_number(text: &str) -> String {
    let t = text.trim();
    let (sign, digits) = match t.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", t.strip_prefix('+').unwrap_or(t)),
    };
    let (whole, fraction) = digits.split_once('.').map_or((digits, None), |(w, f)| (w, Some(f)));
    let whole = whole.trim_start_matches('0');
    let whole = if whole.is_empty() { "0" } else { whole };
    match fraction {
        Some(f) if !f.is_empty() => format!("{sign}{whole}.{f}"),
        _ => format!("{sign}{whole}"),
    }
}

fn mic_e_message(m: MicEMessage) -> String {
    match m {
        MicEMessage::Custom(n) => format!("custom{n}"),
        other => enum_name(other),
    }
}

fn positioned(o: &mut Obj, f: &Positioned) {
    o.put("latitude", json!(f.position.latitude)).put("longitude", json!(f.position.longitude));
    if f.position.ambiguity > 0 {
        o.put("ambiguity", json!(f.position.ambiguity));
    }
    o.put("symbol", json!(symbol(f.symbol))).put("compressed", json!(f.compressed));
    if let Some(t) = f.compression {
        o.put("compression", json!({"fix": enum_name(t.fix), "source": enum_name(t.source), "origin": origin(t.origin)}));
    }
    o.put("course_degrees", json!(f.course_degrees)).put("speed_knots", json!(f.speed_knots)).put("altitude_feet", json!(f.altitude_feet));
    if let Some(p) = f.phg {
        let mut po = Obj::new("");
        po.put("power", json!(p.power)).put("height", json!(p.height)).put("gain", json!(p.gain)).put("directivity", json!(p.directivity));
        po.put("beacons_per_hour", json!(p.beacons_per_hour));
        o.put("phg", po.done());
    }
    o.put("range_miles", json!(f.range_miles));
    if let Some(d) = f.dfs {
        o.put("dfs", json!({"strength": d.strength, "height": d.height, "gain": d.gain, "directivity": d.directivity}));
    }
    if let Some(a) = f.area {
        let mut ao = Obj::new("");
        ao.put("shape", json!(enum_name(a.shape))).put("lat_offset", json!(a.lat_offset)).put("color", json!(color(a.color)));
        ao.put("lon_offset", json!(a.lon_offset)).put("corridor_width_miles", json!(a.corridor_width_miles));
        o.put("area", ao.done());
    }
    if let Some(b) = f.df_bearing {
        o.put("df_bearing", json!({"bearing_degrees": b.bearing_degrees, "number": b.number, "range": b.range, "quality": b.quality}));
    }
    if let Some(s) = f.storm {
        let mut so = Obj::new(&enum_name(s.kind));
        so.put("sustained_wind_knots", json!(s.sustained_wind_knots))
            .put("gust_knots", json!(s.gust_knots))
            .put("central_pressure_mbar", json!(s.central_pressure_mbar))
            .put("hurricane_radius_nm", json!(s.hurricane_radius_nm))
            .put("tropical_storm_radius_nm", json!(s.tropical_storm_radius_nm))
            .put("whole_gale_radius_nm", json!(s.whole_gale_radius_nm));
        o.put("storm", so.done());
    }
    if let Some(d) = f.dao {
        o.put("dao", json!({"datum": d.datum.to_string(), "precision": enum_name(d.precision)}));
    }
    if let Some(t) = &f.telemetry {
        let mut to = Obj::new("");
        to.put("sequence", json!(t.sequence)).put("analog", json!(t.analog));
        to.opt("digital", t.digital);
        o.put("telemetry", to.done());
    }
    if let Some(v) = &f.frequency {
        let mut fo = Obj::new("");
        fo.put("mhz", json!(v.mhz)).put("tone", v.tone.map_or(Value::Null, |t| json!(enum_name(t))));
        fo.put("tone_value", json!(v.tone_value)).put("offset_khz", json!(v.offset_khz)).put("range", json!(v.range));
        fo.put("range_km", json!(v.range_km)).put("narrow", json!(v.narrow)).put("ten_khz_resolution", json!(v.ten_khz_resolution));
        o.put("frequency", fo.done());
    }
    if let Some(w) = &f.weather {
        // A weather object is written even when empty: it says the report is a weather report.
        o.0.insert("weather".into(), weather(w));
    }
    o.put("signpost", json!(f.signpost)).put("comment", json!(f.comment));
}

fn origin(o: CompressionOrigin) -> String {
    match o {
        CompressionOrigin::TncBeaconText => "tnc-beacon-text".into(),
        CompressionOrigin::Reserved3 => "reserved3".into(),
        other => enum_name(other),
    }
}

fn color(c: AreaColor) -> String {
    enum_name(c)
}

fn weather(w: &Weather) -> Value {
    let mut o = Obj::new("");
    o.put("wind_direction_degrees", json!(w.wind_direction_degrees))
        .put("wind_speed_mph", json!(w.wind_speed_mph))
        .put("wind_gust_mph", json!(w.wind_gust_mph))
        .put("temperature_f", json!(w.temperature_f))
        .put("rain_1h_in", json!(w.rain_1h_in))
        .put("rain_24h_in", json!(w.rain_24h_in))
        .put("rain_midnight_in", json!(w.rain_midnight_in))
        .put("rain_raw", json!(w.rain_raw))
        .put("humidity_percent", json!(w.humidity_percent))
        .put("pressure_mbar", json!(w.pressure_mbar))
        .put("luminosity_w_m2", json!(w.luminosity_w_m2))
        .put("snow_24h_in", json!(w.snow_24h_in))
        .put("software", w.software.map_or(Value::Null, |c| json!(c.to_string())))
        .put("unit", json!(w.unit))
        .put("extra", Value::Array(w.extra.iter().map(|e| json!({"letter": e.letter.to_string(), "value": e.value})).collect()));
    o.done()
}

// ------------------------------------------------------------------ reading

pub fn read_data(v: &Value) -> Data {
    let o = v.as_object().expect("data is an object");
    let s = |k: &str| o.get(k).and_then(Value::as_str).map(str::to_string);
    let string = |k: &str| s(k).unwrap_or_default();
    let flag = |k: &str| o.get(k).and_then(Value::as_bool).unwrap_or(false);
    match o["type"].as_str().unwrap() {
        "position" => Data::Position(PositionReport {
            timestamp: read_timestamp(o.get("timestamp")),
            messaging: flag("messaging"),
            fields: read_positioned(o),
        }),
        "mic-e" => Data::MicE(MicEReport {
            message: read_mic_e_message(o.get("mic_e_message").and_then(Value::as_str).unwrap_or("off-duty")),
            old_data: flag("old_data"),
            type_code: s("type_code").and_then(|t| t.chars().next()),
            device_suffix: string("device_suffix"),
            locator: s("locator"),
            legacy_telemetry: o
                .get("legacy_telemetry")
                .map(|a| a.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u8).collect())
                .unwrap_or_default(),
            destination_ssid: o.get("destination_ssid").and_then(Value::as_u64).unwrap_or(0) as u8,
            fields: read_positioned(o),
        }),
        "object" => Data::Object(ObjectReport {
            name: string("name"),
            killed: flag("killed"),
            timestamp: read_timestamp(o.get("timestamp")),
            fields: read_positioned(o),
        }),
        "item" => Data::Item(ItemReport { name: string("name"), killed: flag("killed"), fields: read_positioned(o) }),
        "message" => Data::Message(Message {
            addressee: string("addressee"),
            text: string("text"),
            message_id: s("message_id"),
            reply_ack: s("reply_ack"),
        }),
        "ack" => Data::Ack(Ack {
            addressee: string("addressee"),
            acked_id: string("acked_id"),
            message_id: s("message_id"),
            reply_ack: s("reply_ack"),
        }),
        "reject" => Data::Reject(Reject {
            addressee: string("addressee"),
            rejected_id: string("rejected_id"),
            message_id: s("message_id"),
            reply_ack: s("reply_ack"),
        }),
        "bulletin" => Data::Bulletin(Bulletin { addressee: string("addressee"), text: string("text"), message_id: s("message_id") }),
        "nws-bulletin" => Data::NwsBulletin(Bulletin { addressee: string("addressee"), text: string("text"), message_id: s("message_id") }),
        "telemetry-names" => Data::TelemetryNames(TelemetryLabels { addressee: string("addressee"), labels: strings(o.get("names")) }),
        "telemetry-units" => Data::TelemetryUnits(TelemetryLabels { addressee: string("addressee"), labels: strings(o.get("units")) }),
        "telemetry-coefficients" => Data::TelemetryCoefficients(TelemetryCoefficients {
            addressee: string("addressee"),
            coefficients: o.get("coefficients").map(|a| a.as_array().unwrap().iter().map(|x| x.to_string()).collect()).unwrap_or_default(),
        }),
        "telemetry-bits" => {
            Data::TelemetryBits(TelemetryBits { addressee: string("addressee"), bits: string("bits"), project: string("project") })
        }
        "directed-query" => {
            Data::DirectedQuery(DirectedQuery { addressee: string("addressee"), query_type: string("query_type"), target: s("target") })
        }
        "status" => Data::Status(Status {
            timestamp: read_timestamp(o.get("timestamp")),
            locator: s("locator"),
            symbol: s("symbol").map(|t| read_symbol(&t)),
            beam: o.get("beam").map(|b| BeamHeading {
                heading_code: b["heading_code"].as_str().unwrap().chars().next().unwrap(),
                power_code: b["power_code"].as_str().unwrap().chars().next().unwrap(),
            }),
            text: string("text"),
        }),
        "telemetry" => Data::Telemetry(Telemetry {
            sequence: string("sequence"),
            analog: o
                .get("analog")
                .map(|a| a.as_array().unwrap().iter().map(|x| (!x.is_null()).then(|| x.to_string())).collect())
                .unwrap_or_default(),
            bits: s("bits"),
            comment: string("comment"),
        }),
        "weather" => Data::Weather(WeatherReport {
            timestamp: read_timestamp(o.get("timestamp")),
            weather: o.get("weather").map(read_weather).unwrap_or_default(),
            comment: string("comment"),
        }),
        "raw-weather" => Data::RawWeather(RawWeather {
            format: match o["format"].as_str().unwrap() {
                "peet-bros-hash" => RawWeatherFormat::PeetBrosHash,
                "peet-bros-star" => RawWeatherFormat::PeetBrosStar,
                "ultimeter-packet" => RawWeatherFormat::UltimeterPacket,
                _ => RawWeatherFormat::UltimeterLogging,
            },
            data: string("data"),
        }),
        "nmea" => Data::Nmea(Nmea {
            sentence: string("sentence"),
            has_checksum: flag("has_checksum"),
            latitude: o.get("latitude").and_then(Value::as_f64),
            longitude: o.get("longitude").and_then(Value::as_f64),
            fix_valid: s("fix").map(|f| f == "valid"),
            course_degrees: o.get("course_degrees").and_then(Value::as_f64),
            speed_knots: o.get("speed_knots").and_then(Value::as_f64),
            altitude_m: o.get("altitude_m").and_then(Value::as_f64),
            time: s("time"),
            waypoint: s("waypoint"),
        }),
        "maidenhead-beacon" => Data::MaidenheadBeacon(MaidenheadBeacon { locator: string("locator"), comment: string("comment") }),
        "query" => Data::Query(Query {
            query_type: string("query_type"),
            footprint: o.get("footprint").map(|f| Footprint {
                latitude: f["latitude"].as_f64().unwrap(),
                longitude: f["longitude"].as_f64().unwrap(),
                radius_miles: f["radius_miles"].as_u64().unwrap() as u32,
            }),
        }),
        "capabilities" => Data::Capabilities(Capabilities {
            capabilities: o
                .get("capabilities")
                .map(|a| {
                    a.as_array()
                        .unwrap()
                        .iter()
                        .map(|p| {
                            let p = p.as_array().unwrap();
                            (p[0].as_str().unwrap().to_string(), p.get(1).map(|v| v.as_str().unwrap().to_string()))
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }),
        "third-party" => {
            let p = &o["packet"];
            let path: Vec<PathEntry> = strings(p.get("path")).iter().map(|e| read_path_entry(e)).collect();
            let inner = read_data(&p["data"]);
            let packet = Packet::create(
                Address::new(p["source"].as_str().unwrap()).unwrap(),
                Address::new(p["destination"].as_str().unwrap()).unwrap(),
                path,
                inner,
            )
            .expect("third-party inner packet encodes");
            Data::ThirdParty(Box::new(packet))
        }
        "user-defined" => Data::UserDefined(UserDefined {
            user_id: string("user_id").chars().next().unwrap(),
            packet_type: string("packet_type").chars().next().unwrap(),
            data: string("data").chars().map(|c| c as u8).collect(),
        }),
        "test" => Data::Test(TestData { data: string("data") }),
        "agrelo-df" => Data::AgreloDf(AgreloDf {
            bearing_degrees: o["bearing_degrees"].as_u64().unwrap() as u16,
            quality: o["quality"].as_u64().unwrap() as u8,
        }),
        other => panic!("cannot read data of type {other}"),
    }
}

pub fn read_path_entry(text: &str) -> PathEntry {
    match text.strip_suffix('*') {
        Some(t) => PathEntry { address: Address::new(t).unwrap(), used: true },
        None => PathEntry::new(Address::new(text).unwrap()),
    }
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.map(|a| a.as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect()).unwrap_or_default()
}

fn read_timestamp(v: Option<&Value>) -> Option<Timestamp> {
    let t = v?.as_str()?.as_bytes();
    Timestamp::parse(t).or_else(|| Timestamp::parse_mdhm(t))
}

fn read_symbol(t: &str) -> Symbol {
    let mut c = t.chars();
    Symbol { table: c.next().unwrap(), code: c.next().unwrap() }
}

fn read_mic_e_message(t: &str) -> MicEMessage {
    match t {
        "off-duty" => MicEMessage::OffDuty,
        "en-route" => MicEMessage::EnRoute,
        "in-service" => MicEMessage::InService,
        "returning" => MicEMessage::Returning,
        "committed" => MicEMessage::Committed,
        "special" => MicEMessage::Special,
        "priority" => MicEMessage::Priority,
        "emergency" => MicEMessage::Emergency,
        "unknown" => MicEMessage::Unknown,
        custom => MicEMessage::Custom(custom.trim_start_matches("custom").parse().unwrap()),
    }
}

fn read_positioned(o: &Map<String, Value>) -> Positioned {
    let f64_of = |k: &str| o.get(k).and_then(Value::as_f64);
    let u = |v: &Value, k: &str| v[k].as_u64().unwrap() as u8;
    Positioned {
        position: Position {
            latitude: f64_of("latitude").unwrap(),
            longitude: f64_of("longitude").unwrap(),
            ambiguity: o.get("ambiguity").and_then(Value::as_u64).unwrap_or(0) as u8,
        },
        symbol: read_symbol(o["symbol"].as_str().unwrap()),
        compressed: o.get("compressed").and_then(Value::as_bool).unwrap_or(false),
        compression: o.get("compression").map(|c| CompressionType {
            fix: if c["fix"] == "current" { GpsFix::Current } else { GpsFix::Old },
            source: match c["source"].as_str().unwrap() {
                "gll" => NmeaSource::Gll,
                "gga" => NmeaSource::Gga,
                "rmc" => NmeaSource::Rmc,
                _ => NmeaSource::Other,
            },
            origin: match c["origin"].as_str().unwrap() {
                "compressed" => CompressionOrigin::Compressed,
                "tnc-beacon-text" => CompressionOrigin::TncBeaconText,
                "software" => CompressionOrigin::Software,
                "reserved3" => CompressionOrigin::Reserved3,
                "kpc3" => CompressionOrigin::Kpc3,
                "pico" => CompressionOrigin::Pico,
                "other-tracker" => CompressionOrigin::OtherTracker,
                _ => CompressionOrigin::DigipeaterConversion,
            },
        }),
        course_degrees: o.get("course_degrees").and_then(Value::as_u64).map(|v| v as u16),
        speed_knots: f64_of("speed_knots"),
        altitude_feet: f64_of("altitude_feet"),
        phg: o.get("phg").map(|p| Phg {
            power: u(p, "power"),
            height: u(p, "height"),
            gain: u(p, "gain"),
            directivity: u(p, "directivity"),
            beacons_per_hour: p.get("beacons_per_hour").and_then(Value::as_u64).map(|v| v as u8),
        }),
        range_miles: f64_of("range_miles"),
        dfs: o.get("dfs").map(|d| DfSignalStrength {
            strength: u(d, "strength"),
            height: u(d, "height"),
            gain: u(d, "gain"),
            directivity: u(d, "directivity"),
        }),
        area: o.get("area").map(|a| AreaObject {
            shape: [
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
            ]
            .into_iter()
            .find(|s| enum_name(*s) == a["shape"].as_str().unwrap())
            .unwrap(),
            lat_offset: u(a, "lat_offset"),
            color: ALL_COLORS.into_iter().find(|c| color(*c) == a["color"].as_str().unwrap()).unwrap(),
            lon_offset: u(a, "lon_offset"),
            corridor_width_miles: a.get("corridor_width_miles").and_then(Value::as_u64).map(|v| v as u16),
        }),
        df_bearing: o.get("df_bearing").map(|b| DfBearing {
            bearing_degrees: b["bearing_degrees"].as_u64().unwrap() as u16,
            number: u(b, "number"),
            range: u(b, "range"),
            quality: u(b, "quality"),
        }),
        storm: o.get("storm").map(|s| {
            let n = |k: &str| s.get(k).and_then(Value::as_u64).map(|v| v as u16);
            Storm {
                kind: match s["type"].as_str().unwrap() {
                    "tropical-storm" => StormKind::TropicalStorm,
                    "hurricane" => StormKind::Hurricane,
                    _ => StormKind::TropicalDepression,
                },
                sustained_wind_knots: n("sustained_wind_knots"),
                gust_knots: n("gust_knots"),
                central_pressure_mbar: n("central_pressure_mbar"),
                hurricane_radius_nm: n("hurricane_radius_nm"),
                tropical_storm_radius_nm: n("tropical_storm_radius_nm"),
                whole_gale_radius_nm: n("whole_gale_radius_nm"),
            }
        }),
        dao: o.get("dao").map(|d| Dao {
            datum: d["datum"].as_str().unwrap().chars().next().unwrap(),
            precision: match d["precision"].as_str().unwrap() {
                "thousandths" => DaoPrecision::Thousandths,
                "base91" => DaoPrecision::Base91,
                _ => DaoPrecision::None,
            },
        }),
        telemetry: o.get("telemetry").map(|t| CommentTelemetry {
            sequence: t["sequence"].as_u64().unwrap() as u16,
            analog: t["analog"].as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u16).collect(),
            digital: t.get("digital").and_then(Value::as_u64).map(|v| v as u8),
        }),
        frequency: o.get("frequency").map(|f| VoiceFrequency {
            mhz: f["mhz"].as_f64().unwrap(),
            tone: f.get("tone").map(|t| match t.as_str().unwrap() {
                "off" => Tone::Off,
                "tone" => Tone::Tone,
                "ctcss" => Tone::Ctcss,
                _ => Tone::Dcs,
            }),
            tone_value: f.get("tone_value").and_then(Value::as_u64).map(|v| v as u16),
            offset_khz: f.get("offset_khz").and_then(Value::as_i64).map(|v| v as i32),
            range: f.get("range").and_then(Value::as_u64).map(|v| v as u16),
            range_km: f.get("range_km").and_then(Value::as_bool).unwrap_or(false),
            narrow: f.get("narrow").and_then(Value::as_bool).unwrap_or(false),
            ten_khz_resolution: f.get("ten_khz_resolution").and_then(Value::as_bool).unwrap_or(false),
        }),
        weather: o.get("weather").map(read_weather),
        signpost: o.get("signpost").and_then(Value::as_str).map(str::to_string),
        comment: o.get("comment").and_then(Value::as_str).unwrap_or_default().to_string(),
    }
}

const ALL_COLORS: [AreaColor; 16] = [
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

fn read_weather(v: &Value) -> Weather {
    let f = |k: &str| v.get(k).and_then(Value::as_f64);
    Weather {
        wind_direction_degrees: f("wind_direction_degrees").map(|x| x as u16),
        wind_speed_mph: f("wind_speed_mph"),
        wind_gust_mph: f("wind_gust_mph"),
        temperature_f: f("temperature_f"),
        rain_1h_in: f("rain_1h_in"),
        rain_24h_in: f("rain_24h_in"),
        rain_midnight_in: f("rain_midnight_in"),
        rain_raw: f("rain_raw").map(|x| x as u32),
        humidity_percent: f("humidity_percent").map(|x| x as u8),
        pressure_mbar: f("pressure_mbar"),
        luminosity_w_m2: f("luminosity_w_m2").map(|x| x as u16),
        snow_24h_in: f("snow_24h_in"),
        software: v.get("software").and_then(Value::as_str).and_then(|s| s.chars().next()),
        unit: v.get("unit").and_then(Value::as_str).map(str::to_string),
        extra: v
            .get("extra")
            .map(|a| {
                a.as_array()
                    .unwrap()
                    .iter()
                    .map(|e| WeatherField {
                        letter: e["letter"].as_str().unwrap().chars().next().unwrap(),
                        value: e["value"].as_str().unwrap().to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}
