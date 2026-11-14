//! Builds a builder recipe (the vectors' `build` input form, as `tools/generate.py --recipes` writes it)
//! through this crate's real builder API, as a program would. The dump tool's build mode and the
//! vectors test runner both use it, so a case and a recipe go the same way.
//!
//! A recipe is `{"station": ..., "report": ..., "args": ...}`; only the keys it gives are passed to the
//! builder, and an absent one is never filled in.

use pdn_aprs::{
    BeamHeading, CommentTelemetry, EncodeError, MicEMessage, Packet, Phg, RefusalCode, Station, Symbol, Timestamp, Tone, WeatherBuilder,
};
use serde_json::{Map, Value};

use crate::neutral;

/// What building a recipe came to.
pub enum Outcome {
    /// The TNC2 line built.
    Built(Vec<u8>),
    /// The builder declined: the reason code's id, the message, and whether it was this reader that
    /// declined an input the builder cannot take (a bulletin id of two characters) rather than the builder.
    #[allow(dead_code)] // the flag is read by the dump tool, which shares this file
    Refused(String, String, bool),
    /// The builder has no way to say something in the recipe.
    Unsupported(String),
}

/// Why a recipe was not built: the builder has no way to say something in it (`unsupported`,
/// naming the key), or it declined (`refused`).
enum NotBuilt {
    Unsupported(String),
    Refused(RefusalCode, String),
    /// The reader declined an input the builder has no way to take, and its value alone decides the code.
    Tool(RefusalCode, String),
}

impl From<EncodeError> for NotBuilt {
    fn from(e: EncodeError) -> NotBuilt {
        NotBuilt::Refused(e.code(), e.message().to_string())
    }
}

type Built<T> = Result<T, NotBuilt>;

fn unsupported<T>(why: impl Into<String>) -> Built<T> {
    Err(NotBuilt::Unsupported(why.into()))
}

use crate::neutral::{is_size, rank};

/// The pairs of keys that give one quantity in two units. A recipe holding both is one input with
/// the quantity given twice.
const UNIT_PAIRS: [(&str, &str); 6] = [
    ("speed_knots", "speed_kmh"),
    ("altitude_feet", "altitude_m"),
    ("temperature_f", "temperature_c"),
    ("rain_1h_in", "rain_1h_mm"),
    ("rain_24h_in", "rain_24h_mm"),
    ("rain_midnight_in", "rain_midnight_mm"),
];

/// A recipe's arguments with one unit of each pair in `both` kept (`choice` bit i set keeps the
/// second unit of pair i).
fn with_units(input: &Value, both: &[(&str, &str)], choice: usize) -> Value {
    let mut v = input.clone();
    if let Some(args) = v.get_mut("args").and_then(Value::as_object_mut) {
        for (i, (first, second)) in both.iter().enumerate() {
            args.remove(if choice >> i & 1 == 1 { *first } else { *second });
        }
    }
    v
}

/// A recipe's arguments. Each one read is marked taken, so that one no step read (something the
/// builder has no way to say) can be named.
struct Args<'a> {
    o: &'a Map<String, Value>,
    taken: std::cell::RefCell<Vec<&'a str>>,
}

impl<'a> Args<'a> {
    fn new(v: &'a Value) -> Built<Args<'a>> {
        match v.as_object() {
            Some(o) => Ok(Args { o, taken: Default::default() }),
            None => unsupported("args: not an object"),
        }
    }

    fn get(&self, k: &str) -> Option<&'a Value> {
        let (key, v) = self.o.get_key_value(k)?;
        self.taken.borrow_mut().push(key.as_str());
        (!v.is_null()).then_some(v)
    }

    fn has(&self, k: &str) -> bool {
        self.o.get(k).is_some_and(|v| !v.is_null())
    }

    fn float(&self, k: &str) -> Built<Option<f64>> {
        match self.get(k) {
            None => Ok(None),
            Some(v) => neutral::num(v).map(Some).ok_or_else(|| NotBuilt::Unsupported(format!("{k}: not a number"))),
        }
    }

