//! Differential comparison with other implementations of the conformance vectors: the dump tool
//! packet-net/aprs-vectors describes in its README ("Comparing implementations"), whose
//! `tools/compare.py` compares any number of such dumps. Every file is gzip-compressed JSON lines,
//! and the output keeps the input's order.
//!
//! - decode (the default): reads a capture extracted as one hex-encoded TNC2 line per line, as
//!   packet.net's `aprs-corpus diff lines` writes it, and writes the lenient and strict results in
//!   the vectors' neutral form, what re-encoding gives and the bytes it wrote (`written`, and
//!   `written_destination` for Mic-E), and the API view: what the packet's own accessors say.
//! - `--encode`: reads data in the neutral form (`tools/generate.py`), encodes it, and writes the
//!   bytes, or the refusal, and what they decode to.
//! - `--build`: reads builder recipes (`tools/generate.py --recipes`), builds each with the
//!   builder as a program would, and writes the TNC2 line, or the refusal, and what it decodes to.
//!
//! ```sh
//! cargo run --release --example diff_dump -- lines.hex.gz rs.jsonl.gz
//! cargo run --release --example diff_dump -- --encode data.jsonl.gz rs.jsonl.gz
//! cargo run --release --example diff_dump -- --build recipes.jsonl.gz rs.jsonl.gz
//! ```

#[path = "../tests/vectors/compare.rs"]
mod compare;
#[path = "../tests/vectors/neutral.rs"]
#[allow(dead_code)]
mod neutral;

use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};

use flate2::Compression;
use flate2::read::MultiGzDecoder;
use flate2::write::GzEncoder;
use pdn_aprs::{
    Address, BeamHeading, CommentTelemetry, Data, EncodeError, MicEMessage, Packet, ParseOptions, Phg, RefusalCode, Station, Symbol,
    Timestamp, Tone, WeatherBuilder,
};
use serde_json::{Map, Value, json};

enum Mode {
    Decode,
    Encode,
    Build,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mode, files) = match args.first().map(String::as_str) {
        Some("--encode") => (Mode::Encode, &args[1..]),
        Some("--build") => (Mode::Build, &args[1..]),
        _ => (Mode::Decode, &args[..]),
    };
    if files.len() != 2 {
        eprintln!("diff_dump [--encode | --build] <in.gz> <out.jsonl.gz>");
        std::process::exit(2);
    }
    let input = BufReader::new(MultiGzDecoder::new(File::open(&files[0])?));
    let mut out = BufWriter::new(GzEncoder::new(File::create(&files[1])?, Compression::fast()));
    let mut n: u64 = 0;
    for line in input.lines() {
        let line = line?;
        let record = match mode {
            Mode::Decode => decode_record(n, &hex_bytes(&line)),
            Mode::Encode => encode_record(&neutral::read_json(&line)?),
            Mode::Build => build_record(&neutral::read_json(&line)?),
        };
        serde_json::to_writer(&mut out, &record)?;
        out.write_all(b"\n")?;
        n += 1;
    }
    out.into_inner().map_err(|e| e.into_error())?.finish()?;
    println!("{}: {n} records", files[1]);
    Ok(())
}

fn hex_bytes(text: &str) -> Vec<u8> {
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap_or(0)).collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn address(text: &str) -> Address {
    Address::new(text).expect("a well-formed address")
}

// ------------------------------------------------------------------ decode

fn decode_record(n: u64, line: &[u8]) -> Value {
    let (lenient, packet) = result(line, ParseOptions::LENIENT);
    let (strict, _) = result(line, ParseOptions::STRICT);
    let (how, written, refusal) = reencode(packet.as_ref());
    let mut record = json!({"n": n, "lenient": lenient, "strict": strict, "reencode": how});
    if let Some(code) = refusal {
        record["reencode_refusal"] = json!(code.id());
    }
    if let Some((info, destination)) = written {
        record["written"] = json!(hex(&info));
        if let Some(destination) = destination {
            record["written_destination"] = json!(destination);
        }
    }
    if let Some(p) = &packet {
        record["api"] = api(p);
    }
    record
}

/// The header, data and diagnostics, or the header error's diagnostics.
fn result(line: &[u8], options: ParseOptions) -> (Value, Option<Packet>) {
    match Packet::decode_tnc2(line, options) {
        Err(e) => (json!({"header_error": neutral::diagnostics(&e.diagnostics)}), None),
        Ok(p) => {
            let mut o = Map::new();
            o.insert("header".into(), neutral::header(&p));
            o.insert("data".into(), neutral::data(&p.data));
            o.insert("diagnostics".into(), json!(neutral::diagnostics(&p.diagnostics)));
            (Value::Object(o), Some(p))
        }
    }
}

