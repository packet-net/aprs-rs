//! The vectors' neutral data form: writing decoded data (for comparison) and reading it back (to
//! build data for the encode cases). The rules are in vectors/README.md.

use pdn_aprs::*;
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
        Data::TelemetryNames(t) => Obj::new("telemetry-names")
            .put("addressee", json!(t.addressee))
            .put("names", json!(t.labels))
            .put("message_id", json!(t.message_id))
            .done(),
        Data::TelemetryUnits(t) => Obj::new("telemetry-units")
            .put("addressee", json!(t.addressee))
            .put("units", json!(t.labels))
            .put("message_id", json!(t.message_id))
            .done(),
        Data::TelemetryCoefficients(t) => Obj::new("telemetry-coefficients")
            .put("addressee", json!(t.addressee))
            .put("coefficients", Value::Array(t.coefficients.iter().map(|c| number_text(c)).collect()))
            .put("message_id", json!(t.message_id))
            .done(),
        Data::TelemetryBits(t) => Obj::new("telemetry-bits")
            .put("addressee", json!(t.addressee))
            .put("bits", json!(t.bits))
            .put("project", json!(t.project))
            .put("message_id", json!(t.message_id))
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
            .put("comment", json!(n.comment))
            .done(),
        Data::MaidenheadBeacon(m) => Obj::new("maidenhead-beacon").put("locator", json!(m.locator)).put("comment", json!(m.comment)).done(),
        Data::Query(q) => {
            let mut o = Obj::new("query");
            o.put("query_type", json!(q.query_type));
            if let Some(f) = &q.footprint {
                o.put(
                    "footprint",
                    json!({"latitude": number_text(&f.latitude), "longitude": number_text(&f.longitude), "radius_miles": f.radius_miles}),
                );
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
            // A header's source and destination are always written, even when empty.
            let mut inner = Obj::new("");
            inner.0.insert("source".into(), json!(p.source.as_str()));
            inner.0.insert("destination".into(), json!(p.destination.as_str()));
            inner.put("path", path(&p.path));
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
    serde_json::from_str::<Value>(&normalise_number(text))
        .or_else(|_| text.trim().parse::<f64>().map(|v| json!(v)))
        .unwrap_or_else(|_| json!(text))
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

/// Why data in the neutral form could not be made into this crate's data.
#[derive(Debug)]
pub enum Unreadable {
    /// This crate's data cannot hold something in it: the key, and why.
    Unsupported(String),
    /// Its encoder refused a part that has to be encoded to be held: a third-party packet keeps
    /// its inner packet's information field, which the neutral form does not carry.
    Refused(String),
}

impl std::fmt::Display for Unreadable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unreadable::Unsupported(why) | Unreadable::Refused(why) => f.write_str(why),
        }
    }
}

type Read<T> = Result<T, Unreadable>;

fn unsupported<T>(why: String) -> Read<T> {
    Err(Unreadable::Unsupported(why))
}

/// An object's keys, checked against the ones this crate's data can hold, with getters that name
/// the key when a value will not fit.
struct Keys<'a> {
    o: &'a Map<String, Value>,
    at: String,
}

impl<'a> Keys<'a> {
    fn new(v: &'a Value, at: &str, allowed: &[&str]) -> Read<Keys<'a>> {
        let Some(o) = v.as_object() else { return unsupported(format!("{at}: not an object")) };
        let keys = Keys { o, at: at.to_string() };
        if let Some(k) = o.keys().find(|k| !allowed.contains(&k.as_str())) {
            return unsupported(format!("{}: not a field this crate has", keys.name(k)));
        }
        Ok(keys)
    }

    fn name(&self, k: &str) -> String {
        if self.at.is_empty() { k.to_string() } else { format!("{}.{k}", self.at) }
    }

    fn get(&self, k: &str) -> Option<&'a Value> {
        self.o.get(k).filter(|v| !v.is_null())
    }