    fn required_float(&self, k: &str) -> Built<f64> {
        self.float(k)?.ok_or_else(|| NotBuilt::Unsupported(format!("{k}: missing")))
    }

    /// A whole number of the type the builder's method takes, except that a number the builder's type cannot take but whose value alone decides
    /// a refusal (NaN or an infinity: `not-a-number`; a whole number outside the type: `value-out-of-range`)
    /// is noted as that refusal and left out, for the builder to find any earlier code.
    fn whole_or_defer<T: TryFrom<i64>>(&self, k: &str, what: &str) -> Built<Option<T>> {
        let Some(v) = self.get(k) else { return Ok(None) };
        if let Some(t) = whole(v).and_then(|x| T::try_from(x).ok()) {
            return Ok(Some(t));
        }
        let fraction = neutral::num(v).is_some_and(|f| f.is_finite() && f.fract() != 0.0);
        match neutral::num(v) {
            Some(f) if !f.is_finite() => {
                neutral::defer(RefusalCode::NotANumber, format!("{k}: not a number"));
                Ok(None)
            }
            Some(_) if !fraction => {
                neutral::defer(RefusalCode::ValueOutOfRange, format!("{k}: outside what the builder takes ({what})"));
                Ok(None)
            }
            _ => unsupported(format!("{k}: the builder takes {what}")),
        }
    }

    fn text(&self, k: &str) -> Built<Option<&'a str>> {
        match self.get(k) {
            None => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.as_str())),
            Some(_) => unsupported(format!("{k}: not text")),
        }
    }

    fn flag(&self, k: &str) -> Built<bool> {
        match self.get(k) {
            None => Ok(false),
            Some(Value::Bool(b)) => Ok(*b),
            Some(_) => unsupported(format!("{k}: not a boolean")),
        }
    }

    /// Fails naming the first argument no step read.
    fn all_taken(&self) -> Built<()> {
        let taken = self.taken.borrow();
        match self.o.keys().find(|k| !taken.contains(&k.as_str())) {
            Some(k) => unsupported(format!("{k}: the builder has no such option")),
            None => Ok(()),
        }
    }
}

fn whole(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| neutral::num(v).filter(|f| f.fract() == 0.0 && f.abs() < 1e15).map(|f| f as i64))
}

/// The options every positioned builder shares: the same methods on each builder type.
macro_rules! positioned {
    ($b:expr, $args:expr) => {{
        let args: &Args = $args;
        let mut b = $b;
        if let Some(s) = symbol(args.text("symbol")?)? {
            b = b.symbol(s);
        }
        if let Some(c) = args.float("course_degrees")? {
            b = b.course_degrees(c);
        }
        if args.has("speed_knots") && args.has("speed_kmh") {
            return unsupported("speed_kmh: the builder takes one speed");
        }
        if let Some(s) = args.float("speed_knots")? {
            b = b.speed_knots(s);
        }
        if let Some(s) = args.float("speed_kmh")? {
            b = b.speed_kmh(s);
        }
        if args.has("altitude_feet") && args.has("altitude_m") {
            return unsupported("altitude_m: the builder takes one altitude");
        }
        if let Some(a) = args.float("altitude_feet")? {
            b = b.altitude_feet(a);
        }
        if let Some(a) = args.float("altitude_m")? {
            b = b.altitude_m(a);
        }
        if let Some(c) = args.text("comment")? {
            b = b.comment(c);
        }
        if let Some(p) = args.get("phg") {
            b = b.phg(phg(p)?);
        }
        if let Some(r) = args.float("range_miles")? {
            b = b.range_miles(r);
        }
        if let Some(f) = args.get("frequency") {
            let Frequency { mhz, tone, value, offset } = frequency(f)?;
            b = b.frequency(mhz);
            // A tone value with no kind is the plain tone, as it always was.
            if let Some(value) = value {
                let value = Some(value)
                    .filter(|v| v.fract() == 0.0 && (0.0..65536.0).contains(v))
                    .ok_or_else(|| NotBuilt::Unsupported("frequency.tone_value: the builder takes a whole number".into()))?;
                b = b.tone(tone.unwrap_or(Tone::Tone), value as u16);
            } else if tone.is_some() {
                return unsupported("frequency.tone: the builder takes a tone with its value");
            }
            if let Some(khz) = offset {
                b = b.offset_khz(khz);
            }
        }
        if args.flag("compressed")? {
            b = b.compressed();
        }
        if let Some(a) = args.whole_or_defer::<u8>("ambiguity", "0-255 digits")? {
            b = b.ambiguity(a);
        }
        if args.flag("dao")? {
            b = b.dao();
        }
        if let Some(t) = args.get("telemetry") {
            b = b.telemetry(comment_telemetry(t)?);
        }
        b
    }};
}