/// The information field and, for Mic-E, the destination an encoder wrote.
type Written = Option<(Vec<u8>, Option<String>)>;

/// What encoding the lenient data again gives, as the vectors' reencode check sees it, and what it
/// wrote.
fn reencode(packet: Option<&Packet>) -> (&'static str, Written, Option<RefusalCode>) {
    let Some(packet) = packet else { return ("none", None, None) };
    if matches!(packet.data, Data::Unrecognized(_)) {
        return ("none", None, None);
    }
    let written = match &packet.data {
        Data::MicE(m) => Packet::create(packet.source.clone(), packet.destination.clone(), packet.path.clone(), Data::MicE(m.clone()))
            .map(|p| (p.information, p.destination)),
        data => data.encode().map(|info| (info, packet.destination.clone())),
    };
    let (info, destination) = match written {
        Ok(w) => w,
        Err(e) => return ("refused", None, Some(e.code())),
    };
    let wrote = Some((info.clone(), matches!(packet.data, Data::MicE(_)).then(|| destination.as_str().to_string())));
    let mut original: &[u8] = &packet.information;
    while let [rest @ .., b'\r' | b'\n'] = original {
        original = rest;
    }
    if info == original && destination == packet.destination {
        return ("identical", wrote, None);
    }
    let again = Packet::decode(packet.source.clone(), destination, packet.path.clone(), &info, ParseOptions::LENIENT);
    let mut differences = Vec::new();
    compare::differences(&neutral::data(&packet.data), &neutral::data(&again.data), "", &mut differences);
    let how = if differences.is_empty() && !again.has_warnings() && !again.has_errors() { "equivalent" } else { "fails" };
    (how, wrote, None)
}

/// The API view: what the packet's own public accessors say, as a program would read them.
fn api(p: &Packet) -> Value {
    let mut o = Map::new();
    o.insert("source".into(), json!(p.source.as_str()));
    o.insert("destination".into(), json!(p.destination.as_str()));
    let path: Vec<String> = p.path.iter().map(|e| format!("{}{}", e.address, if e.used { "*" } else { "" })).collect();
    o.insert("path".into(), json!(path));
    let q = p.q_construct().map(|q| {
        let mut qo = Map::new();
        qo.insert("construct".into(), json!(q.construct));
        if let Some(station) = q.station {
            qo.insert("station".into(), json!(station.as_str()));
        }
        Value::Object(qo)
    });
    o.insert("q_construct".into(), q.unwrap_or(Value::Null));
    o.insert("third_party".into(), json!(p.third_party));
    o.insert("has_errors".into(), json!(p.has_errors()));
    o.insert("has_warnings".into(), json!(p.has_warnings()));
    o.insert("tnc2".into(), json!(hex(&p.to_tnc2())));
    match p.to_ax25() {
        Ok(frame) => o.insert("ax25".into(), json!(hex(&frame))),
        Err(e) => {
            o.insert("ax25_refusal".into(), json!(e.code().id()));
            o.insert("ax25".into(), json!("refused"))
        }
    };
    let device = p.device().map(|d| {
        let mut named = Map::new();
        for (k, v) in [("vendor", d.vendor), ("model", d.model), ("class", d.class)] {
            if let Some(v) = v {
                named.insert(k.into(), json!(v));
            }
        }
        Value::Object(named)
    });
    o.insert("device".into(), device.unwrap_or(Value::Null));
    let (positioned, status_symbol) = match &p.data {
        Data::Position(r) => (Some(&r.fields), None),
        Data::MicE(r) => (Some(&r.fields), None),
        Data::Object(r) => (Some(&r.fields), None),
        Data::Item(r) => (Some(&r.fields), None),
        Data::Status(r) => (None, r.symbol),
        _ => (None, None),
    };
    if let Some(description) = positioned.map(|f| f.symbol).or(status_symbol).and_then(|s| s.description()) {
        o.insert("symbol".into(), json!({"description": description}));
    }
    if let Some(phg) = positioned.and_then(|f| f.phg) {
        o.insert(
            "phg".into(),
            json!({
                "watts": phg.power_watts(),
                "height_feet": phg.height_feet(),
                "gain_db": phg.gain_db(),
                "directivity_degrees": phg.directivity_degrees(),
            }),
        );
    }
    if let Data::ThirdParty(inner) = &p.data {
        o.insert("inner".into(), api(inner));
    }
    Value::Object(o)
}

// ------------------------------------------------------------------ encode