    fn string(&self, k: &str) -> Read<Option<String>> {
        match self.get(k) {
            None => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.clone())),
            Some(_) => unsupported(format!("{}: not text", self.name(k))),
        }
    }

    fn text(&self, k: &str) -> Read<String> {
        Ok(self.string(k)?.unwrap_or_default())
    }

    fn char(&self, k: &str) -> Read<Option<char>> {
        let Some(t) = self.string(k)? else { return Ok(None) };
        let mut chars = t.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => Ok(Some(c)),
            _ => unsupported(format!("{}: not one character", self.name(k))),
        }
    }

    fn flag(&self, k: &str) -> Read<bool> {
        match self.get(k) {
            None => Ok(false),
            Some(Value::Bool(b)) => Ok(*b),
            Some(_) => unsupported(format!("{}: not a boolean", self.name(k))),
        }
    }

    fn float(&self, k: &str) -> Read<Option<f64>> {
        match self.get(k) {
            None => Ok(None),
            Some(v) => v.as_f64().map(Some).ok_or_else(|| Unreadable::Unsupported(format!("{}: not a number", self.name(k)))),
        }
    }

    fn required_float(&self, k: &str) -> Read<f64> {
        self.float(k)?
            .ok_or_else(|| Unreadable::Unsupported(format!("{}: missing, and this crate has no value for its absence", self.name(k))))
    }

    /// A whole number that fits `T`.
    fn int<T: TryFrom<i128>>(&self, k: &str) -> Read<Option<T>> {
        let Some(v) = self.get(k) else { return Ok(None) };
        whole(v)
            .and_then(|n| T::try_from(n).ok())
            .map(Some)
            .ok_or_else(|| Unreadable::Unsupported(format!("{}: not a whole number in the range this crate holds", self.name(k))))
    }

    fn required_int<T: TryFrom<i128>>(&self, k: &str) -> Read<T> {
        self.int(k)?
            .ok_or_else(|| Unreadable::Unsupported(format!("{}: missing, and this crate has no value for its absence", self.name(k))))
    }

    fn strings(&self, k: &str) -> Read<Vec<String>> {
        match self.get(k) {
            None => Ok(Vec::new()),
            Some(Value::Array(a)) => a
                .iter()
                .map(|x| match x {
                    Value::String(s) => Ok(s.clone()),
                    _ => unsupported(format!("{}: not text", self.name(k))),
                })
                .collect(),
            Some(_) => unsupported(format!("{}: not a list", self.name(k))),
        }
    }

    fn array(&self, k: &str) -> Read<Option<&'a Vec<Value>>> {
        match self.get(k) {
            None => Ok(None),
            Some(Value::Array(a)) => Ok(Some(a)),
            Some(_) => unsupported(format!("{}: not a list", self.name(k))),
        }
    }

    fn object(&self, k: &str, allowed: &[&str]) -> Read<Option<Keys<'a>>> {
        self.get(k).map(|v| Keys::new(v, &self.name(k), allowed)).transpose()
    }

    /// One of an enumeration's values, by its neutral name.
    fn choice<T: Copy>(&self, k: &str, values: &[(&str, T)]) -> Read<Option<T>> {
        let Some(t) = self.string(k)? else { return Ok(None) };
        match values.iter().find(|(name, _)| *name == t) {
            Some((_, v)) => Ok(Some(*v)),
            None => unsupported(format!("{}: not a value this crate has", self.name(k))),
        }
    }

    fn required<T>(&self, k: &str, v: Option<T>) -> Read<T> {
        v.ok_or_else(|| Unreadable::Unsupported(format!("{}: missing, and this crate has no value for its absence", self.name(k))))
    }
}

/// A JSON number that is a whole number, as an integer.
fn whole(v: &Value) -> Option<i128> {
    if let Some(n) = v.as_i64() {
        return Some(i128::from(n));
    }
    if let Some(n) = v.as_u64() {
        return Some(i128::from(n));
    }
    let f = v.as_f64()?;
    (f.fract() == 0.0 && f.abs() < 1e30).then_some(f as i128)
}