fn build(input: &Value) -> Built<Packet> {
    let station = station(&input["station"])?;
    let args = Args::new(input.get("args").unwrap_or(&Value::Null))?;
    let report = input.get("report").and_then(Value::as_str).unwrap_or("");
    let packet = match report {
        "position" => {
            let mut b = station.position(args.required_float("latitude")?, args.required_float("longitude")?);
            b = positioned!(b, &args);
            if args.flag("messaging")? {
                b = b.messaging();
            }
            if let Some(t) = timestamp(&args)? {
                b = b.timestamp(t);
            }
            args.all_taken()?;
            b.build()?
        }
        "object" => {
            let mut b = station.object(args.text("name")?.unwrap_or_default());
            b = b.at(args.required_float("latitude")?, args.required_float("longitude")?);
            b = positioned!(b, &args);
            if let Some(t) = timestamp(&args)? {
                b = b.timestamp(t);
            }
            if args.flag("killed")? {
                b = b.killed();
            }
            args.all_taken()?;
            b.build()?
        }
        "item" => {
            let mut b = station.item(args.text("name")?.unwrap_or_default());
            b = b.at(args.required_float("latitude")?, args.required_float("longitude")?);
            b = positioned!(b, &args);
            if args.flag("killed")? {
                b = b.killed();
            }
            args.all_taken()?;
            b.build()?
        }
        "mic-e" => {
            let mut b = station.mic_e(args.required_float("latitude")?, args.required_float("longitude")?);
            b = positioned!(b, &args);
            if let Some(m) = args.text("mic_e_message")? {
                b = b.mic_e_message(mic_e_message(m)?);
            }
            if args.flag("messaging")? {
                b = b.messaging();
            }
            args.all_taken()?;
            b.build()?
        }
        "weather" => weather(&station, &args)?,
        "message" => {
            let mut b = station.message(args.text("addressee")?.unwrap_or_default(), args.text("text")?.unwrap_or_default());
            if let Some(id) = args.text("message_id")? {
                b = b.message_id(id);
            }
            if let Some(r) = args.text("reply_ack")? {
                b = b.reply_ack(r);
            }
            args.all_taken()?;
            b.build()?
        }
        "ack" | "reject" => {
            let (addressee, id) = (args.text("addressee")?.unwrap_or_default(), args.text("message_id")?.unwrap_or_default());
            let mut b = if report == "ack" { station.ack(addressee, id) } else { station.reject(addressee, id) };
            if let Some(r) = args.text("reply_ack")? {
                b = b.reply_ack(r);
            }
            args.all_taken()?;
            b.build()?
        }
        "bulletin" => {
            // An id of the wrong length is `invalid-addressee`, noted and stood in for by `0`; one left out
            // is a record the builder has no answer for.
            let id = match args.text("id")? {
                None => return unsupported("id: missing"),
                Some(t) => one_char(t).unwrap_or_else(|| {
                    neutral::defer(RefusalCode::InvalidAddressee, "id: a bulletin id is one character".into());
                    '0'
                }),
            };
            let text = args.text("text")?.unwrap_or_default();
            let b = match args.text("group")? {
                Some(group) => station.group_bulletin(id, group, text),
                None => station.bulletin(id, text),
            };
            args.all_taken()?;
            b.build()?
        }
        "status" => {
            let mut b = station.status(args.text("text")?.unwrap_or_default());
            if let Some(t) = timestamp(&args)? {
                b = b.timestamp(t);
            }
            if let Some(locator) = args.text("locator")? {
                b = b.locator(locator);
            }
            if let Some(symbol) = symbol(args.text("symbol")?)? {
                b = b.symbol(symbol);
            }
            if let Some(beam) = args.get("beam") {
                let code = |k: &str| {
                    beam.get(k)
                        .and_then(Value::as_str)
                        .and_then(one_char)
                        .ok_or_else(|| NotBuilt::Unsupported(format!("beam.{k}: the builder takes one character")))
                };
                b = b.beam(BeamHeading { heading_code: code("heading_code")?, power_code: code("power_code")? });
            }
            args.all_taken()?;
            b.build()?
        }
        "telemetry" => {
            // The builder takes the sequence as an argument, so one left out is built with a placeholder, to
            // find any fault ranked before `missing-field`; failing that it is the refusal the rules give,
            // never a default of 0.
            let sequence = args.whole_or_defer::<u16>("sequence", "a number 0-65535")?;
            let mut b = station.telemetry(sequence.unwrap_or(0));
            if let Some(a) = args.get("analog") {
                let Some(a) = a.as_array() else { return unsupported("analog: not a list") };
                // A channel may be empty on its own, and the ones not given are sent empty.
                let given = a.iter().rposition(|v| !v.is_null()).map_or(0, |i| i + 1);
                let values = a[..given]
                    .iter()
                    .map(|v| if v.is_null() { Ok(None) } else { neutral::num(v).map(Some).ok_or(()) })
                    .collect::<Result<Vec<Option<f64>>, ()>>()
                    .map_err(|()| NotBuilt::Unsupported("analog: not a number".into()))?;
                b = b.analog(&values);
            }
            if let Some(bits) = args.text("bits")? {
                b = b.bits(bits);
            }
            if let Some(c) = args.text("comment")? {
                b = b.comment(c);
            }
            args.all_taken()?;
            match (sequence, b.build()) {
                (Some(_), built) => built?,
                (None, Err(e)) if rank(e.code()) < rank(RefusalCode::MissingField) && !is_size(e.code(), e.message()) => {
                    return Err(e.into());
                }
                (None, _) => {
                    return Err(NotBuilt::Refused(RefusalCode::MissingField, "a telemetry report needs a sequence number".into()));
                }
            }
        }
        "telemetry-names" | "telemetry-units" => {
            let addressee = args.text("addressee")?;
            let labels = strings(&args, if report == "telemetry-names" { "names" } else { "units" })?;
            let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
            let mut b = if report == "telemetry-names" { station.telemetry_names(&labels) } else { station.telemetry_units(&labels) };
            if let Some(a) = addressee {
                b = b.addressee(a);
            }
            args.all_taken()?;
            b.build()?
        }
        "telemetry-coefficients" => {
            let addressee = args.text("addressee")?;
            let coefficients = match args.get("coefficients") {
                None => Vec::new(),
                Some(a) => a
                    .as_array()
                    .and_then(|a| a.iter().map(neutral::num).collect::<Option<Vec<f64>>>())
                    .ok_or_else(|| NotBuilt::Unsupported("coefficients: the builder takes numbers".into()))?,
            };
            let mut b = station.telemetry_coefficients(&coefficients);
            if let Some(a) = addressee {
                b = b.addressee(a);
            }
            args.all_taken()?;
            b.build()?
        }
        "telemetry-bits" => {
            let addressee = args.text("addressee")?;
            let mut b = station.telemetry_bits(args.text("bits")?.unwrap_or_default(), args.text("project")?.unwrap_or_default());
            if let Some(a) = addressee {
                b = b.addressee(a);
            }
            args.all_taken()?;
            b.build()?
        }
        _ => return unsupported("report: the builder has no such report"),
    };
    Ok(packet)
}