fn encode_record(input: &Value) -> Value {
    let n = input.get("n").cloned().unwrap_or(Value::Null);
    let data = match neutral::try_read_data(&input["data"]) {
        Ok(data) => data,
        Err(neutral::Unreadable::Unsupported(why)) => return json!({"n": n, "result": "unsupported", "reason": why}),
        Err(neutral::Unreadable::Refused(code, why)) => return json!({"n": n, "result": "refused", "reason": why, "refusal": code.id()}),
    };
    let written = match data {
        Data::MicE(m) => {
            Packet::create(address("N0CALL"), address("APZ001"), Vec::new(), Data::MicE(m)).map(|p| (p.information, p.destination))
        }
        data => data.encode().map(|info| (info, address("APZ001"))),
    };
    match written {
        Err(e) => json!({"n": n, "result": "refused", "reason": e.message(), "refusal": e.code().id()}),
        Ok((info, destination)) => {
            // The bytes decoded leniently under N0CALL> and the destination.
            let p = Packet::decode(address("N0CALL"), destination.clone(), Vec::new(), &info, ParseOptions::LENIENT);
            let again = json!({"data": neutral::data(&p.data), "diagnostics": neutral::diagnostics(&p.diagnostics)});
            json!({"n": n, "result": "written", "info": hex(&info), "destination": destination.as_str(), "again": again})
        }
    }
}

// ------------------------------------------------------------------ build

/// Why a recipe was not built: the builder has no way to say something in it (`unsupported`,
/// naming the key), or it declined (`refused`).
enum NotBuilt {
    Unsupported(String),
    Refused(RefusalCode, String),
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

/// The Refusal reasons order (vectors README): the first code that applies is the one named.
const ORDER: [&str; 26] = [
    "not-encodable",
    "invalid-address",
    "ax25-cannot-carry",
    "tnc2-cannot-carry",
    "invalid-third-party-header",
    "invalid-addressee",
    "invalid-name",
    "invalid-timestamp",
    "not-a-number",
    "value-out-of-range",
    "value-has-no-form",
    "invalid-code",
    "invalid-symbol",
    "invalid-character",
    "too-long",
    "padding-space",
    "malformed",
    "invalid-message-id",
    "invalid-nmea",
    "extension-conflict",
    "field-not-allowed",
    "symbol-mismatch",
    "missing-field",
    "value-given-twice",
    "text-reads-as-field",
    "does-not-read-back",
];

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

fn build_record(input: &Value) -> Value {
    let n = input.get("n").cloned().unwrap_or(Value::Null);
    let both: Vec<(&str, &str)> = UNIT_PAIRS
        .iter()
        .copied()
        .filter(|(a, b)| {
            input.get("args").is_some_and(|args| args.get(a).is_some_and(|v| !v.is_null()) && args.get(b).is_some_and(|v| !v.is_null()))
        })
        .collect();
    if !both.is_empty() {
        // One quantity in two units: every value is checked on its own, by building once with each
        // unit, and the refusal is the first code in the order among those results and
        // `value-given-twice`. A line is never built from one unit.
        let mut best = "value-given-twice".to_string();
        let mut why = "a quantity is given in two units".to_string();
        for choice in 0..1usize << both.len() {
            match build(&with_units(input, &both, choice)) {
                Ok(_) => {}
                Err(NotBuilt::Unsupported(why)) => return json!({"n": n, "result": "unsupported", "reason": why}),
                Err(NotBuilt::Refused(code, text)) => {
                    let rank = |c: &str| ORDER.iter().position(|x| *x == c).unwrap_or(ORDER.len());
                    if rank(code.id()) < rank(&best) {
                        best = code.id().to_string();
                        why = text;
                    }
                }
            }
        }
        return json!({"n": n, "result": "refused", "reason": why, "refusal": best});
    }
    match build(input) {
        Ok(p) => {
            let line = p.to_tnc2();
            let (decoded, _) = result(&line, ParseOptions::LENIENT);
            json!({"n": n, "result": "built", "tnc2": hex(&line), "again": decoded})
        }
        Err(NotBuilt::Unsupported(why)) => json!({"n": n, "result": "unsupported", "reason": why}),
        Err(NotBuilt::Refused(code, why)) => json!({"n": n, "result": "refused", "reason": why, "refusal": code.id()}),
    }
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

    /// A whole number of the type the builder's method takes.
    fn whole<T: TryFrom<i64>>(&self, k: &str, what: &str) -> Built<Option<T>> {
        let Some(v) = self.get(k) else { return Ok(None) };
        whole(v).and_then(|x| T::try_from(x).ok()).map(Some).ok_or_else(|| NotBuilt::Unsupported(format!("{k}: the builder takes {what}")))
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
        if let Some(a) = args.whole::<u8>("ambiguity", "0-255 digits")? {
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
            let Some(id) = one_char(args.text("id")?.unwrap_or_default()) else {
                return unsupported("id: the builder takes one character");
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
            let mut b = station.telemetry(args.whole::<u16>("sequence", "a number 0-65535")?.unwrap_or(0));
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
            b.build()?
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
        _ => unsupported("symbol: the builder takes a table and a code"),
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