/// A number in the neutral form as the text this crate keeps as sent: as the JSON gives it, or in
/// full where the JSON would use an exponent, which APRS numbers do not.
fn number_as_text(v: &Value) -> Option<String> {
    match v {
        Value::Number(n) => {
            let t = n.to_string();
            Some(if t.contains(['e', 'E']) { format!("{}", n.as_f64()?) } else { t })
        }
        Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

const POSITIONED: [&str; 24] = [
    "latitude",
    "longitude",
    "ambiguity",
    "symbol",
    "compressed",
    "compression",
    "course_degrees",
    "speed_knots",
    "altitude_feet",
    "phg",
    "range_miles",
    "dfs",
    "area",
    "df_bearing",
    "storm",
    "dao",
    "telemetry",
    "frequency",
    "weather",
    "signpost",
    "comment",
    "type",
    "timestamp",
    "messaging",
];

fn with_positioned(extra: &[&'static str]) -> Vec<&'static str> {
    POSITIONED.iter().copied().chain(extra.iter().copied()).collect()
}

/// Neutral data as this crate's data, or why it cannot be.
pub fn try_read_data(v: &Value) -> Read<Data> {
    let kind = v.get("type").and_then(Value::as_str).unwrap_or("");
    let keys: Vec<&str> = match kind {
        "position" => with_positioned(&[]),
        "mic-e" => {
            with_positioned(&["mic_e_message", "old_data", "type_code", "device_suffix", "locator", "legacy_telemetry", "destination_ssid"])
        }
        "object" => with_positioned(&["name", "killed"]),
        "item" => with_positioned(&["name", "killed"]),
        "message" => vec!["type", "addressee", "text", "message_id", "reply_ack"],
        "ack" => vec!["type", "addressee", "acked_id", "message_id", "reply_ack"],
        "reject" => vec!["type", "addressee", "rejected_id", "message_id", "reply_ack"],
        "bulletin" | "nws-bulletin" => vec!["type", "addressee", "text", "message_id"],
        "telemetry-names" => vec!["type", "addressee", "names", "message_id"],
        "telemetry-units" => vec!["type", "addressee", "units", "message_id"],
        "telemetry-coefficients" => vec!["type", "addressee", "coefficients", "message_id"],
        "telemetry-bits" => vec!["type", "addressee", "bits", "project", "message_id"],
        "directed-query" => vec!["type", "addressee", "query_type", "target"],
        "status" => vec!["type", "timestamp", "locator", "symbol", "beam", "text"],
        "telemetry" => vec!["type", "sequence", "analog", "bits", "comment"],
        "weather" => vec!["type", "timestamp", "weather", "comment"],
        "raw-weather" => vec!["type", "format", "data"],
        "nmea" => vec![
            "type",
            "sentence",
            "has_checksum",
            "latitude",
            "longitude",
            "fix",
            "course_degrees",
            "speed_knots",
            "altitude_m",
            "time",
            "waypoint",
            "comment",
        ],
        "maidenhead-beacon" => vec!["type", "locator", "comment"],
        "query" => vec!["type", "query_type", "footprint"],
        "capabilities" => vec!["type", "capabilities"],
        "third-party" => vec!["type", "packet"],
        "user-defined" => vec!["type", "user_id", "packet_type", "data"],
        "test" => vec!["type", "data"],
        "agrelo-df" => vec!["type", "bearing_degrees", "quality"],
        "unrecognized" => vec!["type", "reason"],
        _ => return unsupported("type: not a data type this crate has".into()),
    };
    // Positioned fields that belong to another kind of report.
    let keys: Vec<&str> = keys
        .into_iter()
        .filter(|k| match *k {
            "timestamp" => matches!(kind, "position" | "object" | "status" | "weather"),
            "messaging" => kind == "position",
            _ => true,
        })
        .collect();
    let o = Keys::new(v, "", &keys)?;
    Ok(match kind {
        "position" => {
            Data::Position(PositionReport { timestamp: read_timestamp(&o)?, messaging: o.flag("messaging")?, fields: read_positioned(&o)? })
        }
        "mic-e" => Data::MicE(MicEReport {
            message: o.required("mic_e_message", read_mic_e_message(&o)?)?,
            old_data: o.flag("old_data")?,
            type_code: o.char("type_code")?,
            device_suffix: o.text("device_suffix")?,
            locator: o.string("locator")?,
            legacy_telemetry: match o.array("legacy_telemetry")? {
                None => Vec::new(),
                Some(a) => a
                    .iter()
                    .map(|x| whole(x).and_then(|n| u8::try_from(n).ok()))
                    .collect::<Option<Vec<u8>>>()
                    .ok_or_else(|| Unreadable::Unsupported("legacy_telemetry: values are bytes, 0-255".into()))?,
            },
            destination_ssid: o.int("destination_ssid")?.unwrap_or(0),
            fields: read_positioned(&o)?,
        }),
        "object" => Data::Object(ObjectReport {
            name: o.text("name")?,
            killed: o.flag("killed")?,
            timestamp: read_timestamp(&o)?,
            fields: read_positioned(&o)?,
        }),
        "item" => Data::Item(ItemReport { name: o.text("name")?, killed: o.flag("killed")?, fields: read_positioned(&o)? }),
        "message" => Data::Message(Message {
            addressee: o.text("addressee")?,
            text: o.text("text")?,
            message_id: o.string("message_id")?,
            reply_ack: o.string("reply_ack")?,
        }),
        "ack" => Data::Ack(Ack {
            addressee: o.text("addressee")?,
            acked_id: o.text("acked_id")?,
            message_id: o.string("message_id")?,
            reply_ack: o.string("reply_ack")?,
        }),
        "reject" => Data::Reject(Reject {
            addressee: o.text("addressee")?,
            rejected_id: o.text("rejected_id")?,
            message_id: o.string("message_id")?,
            reply_ack: o.string("reply_ack")?,
        }),
        "bulletin" | "nws-bulletin" => {
            let b = Bulletin { addressee: o.text("addressee")?, text: o.text("text")?, message_id: o.string("message_id")? };
            if kind == "bulletin" { Data::Bulletin(b) } else { Data::NwsBulletin(b) }
        }
        "telemetry-names" => Data::TelemetryNames(TelemetryLabels {
            addressee: o.text("addressee")?,
            labels: o.strings("names")?,
            message_id: o.string("message_id")?,
        }),
        "telemetry-units" => Data::TelemetryUnits(TelemetryLabels {
            addressee: o.text("addressee")?,
            labels: o.strings("units")?,
            message_id: o.string("message_id")?,
        }),
        "telemetry-coefficients" => Data::TelemetryCoefficients(TelemetryCoefficients {
            addressee: o.text("addressee")?,
            coefficients: o
                .array("coefficients")?
                .map(|a| a.iter().map(number_as_text).collect::<Option<Vec<String>>>())
                .unwrap_or(Some(Vec::new()))
                .ok_or_else(|| Unreadable::Unsupported("coefficients: each is a number".into()))?,
            message_id: o.string("message_id")?,
        }),
        "telemetry-bits" => Data::TelemetryBits(TelemetryBits {
            addressee: o.text("addressee")?,
            bits: o.text("bits")?,
            project: o.text("project")?,
            message_id: o.string("message_id")?,
        }),
        "directed-query" => Data::DirectedQuery(DirectedQuery {
            addressee: o.text("addressee")?,
            query_type: o.text("query_type")?,
            target: o.string("target")?,
        }),
        "status" => Data::Status(Status {
            timestamp: read_timestamp(&o)?,
            locator: o.string("locator")?,
            symbol: read_symbol(&o)?,
            beam: match o.object("beam", &["heading_code", "power_code"])? {
                None => None,
                Some(b) => Some(BeamHeading {
                    heading_code: b.required("heading_code", b.char("heading_code")?)?,
                    power_code: b.required("power_code", b.char("power_code")?)?,
                }),
            },
            text: o.text("text")?,
        }),
        "telemetry" => Data::Telemetry(Telemetry {
            sequence: o.text("sequence")?,
            analog: o
                .array("analog")?
                .map(|a| {
                    a.iter()
                        .map(|x| if x.is_null() { Some(None) } else { number_as_text(x).map(Some) })
                        .collect::<Option<Vec<Option<String>>>>()
                })
                .unwrap_or(Some(Vec::new()))
                .ok_or_else(|| Unreadable::Unsupported("analog: each is a number or null".into()))?,
            bits: o.string("bits")?,
            comment: o.text("comment")?,
        }),
        "weather" => Data::Weather(WeatherReport {
            timestamp: read_timestamp(&o)?,
            weather: read_weather(&o)?.unwrap_or_default(),
            comment: o.text("comment")?,
        }),
        "raw-weather" => Data::RawWeather(RawWeather {
            format: o.required(
                "format",
                o.choice(
                    "format",
                    &[
                        ("peet-bros-hash", RawWeatherFormat::PeetBrosHash),
                        ("peet-bros-star", RawWeatherFormat::PeetBrosStar),
                        ("ultimeter-packet", RawWeatherFormat::UltimeterPacket),
                        ("ultimeter-logging", RawWeatherFormat::UltimeterLogging),
                    ],
                )?,
            )?,
            data: o.text("data")?,
        }),
        "nmea" => Data::Nmea(Nmea {
            sentence: o.text("sentence")?,
            has_checksum: o.flag("has_checksum")?,
            latitude: o.float("latitude")?,
            longitude: o.float("longitude")?,
            fix_valid: o.choice("fix", &[("valid", true), ("invalid", false)])?,
            course_degrees: o.float("course_degrees")?,
            speed_knots: o.float("speed_knots")?,
            altitude_m: o.float("altitude_m")?,
            time: o.string("time")?,
            waypoint: o.string("waypoint")?,
            comment: o.text("comment")?,
        }),
        "maidenhead-beacon" => Data::MaidenheadBeacon(MaidenheadBeacon { locator: o.text("locator")?, comment: o.text("comment")? }),
        "query" => Data::Query(Query {
            query_type: o.text("query_type")?,
            // The neutral form has the numbers, not their text: each is written as the JSON gives
            // it, after a space when it is positive (APRS12c ch. 15).
            footprint: match o.object("footprint", &["latitude", "longitude", "radius_miles"])? {
                None => None,
                Some(f) => {
                    let text = |k: &str| -> Read<String> {
                        let t = f.get(k).and_then(number_as_text);
                        let t = f.required(k, t)?;
                        Ok(if t.starts_with('-') { t } else { format!(" {t}") })
                    };
                    Some(Footprint {
                        latitude: text("latitude")?,
                        longitude: text("longitude")?,
                        radius_miles: f.required_int("radius_miles")?,
                    })
                }
            },
        }),
        "capabilities" => Data::Capabilities(Capabilities {
            capabilities: o
                .array("capabilities")?
                .map(|a| {
                    a.iter()
                        .map(|p| match p.as_array().map(Vec::as_slice) {
                            Some([Value::String(t)]) => Some((t.clone(), None)),
                            Some([Value::String(t), Value::String(v)]) => Some((t.clone(), Some(v.clone()))),
                            _ => None,
                        })
                        .collect::<Option<Vec<_>>>()
                })
                .unwrap_or(Some(Vec::new()))
                .ok_or_else(|| Unreadable::Unsupported("capabilities: each is [token] or [token, value]".into()))?,
        }),
        "third-party" => {
            // The inner packet as the neutral form gives it: its source may be any third-party
            // source (APRS12c ch. 17), and its diagnostics are part of the data. This crate keeps
            // the inner information field as received, which the neutral form does not carry, so
            // it is the inner data encoded.
            let p = o.required("packet", o.object("packet", &["source", "destination", "path", "data", "diagnostics"])?)?;
            let source = p.text("source")?;
            let source = Address::third_party_source(&source).or_else(|_| unsupported("packet.source: not a third-party source".into()))?;
            let destination = read_destination(&p.text("destination")?)?;
            let path = p.strings("path")?.iter().map(|e| read_path_entry_checked(e)).collect::<Read<Vec<PathEntry>>>()?;
            let data = try_read_data(p.required("data", p.get("data"))?)?;
            let information = match &data {
                Data::Unrecognized(Unrecognized::Empty) => Vec::new(),
                Data::Unrecognized(_) => return unsupported("packet.data: undecoded, and its bytes are not in the neutral form".into()),
                data => data.encode().map_err(|e| Unreadable::Refused(format!("the inner packet's data: {e}")))?,
            };
            let diagnostics = p.strings("diagnostics")?.iter().map(|d| read_diagnostic(d)).collect::<Read<Vec<Diagnostic>>>()?;
            Data::ThirdParty(Box::new(Packet { source, destination, path, information, data, diagnostics, third_party: true }))
        }
        "user-defined" => {
            let byte_text = |k: &str| -> Read<Vec<u8>> {
                o.text(k)?
                    .chars()
                    .map(|c| u8::try_from(u32::from(c)).ok())
                    .collect::<Option<Vec<u8>>>()
                    .ok_or_else(|| Unreadable::Unsupported(format!("{k}: user-defined data is bytes, U+0000-U+00FF")))
            };
            Data::UserDefined(UserDefined {
                user_id: o.required("user_id", o.char("user_id")?)?,
                packet_type: o.required("packet_type", o.char("packet_type")?)?,
                data: byte_text("data")?,
            })
        }
        "test" => Data::Test(TestData { data: o.text("data")? }),
        "agrelo-df" => {
            Data::AgreloDf(AgreloDf { bearing_degrees: o.required_int("bearing_degrees")?, quality: o.required_int("quality")? })
        }
        _ => Data::Unrecognized(
            o.choice(
                "reason",
                &[
                    ("empty", Unrecognized::Empty),
                    ("not-aprs", Unrecognized::NotAprs),
                    ("reserved-data-type", Unrecognized::ReservedDataType),
                    ("malformed", Unrecognized::Malformed),
                ],
            )?
            .unwrap_or(Unrecognized::Malformed),
        ),
    })
}

/// A destination address; an empty one (a tolerated defect, UAP 5.2) is only made by decoding
/// a header that has one.
fn read_destination(text: &str) -> Read<Address> {
    if text.is_empty() {
        return Ok(Packet::decode_tnc2(b"N0CALL>:", ParseOptions::LENIENT).expect("an empty destination is tolerated").destination);
    }
    Address::new(text).or_else(|_| unsupported("packet.destination: not an address".into()))
}

/// A diagnostic from its neutral form, `severity:code`; the message and offset are not part of it.
fn read_diagnostic(text: &str) -> Read<Diagnostic> {
    let Some((severity, code)) = text.split_once(':') else {
        return unsupported("packet.diagnostics: not severity:code".into());
    };
    let severity = match severity {
        "info" => Severity::Info,
        "warning" => Severity::Warning,
        "error" => Severity::Error,
        _ => return unsupported("packet.diagnostics: not a severity".into()),
    };
    let Some(code) = Code::from_id(code) else {
        return unsupported("packet.diagnostics: not a code this crate has".into());
    };
    Ok(Diagnostic { severity, code, message: String::new(), offset: None })
}

pub fn read_path_entry(text: &str) -> PathEntry {
    read_path_entry_checked(text).unwrap_or_else(|e| panic!("{e}"))
}

fn read_path_entry_checked(text: &str) -> Read<PathEntry> {
    let (address, used) = match text.strip_suffix('*') {
        Some(t) => (t, true),
        None => (text, false),
    };
    let address = Address::new(address).or_else(|_| unsupported("path: not an address".into()))?;
    Ok(PathEntry { address, used })
}

fn read_timestamp(o: &Keys) -> Read<Option<Timestamp>> {
    let Some(t) = o.string("timestamp")? else { return Ok(None) };
    let b = t.as_bytes();
    match Timestamp::parse(b).or_else(|| Timestamp::parse_mdhm(b)) {
        Some(ts) => Ok(Some(ts)),
        None => unsupported("timestamp: not a timestamp as on air".into()),
    }
}

fn read_symbol(o: &Keys) -> Read<Option<Symbol>> {
    let Some(t) = o.string("symbol")? else { return Ok(None) };
    let mut c = t.chars();
    match (c.next(), c.next(), c.next()) {
        (Some(table), Some(code), None) => Ok(Some(Symbol { table, code })),
        _ => unsupported("symbol: not a table and a code".into()),
    }
}

fn read_mic_e_message(o: &Keys) -> Read<Option<MicEMessage>> {
    let Some(t) = o.string("mic_e_message")? else { return Ok(None) };
    Ok(Some(match t.as_str() {
        "off-duty" => MicEMessage::OffDuty,
        "en-route" => MicEMessage::EnRoute,
        "in-service" => MicEMessage::InService,
        "returning" => MicEMessage::Returning,
        "committed" => MicEMessage::Committed,
        "special" => MicEMessage::Special,
        "priority" => MicEMessage::Priority,
        "emergency" => MicEMessage::Emergency,
        "unknown" => MicEMessage::Unknown,
        custom => match custom.strip_prefix("custom").and_then(|n| n.parse::<u8>().ok()) {
            Some(n) => MicEMessage::Custom(n),
            None => return unsupported("mic_e_message: not a value this crate has".into()),
        },
    }))
}

const SHAPES: [(&str, AreaShape); 10] = [
    ("open-circle", AreaShape::OpenCircle),
    ("line-down-right", AreaShape::LineDownRight),
    ("open-ellipse", AreaShape::OpenEllipse),
    ("open-triangle", AreaShape::OpenTriangle),
    ("open-box", AreaShape::OpenBox),
    ("filled-circle", AreaShape::FilledCircle),
    ("line-down-left", AreaShape::LineDownLeft),
    ("filled-ellipse", AreaShape::FilledEllipse),
    ("filled-triangle", AreaShape::FilledTriangle),
    ("filled-box", AreaShape::FilledBox),
];

const COLORS: [(&str, AreaColor); 16] = [
    ("black", AreaColor::Black),
    ("blue", AreaColor::Blue),
    ("green", AreaColor::Green),
    ("cyan", AreaColor::Cyan),
    ("red", AreaColor::Red),
    ("violet", AreaColor::Violet),
    ("yellow", AreaColor::Yellow),
    ("gray", AreaColor::Gray),
    ("black-low", AreaColor::BlackLow),
    ("blue-low", AreaColor::BlueLow),
    ("green-low", AreaColor::GreenLow),
    ("cyan-low", AreaColor::CyanLow),
    ("red-low", AreaColor::RedLow),
    ("violet-low", AreaColor::VioletLow),
    ("yellow-low", AreaColor::YellowLow),
    ("gray-low", AreaColor::GrayLow),
];

fn read_positioned(o: &Keys) -> Read<Positioned> {
    Ok(Positioned {
        position: Position {
            latitude: o.required_float("latitude")?,
            longitude: o.required_float("longitude")?,
            ambiguity: o.int("ambiguity")?.unwrap_or(0),
        },
        symbol: o.required("symbol", read_symbol(o)?)?,
        compressed: o.flag("compressed")?,
        compression: match o.object("compression", &["fix", "source", "origin"])? {
            None => None,
            Some(c) => Some(CompressionType {
                fix: c.required("fix", c.choice("fix", &[("old", GpsFix::Old), ("current", GpsFix::Current)])?)?,
                source: c.required(
                    "source",
                    c.choice(
                        "source",
                        &[("other", NmeaSource::Other), ("gll", NmeaSource::Gll), ("gga", NmeaSource::Gga), ("rmc", NmeaSource::Rmc)],
                    )?,
                )?,
                origin: c.required(
                    "origin",
                    c.choice(
                        "origin",
                        &[
                            ("compressed", CompressionOrigin::Compressed),
                            ("tnc-beacon-text", CompressionOrigin::TncBeaconText),
                            ("software", CompressionOrigin::Software),
                            ("reserved3", CompressionOrigin::Reserved3),
                            ("kpc3", CompressionOrigin::Kpc3),
                            ("pico", CompressionOrigin::Pico),
                            ("other-tracker", CompressionOrigin::OtherTracker),
                            ("digipeater-conversion", CompressionOrigin::DigipeaterConversion),
                        ],
                    )?,
                )?,
            }),
        },
        course_degrees: o.int("course_degrees")?,
        speed_knots: o.float("speed_knots")?,
        altitude_feet: o.float("altitude_feet")?,
        phg: match o.object("phg", &["power", "height", "gain", "directivity", "beacons_per_hour"])? {
            None => None,
            Some(p) => Some(Phg {
                power: p.required_int("power")?,
                height: p.required_int("height")?,
                gain: p.required_int("gain")?,
                directivity: p.required_int("directivity")?,
                beacons_per_hour: p.int("beacons_per_hour")?,
            }),
        },
        range_miles: o.float("range_miles")?,
        dfs: match o.object("dfs", &["strength", "height", "gain", "directivity"])? {
            None => None,
            Some(d) => Some(DfSignalStrength {
                strength: d.required_int("strength")?,
                height: d.required_int("height")?,
                gain: d.required_int("gain")?,
                directivity: d.required_int("directivity")?,
            }),
        },
        area: match o.object("area", &["shape", "lat_offset", "color", "lon_offset", "corridor_width_miles"])? {
            None => None,
            Some(a) => Some(AreaObject {
                shape: a.required("shape", a.choice("shape", &SHAPES)?)?,
                lat_offset: a.required_int("lat_offset")?,
                color: a.required("color", a.choice("color", &COLORS)?)?,
                lon_offset: a.required_int("lon_offset")?,
                corridor_width_miles: a.int("corridor_width_miles")?,
            }),
        },
        df_bearing: match o.object("df_bearing", &["bearing_degrees", "number", "range", "quality"])? {
            None => None,
            Some(b) => Some(DfBearing {
                bearing_degrees: b.required_int("bearing_degrees")?,
                number: b.required_int("number")?,
                range: b.required_int("range")?,
                quality: b.required_int("quality")?,
            }),
        },
        storm: match o.object(
            "storm",
            &[
                "type",
                "sustained_wind_knots",
                "gust_knots",
                "central_pressure_mbar",
                "hurricane_radius_nm",
                "tropical_storm_radius_nm",
                "whole_gale_radius_nm",
            ],
        )? {
            None => None,
            Some(s) => Some(Storm {
                kind: s.required(
                    "type",
                    s.choice(
                        "type",
                        &[
                            ("tropical-storm", StormKind::TropicalStorm),
                            ("hurricane", StormKind::Hurricane),
                            ("tropical-depression", StormKind::TropicalDepression),
                        ],
                    )?,
                )?,
                sustained_wind_knots: s.int("sustained_wind_knots")?,
                gust_knots: s.int("gust_knots")?,
                central_pressure_mbar: s.int("central_pressure_mbar")?,
                hurricane_radius_nm: s.int("hurricane_radius_nm")?,
                tropical_storm_radius_nm: s.int("tropical_storm_radius_nm")?,
                whole_gale_radius_nm: s.int("whole_gale_radius_nm")?,
            }),
        },
        dao: match o.object("dao", &["datum", "precision"])? {
            None => None,
            Some(d) => Some(Dao {
                datum: d.required("datum", d.char("datum")?)?,
                precision: d
                    .choice(
                        "precision",
                        &[("none", DaoPrecision::None), ("thousandths", DaoPrecision::Thousandths), ("base91", DaoPrecision::Base91)],
                    )?
                    .unwrap_or(DaoPrecision::None),
            }),
        },
        telemetry: match o.object("telemetry", &["sequence", "analog", "digital"])? {
            None => None,
            Some(t) => Some(CommentTelemetry {
                sequence: t.required_int("sequence")?,
                analog: t
                    .array("analog")?
                    .map(|a| a.iter().map(|x| whole(x).and_then(|n| u16::try_from(n).ok())).collect::<Option<Vec<u16>>>())
                    .unwrap_or(Some(Vec::new()))
                    .ok_or_else(|| Unreadable::Unsupported("telemetry.analog: each is a whole number, 0-65535".into()))?,
                digital: t.int("digital")?,
            }),
        },
        frequency: match o
            .object("frequency", &["mhz", "tone", "tone_value", "offset_khz", "range", "range_km", "narrow", "ten_khz_resolution"])?
        {
            None => None,
            Some(f) => Some(VoiceFrequency {
                mhz: f.required_float("mhz")?,
                tone: f.choice(
                    "tone",
                    &[
                        ("off", Tone::Off),
                        ("tone", Tone::Tone),
                        ("ctcss", Tone::Ctcss),
                        ("dcs", Tone::Dcs),
                        ("tone-burst", Tone::ToneBurst),
                    ],
                )?,
                tone_value: f.int("tone_value")?,
                offset_khz: f.int("offset_khz")?,
                range: f.int("range")?,
                range_km: f.flag("range_km")?,
                narrow: f.flag("narrow")?,
                ten_khz_resolution: f.flag("ten_khz_resolution")?,
            }),
        },
        weather: read_weather(o)?,
        signpost: o.string("signpost")?,
        comment: o.text("comment")?,
    })
}

fn read_weather(o: &Keys) -> Read<Option<Weather>> {
    let Some(w) = o.object(
        "weather",
        &[
            "wind_direction_degrees",
            "wind_speed_mph",
            "wind_gust_mph",
            "temperature_f",
            "rain_1h_in",
            "rain_24h_in",
            "rain_midnight_in",
            "rain_raw",
            "humidity_percent",
            "pressure_mbar",
            "luminosity_w_m2",
            "snow_24h_in",
            "software",
            "unit",
            "extra",
        ],
    )?
    else {
        return Ok(None);
    };
    let extra = match w.array("extra")? {
        None => Vec::new(),
        Some(a) => a
            .iter()
            .map(|e| {
                let e = Keys::new(e, "weather.extra", &["letter", "value"])?;
                Ok(WeatherField { letter: e.required("letter", e.char("letter")?)?, value: e.text("value")? })
            })
            .collect::<Read<Vec<WeatherField>>>()?,
    };
    Ok(Some(Weather {
        wind_direction_degrees: w.int("wind_direction_degrees")?,
        wind_speed_mph: w.float("wind_speed_mph")?,
        wind_gust_mph: w.float("wind_gust_mph")?,
        temperature_f: w.float("temperature_f")?,
        rain_1h_in: w.float("rain_1h_in")?,
        rain_24h_in: w.float("rain_24h_in")?,
        rain_midnight_in: w.float("rain_midnight_in")?,
        rain_raw: w.int("rain_raw")?,
        humidity_percent: w.int("humidity_percent")?,
        pressure_mbar: w.float("pressure_mbar")?,
        luminosity_w_m2: w.int("luminosity_w_m2")?,
        snow_24h_in: w.float("snow_24h_in")?,
        software: w.char("software")?,
        unit: w.string("unit")?,
        extra,
    }))
}