fn station(v: &Value) -> Built<Station> {
    let text = |k: &str| v.get(k).and_then(Value::as_str);
    let refused = |e: pdn_aprs::InvalidAddress| NotBuilt::Refused(RefusalCode::InvalidAddress, e.to_string());
    let mut station = Station::new(text("source").unwrap_or("")).map_err(refused)?;
    if let Some(d) = text("destination") {
        station = station.to(d).map_err(refused)?;
    }
    if let Some(path) = v.get("path").and_then(Value::as_array) {
        let path: Vec<&str> = path.iter().filter_map(Value::as_str).collect();
        station = station.via(&path).map_err(refused)?;
    }
    Ok(station)
}

fn strings(args: &Args, k: &str) -> Built<Vec<String>> {
    match args.get(k) {
        None => Ok(Vec::new()),
        Some(Value::Array(a)) => {
            a.iter().map(|x| x.as_str().map(str::to_string).ok_or_else(|| NotBuilt::Unsupported(format!("{k}: not text")))).collect()
        }
        Some(_) => unsupported(format!("{k}: not a list")),
    }
}

fn one_char(t: &str) -> Option<char> {
    let mut c = t.chars();
    match (c.next(), c.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    }
}

fn symbol(text: Option<&str>) -> Built<Option<Symbol>> {
    let Some(t) = text else { return Ok(None) };
    let mut c = t.chars();
    match (c.next(), c.next(), c.next()) {
        (Some(table), Some(code), None) => Ok(Some(Symbol::new(table, code))),
        // Not a table and a code: `invalid-symbol`, noted; the symbol is left out for the builder to find any
        // earlier code (it would refuse a missing one as `invalid-symbol` too).
        _ => {
            neutral::defer(RefusalCode::InvalidSymbol, "symbol: not a table and a code".into());
            Ok(None)
        }
    }
}

