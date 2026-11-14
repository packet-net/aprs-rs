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

#[path = "../tests/vectors/build.rs"]
mod build;
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
use pdn_aprs::{Address, Data, Packet, ParseOptions, RefusalCode};
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
        Err(neutral::Unreadable::Refused(code, why)) => {
            return json!({"n": n, "result": "refused", "reason": why, "refusal": code.id(), "via": "tool"});
        }
    };
    let written = match data {
        Data::MicE(m) => {
            Packet::create(address("N0CALL"), address("APZ001"), Vec::new(), Data::MicE(m)).map(|p| (p.information, p.destination))
        }
        data => data.encode().map(|info| (info, address("APZ001"))),
    };
    // Inputs the reader replaced with stand-ins have their own refusals, settled against the encoder's.
    let written = neutral::settle(written.map_err(|e| (e.code(), e.message().to_string())));
    match written {
        Err((code, why, by_tool)) => {
            json!({"n": n, "result": "refused", "reason": why, "refusal": code.id(), "via": if by_tool { "tool" } else { "library" }})
        }
        Ok((info, destination)) => {
            // The bytes decoded leniently under N0CALL> and the destination.
            let p = Packet::decode(address("N0CALL"), destination.clone(), Vec::new(), &info, ParseOptions::LENIENT);
            let again = json!({"data": neutral::data(&p.data), "diagnostics": neutral::diagnostics(&p.diagnostics)});
            json!({"n": n, "result": "written", "info": hex(&info), "destination": destination.as_str(), "again": again})
        }
    }
}
// ------------------------------------------------------------------ build

fn build_record(input: &Value) -> Value {
    let n = input.get("n").cloned().unwrap_or(Value::Null);
    match build::outcome(input) {
        build::Outcome::Built(line) => {
            let (decoded, _) = result(&line, ParseOptions::LENIENT);
            json!({"n": n, "result": "built", "tnc2": hex(&line), "again": decoded})
        }
        build::Outcome::Unsupported(why) => json!({"n": n, "result": "unsupported", "reason": why}),
        build::Outcome::Refused(code, why, by_tool) => {
            json!({"n": n, "result": "refused", "reason": why, "refusal": code, "via": if by_tool { "tool" } else { "library" }})
        }
    }
}
