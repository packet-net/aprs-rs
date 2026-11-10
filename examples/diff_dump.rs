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
    Address, BeamHeading, Data, EncodeError, MicEMessage, Packet, ParseOptions, Phg, Station, Symbol, Timestamp, Tone, VoiceFrequency,
    Weather, WeatherBuilder,
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
            Mode::Encode => encode_record(&serde_json::from_str(&line)?),
            Mode::Build => build_record(&serde_json::from_str(&line)?),
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
    let (how, written) = reencode(packet.as_ref());
    let mut record = json!({"n": n, "lenient": lenient, "strict": strict, "reencode": how});
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
fn reencode(packet: Option<&Packet>) -> (&'static str, Written) {
    let Some(packet) = packet else { return ("none", None) };
    if matches!(packet.data, Data::Unrecognized(_)) {
        return ("none", None);
    }
    let written = match &packet.data {
        Data::MicE(m) => {
            Packet::create_mic_e(packet.source.clone(), m.clone(), packet.path.clone()).map(|p| (p.information, p.destination))
        }
        data => data.encode().map(|info| (info, packet.destination.clone())),
    };
    let Ok((info, destination)) = written else { return ("refused", None) };
    let wrote = Some((info.clone(), matches!(packet.data, Data::MicE(_)).then(|| destination.as_str().to_string())));
    let mut original: &[u8] = &packet.information;
    while let [rest @ .., b'\r' | b'\n'] = original {
        original = rest;
    }
    if info == original && destination == packet.destination {
        return ("identical", wrote);
    }
    let again = Packet::decode(packet.source.clone(), destination, packet.path.clone(), &info, ParseOptions::LENIENT);
    let mut differences = Vec::new();
    compare::differences(&neutral::data(&packet.data), &neutral::data(&again.data), "", &mut differences);
    let how = if differences.is_empty() && !again.has_warnings() && !again.has_errors() { "equivalent" } else { "fails" };
    (how, wrote)
}

/// The API view: what the packet's own public accessors say, as a program would read them. This
/// crate has no symbol descriptions and no PHG arithmetic, so `symbol` and `phg` are left out.
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
    o.insert("ax25".into(), p.to_ax25().map_or(json!("refused"), |frame| json!(hex(&frame))));
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
        Err(neutral::Unreadable::Refused(why)) => return json!({"n": n, "result": "refused", "reason": why}),
    };
    let written = match data {
        Data::MicE(m) => Packet::create_mic_e(address("N0CALL"), m, Vec::new()).map(|p| (p.information, p.destination)),
        data => data.encode().map(|info| (info, address("APZ001"))),
    };
    match written {
        Err(e) => json!({"n": n, "result": "refused", "reason": e.message}),
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
    Refused(String),
}

impl From<EncodeError> for NotBuilt {
    fn from(e: EncodeError) -> NotBuilt {
        NotBuilt::Refused(e.message)
    }
}

type Built<T> = Result<T, NotBuilt>;

fn unsupported<T>(why: impl Into<String>) -> Built<T> {
    Err(NotBuilt::Unsupported(why.into()))
}