fn phg(p: &Value) -> Built<Phg> {
    if let Some(k) =
        p.as_object().and_then(|o| o.keys().find(|k| !["power", "height", "gain", "directivity", "beacons_per_hour"].contains(&k.as_str())))
    {
        return unsupported(format!("phg.{k}: the builder has no such code"));
    }
    let code = |k: &str| {
        p.get(k)
            .and_then(whole)
            .and_then(|v| u8::try_from(v).ok())
            .ok_or_else(|| NotBuilt::Unsupported(format!("phg.{k}: the builder takes a code 0-255")))
    };
    let rate = match p.get("beacons_per_hour") {
        None | Some(Value::Null) => None,
        Some(_) => Some(code("beacons_per_hour")?),
    };
    Ok(Phg {
        power: code("power")?,
        height: code("height")?,
        gain: code("gain")?,
        directivity: code("directivity")?,
        beacons_per_hour: rate,
    })
}

/// A recipe's frequency.
struct Frequency {
    mhz: f64,
    tone: Option<Tone>,
    /// The tone's value, in Hz or the DCS code.
    value: Option<f64>,
    offset: Option<i32>,
}

fn frequency(f: &Value) -> Built<Frequency> {
    if let Some(k) = f.as_object().and_then(|o| o.keys().find(|k| !["mhz", "tone", "tone_value", "offset_khz"].contains(&k.as_str()))) {
        return unsupported(format!("frequency.{k}: the builder has no such option"));
    }
    let mhz = f.get("mhz").and_then(neutral::num).ok_or_else(|| NotBuilt::Unsupported("frequency.mhz: missing".into()))?;
    let tone = match f.get("tone").and_then(Value::as_str) {
        None => None,
        Some("tone") => Some(Tone::Tone),
        Some("ctcss") => Some(Tone::Ctcss),
        Some("dcs") => Some(Tone::Dcs),
        Some(_) => return unsupported("frequency.tone: the builder has no such tone"),
    };
    let value = f.get("tone_value").and_then(neutral::num);
    let offset = match f.get("offset_khz") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            whole(v)
                .and_then(|v| i32::try_from(v).ok())
                .ok_or_else(|| NotBuilt::Unsupported("frequency.offset_khz: the builder takes whole kHz".into()))?,
        ),
    };
    Ok(Frequency { mhz, tone, value, offset })
}

