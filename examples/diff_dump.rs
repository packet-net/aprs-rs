//! Differential comparison with other implementations of the conformance vectors.
//!
//! Reads a capture extracted as one hex-encoded TNC2 line per line (gzip), as packet.net's
//! `aprs-corpus diff lines` writes it, and writes one JSON object per line in the vectors' neutral
//! form: the lenient and strict results and what re-encoding gives. packet-net/aprs-vectors
//! `tools/compare.py` compares two such files.
//!
//! ```sh
//! cargo run --release --example diff_dump -- lines.hex.gz rust.jsonl.gz
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
use packet_aprs::{Data, Packet, ParseOptions};
use serde_json::{Map, Value, json};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("diff_dump <lines.hex.gz> <out.jsonl.gz>");
        std::process::exit(2);
    }
    let input = BufReader::new(MultiGzDecoder::new(File::open(&args[1])?));
    let mut out = BufWriter::new(GzEncoder::new(File::create(&args[2])?, Compression::fast()));
    let mut n: u64 = 0;
    for line in input.lines() {
        let bytes = hex(&line?);
        let (lenient, packet) = result(&bytes, ParseOptions::LENIENT);
        let (strict, _) = result(&bytes, ParseOptions::STRICT);
        let record = json!({"n": n, "lenient": lenient, "strict": strict, "reencode": reencode(packet.as_ref())});
        serde_json::to_writer(&mut out, &record)?;
        out.write_all(b"\n")?;
        n += 1;
    }
    out.into_inner().map_err(|e| e.into_error())?.finish()?;
    println!("{}: {n} packets", args[2]);
    Ok(())
}

fn hex(text: &str) -> Vec<u8> {
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap_or(0)).collect()
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

/// What encoding the lenient data again gives, as the vectors' reencode check sees it.
fn reencode(packet: Option<&Packet>) -> &'static str {
    let Some(packet) = packet else { return "none" };
    if matches!(packet.data, Data::Unrecognized(_)) {
        return "none";
    }
    let written = match &packet.data {
        Data::MicE(m) => {
            Packet::create_mic_e(packet.source.clone(), m.clone(), packet.path.clone()).map(|p| (p.information, p.destination))
        }
        data => data.encode().map(|info| (info, packet.destination.clone())),
    };
    let Ok((info, destination)) = written else { return "refused" };
    let mut original: &[u8] = &packet.information;
    while let [rest @ .., b'\r' | b'\n'] = original {
        original = rest;
    }
    if info == original && destination == packet.destination {
        return "identical";
    }
    let again = Packet::decode(packet.source.clone(), destination, packet.path.clone(), &info, ParseOptions::LENIENT);
    let mut differences = Vec::new();
    compare::differences(&neutral::data(&packet.data), &neutral::data(&again.data), "", &mut differences);
    if differences.is_empty() && !again.has_warnings() && !again.has_errors() { "equivalent" } else { "fails" }
}