fn build_record(input: &Value) -> Value {
    let n = input.get("n").cloned().unwrap_or(Value::Null);
    match build(input) {
        Ok(p) => {
            let line = p.to_tnc2();
            let (decoded, _) = result(&line, ParseOptions::LENIENT);
            json!({"n": n, "result": "built", "tnc2": hex(&line), "again": decoded})
        }
        Err(NotBuilt::Unsupported(why)) => json!({"n": n, "result": "unsupported", "reason": why}),
        Err(NotBuilt::Refused(why)) => json!({"n": n, "result": "refused", "reason": why}),
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
            Some(v) => v.as_f64().map(Some).ok_or_else(|| NotBuilt::Unsupported(format!("{k}: not a number"))),
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
    v.as_i64().or_else(|| v.as_f64().filter(|f| f.fract() == 0.0 && f.abs() < 1e15).map(|f| f as i64))
}

/// The options every positioned builder shares: the same methods on each builder type.
macro_rules! positioned {
    ($b:expr, $args:expr) => {{
        let args: &Args = $args;
        let mut b = $b;
        if let Some(s) = symbol(args.text("symbol")?)? {
            b = b.symbol(s);
        }
        if let Some(c) = args.whole::<u16>("course_degrees", "whole degrees")? {
            b = b.course(c);
        }
        if args.has("speed_knots") && args.has("speed_kmh") {
            return unsupported("speed_kmh: the builder takes one speed");
        }
        if let Some(s) = args.float("speed_knots")? {
            b = b.speed(s);
        }
        if let Some(s) = args.float("speed_kmh")? {
            b = b.speed_kmh(s);
        }
        if args.has("altitude_feet") && args.has("altitude_m") {
            return unsupported("altitude_m: the builder takes one altitude");
        }
        if let Some(a) = args.float("altitude_feet")? {
            b = b.altitude(a);
        }
        if let Some(a) = args.float("altitude_m")? {
            b = b.altitude_metres(a);
        }
        if let Some(c) = args.text("comment")? {
            b = b.comment(c);
        }
        if let Some(p) = args.get("phg") {
            b = b.phg(phg(p)?);
        }
        if let Some(r) = args.float("range_miles")? {
            b = b.range(r);
        }
        if let Some(f) = args.get("frequency") {
            let Frequency { mhz, tone, value, offset } = frequency(f)?;
            b = match tone {
                // The tone the builder's tone() sends, in whole Hz.
                None | Some(Tone::Tone) => {
                    let b = b.frequency(mhz);
                    match value {
                        Some(hz) => b.tone(hz),
                        None => b,
                    }
                }
                Some(tone) => {
                    let value = value
                        .filter(|v| v.fract() == 0.0 && (0.0..65536.0).contains(v))
                        .ok_or_else(|| NotBuilt::Unsupported("frequency.tone_value: the builder takes a whole number".into()))?;
                    b.voice_frequency(VoiceFrequency { mhz, tone: Some(tone), tone_value: Some(value as u16), ..VoiceFrequency::default() })
                }
            };
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
            let (sequence, analog, digital) = comment_telemetry(t)?;
            b = match digital {
                None => b.telemetry(sequence, &analog),
                Some(d) => b.telemetry_with_bits(sequence, &analog, d),
            };
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
                b = b.kill();
            }
            args.all_taken()?;
            b.build()?
        }
        "item" => {
            let mut b = station.item(args.text("name")?.unwrap_or_default());
            b = b.at(args.required_float("latitude")?, args.required_float("longitude")?);
            b = positioned!(b, &args);
            if args.flag("killed")? {
                b = b.kill();
            }
            args.all_taken()?;
            b.build()?
        }
        "mic-e" => {
            let mut b = station.mic_e(args.required_float("latitude")?, args.required_float("longitude")?);
            b = positioned!(b, &args);
            if let Some(m) = args.text("mic_e_message")? {
                b = b.message(mic_e_message(m)?);
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
                b = b.id(id);
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
                let Some(symbol) = symbol(args.text("symbol")?)? else {
                    return unsupported("locator: the builder takes a locator with a symbol");
                };
                b = b.locator(locator, symbol);
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
                // The channels not given are sent empty, so only trailing ones can be left out.
                let given = a.iter().rposition(|v| !v.is_null()).map_or(0, |i| i + 1);
                let values = a[..given]
                    .iter()
                    .map(Value::as_f64)
                    .collect::<Option<Vec<f64>>>()
                    .ok_or_else(|| NotBuilt::Unsupported("analog: the builder sends only the last channels empty".into()))?;
                b = b.analog(&values);
            }
            if let Some(bits) = args.text("bits")? {
                b = b.digital(bit_byte(bits).ok_or_else(|| NotBuilt::Unsupported("bits: the builder takes eight 0 or 1".into()))?);
            }
            if let Some(c) = args.text("comment")? {
                b = b.comment(c);
            }
            args.all_taken()?;
            b.build()?
        }
        "telemetry-names" | "telemetry-units" => {
            own_addressee(&station, &args)?;
            let labels = strings(&args, if report == "telemetry-names" { "names" } else { "units" })?;
            let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
            let b = if report == "telemetry-names" { station.telemetry_names(&labels) } else { station.telemetry_units(&labels) };
            args.all_taken()?;
            b.build()?
        }
        "telemetry-coefficients" => {
            own_addressee(&station, &args)?;
            let coefficients = match args.get("coefficients") {
                None => Vec::new(),
                Some(a) => a
                    .as_array()
                    .and_then(|a| a.iter().map(Value::as_f64).collect::<Option<Vec<f64>>>())
                    .ok_or_else(|| NotBuilt::Unsupported("coefficients: the builder takes numbers".into()))?,
            };
            let b = station.telemetry_coefficients(&coefficients);
            args.all_taken()?;
            b.build()?
        }
        "telemetry-bits" => {
            own_addressee(&station, &args)?;
            let bits = args.text("bits")?.unwrap_or_default();
            let bits = bit_byte(bits).ok_or_else(|| NotBuilt::Unsupported("bits: the builder takes eight 0 or 1".into()))?;
            let b = station.telemetry_bits(bits, args.text("project")?.unwrap_or_default());
            args.all_taken()?;
            b.build()?
        }
        _ => return unsupported("report: the builder has no such report"),
    };
    Ok(packet)
}

fn station(v: &Value) -> Built<Station> {
    let text = |k: &str| v.get(k).and_then(Value::as_str);
    let refused = |e: pdn_aprs::InvalidAddress| NotBuilt::Refused(e.to_string());
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

/// Telemetry metadata goes to the sending station itself; the builder has no other addressee.
fn own_addressee(station: &Station, args: &Args) -> Built<()> {
    match args.text("addressee")? {
        Some(a) if a != station.source().as_str() => unsupported("addressee: the builder addresses metadata to the station itself"),
        _ => Ok(()),
    }
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

/// Eight `0`/`1` characters, B1 first, as the byte the builder takes (bit 0 is B1).
fn bit_byte(bits: &str) -> Option<u8> {
    if bits.len() != 8 {
        return None;
    }
    bits.bytes().enumerate().try_fold(0u8, |acc, (i, b)| match b {
        b'0' => Some(acc),
        b'1' => Some(acc | 1 << i),
        _ => None,
    })
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
    let mhz = f.get("mhz").and_then(Value::as_f64).ok_or_else(|| NotBuilt::Unsupported("frequency.mhz: missing".into()))?;
    let tone = match f.get("tone").and_then(Value::as_str) {
        None => None,
        Some("tone") => Some(Tone::Tone),
        Some("ctcss") => Some(Tone::Ctcss),
        Some("dcs") => Some(Tone::Dcs),
        Some(_) => return unsupported("frequency.tone: the builder has no such tone"),
    };
    let value = f.get("tone_value").and_then(Value::as_f64);
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
fn comment_telemetry(t: &Value) -> Built<(u16, Vec<u16>, Option<u8>)> {
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
    Ok((sequence, analog, digital))
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
    match (args.whole::<u16>("wind_direction_degrees", "whole degrees")?, args.float("wind_speed_mph")?) {
        (Some(d), Some(s)) => b = b.wind(d, s),
        (None, None) => {}
        // wind() takes both; one alone goes in with observations(), before everything else.
        (d, s) => b = b.observations(Weather { wind_direction_degrees: d, wind_speed_mph: s, ..Weather::default() }),
    }
    if let Some(g) = args.float("wind_gust_mph")? {
        b = b.gust(g);
    }
    if args.has("temperature_f") && args.has("temperature_c") {
        return unsupported("temperature_c: the builder takes one temperature");
    }
    if let Some(t) = args.float("temperature_f")? {
        b = b.temperature(t);
    }
    if let Some(t) = args.float("temperature_c")? {
        b = b.temperature_celsius(t);
    }
    type Rain = fn(WeatherBuilder, f64) -> WeatherBuilder;
    let rain: [(&str, &str, Rain, Rain); 3] = [
        ("rain_1h_in", "rain_1h_mm", WeatherBuilder::rain_last_hour, WeatherBuilder::rain_last_hour_mm),
        ("rain_24h_in", "rain_24h_mm", WeatherBuilder::rain_last_24_hours, WeatherBuilder::rain_last_24_hours_mm),
        ("rain_midnight_in", "rain_midnight_mm", WeatherBuilder::rain_since_midnight, WeatherBuilder::rain_since_midnight_mm),
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
    if let Some(h) = args.whole::<u8>("humidity_percent", "whole percent")? {
        b = b.humidity(h);
    }
    if let Some(p) = args.float("pressure_mbar")? {
        b = b.pressure(p);
    }
    if let Some(l) = args.whole::<u16>("luminosity_w_m2", "whole W/m2")? {
        b = b.luminosity(l);
    }
    if let Some(s) = args.float("snow_24h_in")? {
        b = b.snowfall(s);
    }
    args.all_taken()?;
    Ok(b.build()?)
}