/// Base-91 comment telemetry: the sequence, the analog values and any digital bits.
fn comment_telemetry(t: &Value) -> Built<CommentTelemetry> {
    let value = |v: &Value| whole(v).and_then(|v| u16::try_from(v).ok());
    let sequence =
        t.get("sequence").and_then(value).ok_or_else(|| NotBuilt::Unsupported("telemetry.sequence: the builder takes 0-65535".into()))?;
    let analog = t
        .get("analog")
        .and_then(Value::as_array)
        .map_or(Some(Vec::new()), |a| a.iter().map(value).collect::<Option<Vec<u16>>>())
        .ok_or_else(|| NotBuilt::Unsupported("telemetry.analog: the builder takes values 0-65535".into()))?;
    let digital = match t.get("digital") {
        None | Some(Value::Null) => None,
        Some(d) => Some(
            whole(d)
                .and_then(|v| u8::try_from(v).ok())
                .ok_or_else(|| NotBuilt::Unsupported("telemetry.digital: the builder takes 0-255".into()))?,
        ),
    };
    Ok(CommentTelemetry { sequence, analog, digital })
}

fn mic_e_message(t: &str) -> Built<MicEMessage> {
    Ok(match t {
        "off-duty" => MicEMessage::OffDuty,
        "en-route" => MicEMessage::EnRoute,
        "in-service" => MicEMessage::InService,
        "returning" => MicEMessage::Returning,
        "committed" => MicEMessage::Committed,
        "special" => MicEMessage::Special,
        "priority" => MicEMessage::Priority,
        "emergency" => MicEMessage::Emergency,
        custom => match custom.strip_prefix("custom").and_then(|n| n.parse().ok()) {
            Some(n) => MicEMessage::Custom(n),
            None => return unsupported("mic_e_message: the builder has no such message"),
        },
    })
}

/// `{"utc": "2026-09-27T09:23:45Z", "format": "dhm"}` as the timestamp a program would give the
/// builder: day, hours and minutes (`dhm`), hours, minutes and seconds (`hms`), or month, day,
/// hours and minutes (`mdhm`).
fn timestamp(args: &Args) -> Built<Option<Timestamp>> {
    let Some(t) = args.get("timestamp") else { return Ok(None) };
    let utc = t.get("utc").and_then(Value::as_str).unwrap_or("");
    let field = |at: usize| {
        utc.get(at..at + 2)
            .and_then(|s| s.parse::<u8>().ok())
            .ok_or_else(|| NotBuilt::Unsupported("timestamp.utc: not an ISO 8601 time".into()))
    };
    let (month, day, hour, minute, second) = (field(5)?, field(8)?, field(11)?, field(14)?, field(17)?);
    Ok(Some(match t.get("format").and_then(Value::as_str).unwrap_or("dhm") {
        "dhm" => Timestamp::dhm(day, hour, minute),
        "hms" => Timestamp::hms(hour, minute, second),
        "mdhm" => Timestamp::mdhm(month, day, hour, minute),
        _ => return unsupported("timestamp.format: the builder has no such format"),
    }))
}

fn weather(station: &Station, args: &Args) -> Built<Packet> {
    let mut b = station.weather();
    match (args.float("latitude")?, args.float("longitude")?) {
        (Some(lat), Some(lon)) => b = b.at(lat, lon),
        (None, None) => {}
        _ => return unsupported("latitude: the builder takes a latitude and a longitude together"),
    }
    if let Some(s) = symbol(args.text("symbol")?)? {
        b = b.symbol(s);
    }
    if let Some(t) = timestamp(args)? {
        b = b.timestamp(t);
    }
    if let Some(d) = args.float("wind_direction_degrees")? {
        b = b.wind_direction_degrees(d);
    }
    if let Some(s) = args.float("wind_speed_mph")? {
        b = b.wind_speed_mph(s);
    }
    if let Some(g) = args.float("wind_gust_mph")? {
        b = b.wind_gust_mph(g);
    }
    if args.has("temperature_f") && args.has("temperature_c") {
        return unsupported("temperature_c: the builder takes one temperature");
    }
    if let Some(t) = args.float("temperature_f")? {
        b = b.temperature_f(t);
    }
    if let Some(t) = args.float("temperature_c")? {
        b = b.temperature_c(t);
    }
    type Rain = fn(WeatherBuilder, f64) -> WeatherBuilder;
    let rain: [(&str, &str, Rain, Rain); 3] = [
        ("rain_1h_in", "rain_1h_mm", WeatherBuilder::rain_1h_in, WeatherBuilder::rain_1h_mm),
        ("rain_24h_in", "rain_24h_mm", WeatherBuilder::rain_24h_in, WeatherBuilder::rain_24h_mm),
        ("rain_midnight_in", "rain_midnight_mm", WeatherBuilder::rain_midnight_in, WeatherBuilder::rain_midnight_mm),
    ];
    for (inches, mm, set_in, set_mm) in rain {
        if args.has(inches) && args.has(mm) {
            return unsupported(format!("{mm}: the builder takes one rainfall for each period"));
        }
        if let Some(v) = args.float(inches)? {
            b = set_in(b, v);
        }
        if let Some(v) = args.float(mm)? {
            b = set_mm(b, v);
        }
    }
    if let Some(h) = args.float("humidity_percent")? {
        b = b.humidity_percent(h);
    }
    if let Some(p) = args.float("pressure_mbar")? {
        b = b.pressure_mbar(p);
    }
    if let Some(l) = args.float("luminosity_w_m2")? {
        b = b.luminosity_w_m2(l);
    }
    if let Some(s) = args.float("snow_24h_in")? {
        b = b.snow_24h_in(s);
    }
    args.all_taken()?;
    Ok(b.build()?)
}

/// Builds a recipe once, settling what the reader noted about inputs the builder cannot take against what the
/// builder says: the builder's refusal when it is earlier in the order than every noted one, else the first
/// noted one, which is the reader's.
fn settled(input: &Value) -> Built<Packet> {
    neutral::clear_deferred();
    let result = build(input);
    let (code, why) = match &result {
        Ok(_) => (None, String::new()),
        Err(NotBuilt::Refused(code, why)) => (Some(*code), why.clone()),
        // A record the builder cannot say stays unsupported whatever else the reader noted.
        Err(_) => {
            neutral::clear_deferred();
            return result;
        }
    };
    match neutral::settle(match code {
        None => Ok(result),
        Some(code) => Err((code, why)),
    }) {
        Ok(result) => result,
        Err((code, why, true)) => Err(NotBuilt::Tool(code, why)),
        Err((code, why, false)) => Err(NotBuilt::Refused(code, why)),
    }
}

/// Builds a recipe. A quantity given in two units is built once with each, and refused with the first code
/// in the order among those results and `value-given-twice`: never a line from one unit.
pub fn outcome(input: &Value) -> Outcome {
    let both: Vec<(&str, &str)> = UNIT_PAIRS
        .iter()
        .copied()
        .filter(|(a, b)| {
            input.get("args").is_some_and(|args| args.get(a).is_some_and(|v| !v.is_null()) && args.get(b).is_some_and(|v| !v.is_null()))
        })
        .collect();
    if !both.is_empty() {
        let mut best = RefusalCode::ValueGivenTwice;
        let mut why = "a quantity is given in two units".to_string();
        let mut by_tool = false;
        for choice in 0..1usize << both.len() {
            match settled(&with_units(input, &both, choice)) {
                Ok(_) => {}
                Err(NotBuilt::Unsupported(why)) => return Outcome::Unsupported(why),
                Err(NotBuilt::Refused(code, text)) | Err(NotBuilt::Tool(code, text))
                    if rank(code) >= rank(best) || is_size(code, &text) => {}
                Err(NotBuilt::Refused(code, text)) => (best, why, by_tool) = (code, text, false),
                Err(NotBuilt::Tool(code, text)) => (best, why, by_tool) = (code, text, true),
            }
        }
        return Outcome::Refused(best.id().to_string(), why, by_tool);
    }
    match settled(input) {
        Ok(p) => Outcome::Built(p.to_tnc2()),
        Err(NotBuilt::Unsupported(why)) => Outcome::Unsupported(why),
        Err(NotBuilt::Refused(code, why)) => Outcome::Refused(code.id().to_string(), why, false),
        Err(NotBuilt::Tool(code, why)) => Outcome::Refused(code.id().to_string(), why, true),
    }
}
