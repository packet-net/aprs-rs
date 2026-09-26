//! Runs the language-neutral conformance vectors (the `vectors` submodule, packet-net/aprs-vectors)
//! against this crate. Every case becomes one test per check, named `<check>::<case id>`:
//!
//! - `lenient`: decoding with every tolerance on gives the case's `expect`;
//! - `strict`: decoding with none gives its `strict` result;
//! - `tolerance`: turning off only the tolerance behind a single tolerated defect gives the strict result;
//! - `reencode`: encoding the decoded data again gives what `reencode` says;
//! - `encode`: an encode case writes the expected information field, or refuses;
//! - `readback`: the neutral form of decoded data reads back into this crate's types unchanged.
//!
//! Cases listed in `tests/known-differences.txt` run as ignored tests, each with its reason.

mod compare;
mod neutral;

use compare::differences;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use libtest_mimic::{Arguments, Failed, Trial};
use pdn_aprs::{Address, Code, Data, HeaderError, Packet, ParseOptions, PathEntry, Severity, Unrecognized};
use serde_json::Value;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("vectors")
}

fn main() {
    let args = Arguments::from_args();
    let known = known_differences();
    let mut trials = Vec::new();
    let cases_dir = root().join("cases");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&cases_dir)
        .unwrap_or_else(|_| panic!("{} is missing: run 'git submodule update --init vectors'", cases_dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    for file in files {
        let document: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        for case in document["cases"].as_array().unwrap() {
            add_trials(&mut trials, case.clone(), &known);
        }
    }
    libtest_mimic::run(&args, trials).exit();
}

fn known_differences() -> BTreeMap<String, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("known-differences.txt");
    let Ok(text) = std::fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| match l.split_once(char::is_whitespace) {
            Some((name, reason)) => (name.to_string(), reason.trim().to_string()),
            None => (l.to_string(), String::new()),
        })
        .collect()
}

fn add_trials(trials: &mut Vec<Trial>, case: Value, known: &BTreeMap<String, String>) {
    let id = case["id"].as_str().unwrap().to_string();
    let mut add = |check: &str, run: fn(&Value) -> Result<(), String>| {
        let name = format!("{check}::{id}");
        let ignored = known.contains_key(&name);
        let c = case.clone();
        trials.push(Trial::test(name, move || run(&c).map_err(Failed::from)).with_ignored_flag(ignored));
    };
    if case["input"].get("encode").is_some() {
        add("encode", encode);
        return;
    }
    add("lenient", lenient);
    add("strict", strict);
    if case["strict"].is_object() && tolerance_for(&case).is_some() {
        add("tolerance", tolerance);
    }
    if case.get("reencode").is_some() {
        add("reencode", reencode);
    }
    add("readback", readback);
}

// ------------------------------------------------------------------ decoding

type Decoded = Result<Packet, HeaderError>;

fn hex(text: &str) -> Vec<u8> {
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap()).collect()
}

fn source(input: &Value) -> Address {
    Address::new(input.get("source").and_then(Value::as_str).unwrap_or("N0CALL")).unwrap()
}

fn destination(input: &Value) -> Address {
    Address::new(input.get("destination").and_then(Value::as_str).unwrap_or("APZ001")).unwrap()
}

fn decode(input: &Value, options: ParseOptions) -> Decoded {
    if let Some(line) = input.get("tnc2").and_then(Value::as_str) {
        return Packet::decode_tnc2(line.as_bytes(), options);
    }
    if let Some(line) = input.get("tnc2_hex").and_then(Value::as_str) {
        return Packet::decode_tnc2(&hex(line), options);
    }
    if let Some(frame) = input.get("ax25_hex").and_then(Value::as_str) {
        return Packet::decode_ax25(&hex(frame), options);
    }
    let info = match input.get("info").and_then(Value::as_str) {
        Some(text) => text.as_bytes().to_vec(),
        None => hex(input["info_hex"].as_str().unwrap()),
    };
    let path: Vec<PathEntry> = input
        .get("path")
        .map(|p| p.as_array().unwrap().iter().map(|e| neutral::read_path_entry(e.as_str().unwrap())).collect())
        .unwrap_or_default();
    Ok(Packet::decode(source(input), destination(input), path, &info, options))
}

// ------------------------------------------------------------------ comparing

fn diagnostic_differences(expected: Option<&Value>, actual: &[String], out: &mut Vec<String>) {
    let mut remaining: Vec<String> = actual.to_vec();
    let mut missing = Vec::new();
    for e in expected.and_then(Value::as_array).into_iter().flatten() {
        let e = e.as_str().unwrap();
        match remaining.iter().position(|a| a == e) {
            Some(i) => {
                remaining.remove(i);
            }
            None => missing.push(e.to_string()),
        }
    }
    if !missing.is_empty() {
        out.push(format!("diagnostics: expected {missing:?}, not reported"));
    }
    if !remaining.is_empty() {
        out.push(format!("diagnostics: unexpected {remaining:?}"));
    }
}

fn describe(decoded: &Decoded) -> String {
    match decoded {
        Ok(p) => format!("{} with {:?}", neutral::data(&p.data), neutral::diagnostics(&p.diagnostics)),
        Err(e) => format!("header error {:?}", neutral::diagnostics(&e.diagnostics)),
    }
}

fn check(expect: &Value, decoded: &Decoded) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(header_error) = expect.get("header_error") {
        match decoded {
            Ok(p) => out.push(format!("expected a header error, decoded {}", neutral::data(&p.data))),
            Err(e) => diagnostic_differences(Some(header_error), &neutral::diagnostics(&e.diagnostics), &mut out),
        }
        return out;
    }
    let p = match decoded {
        Ok(p) => p,
        Err(e) => return vec![format!("header error: {:?}", neutral::diagnostics(&e.diagnostics))],
    };
    differences(&expect["data"], &neutral::data(&p.data), "data", &mut out);
    diagnostic_differences(expect.get("diagnostics"), &neutral::diagnostics(&p.diagnostics), &mut out);
    if let Some(header) = expect.get("header") {
        differences(header, &neutral::header(p), "header", &mut out);
    }
    if let Some(device) = expect.get("device") {
        let found = p.device().map_or(serde_json::json!({}), |d| {
            let mut o = serde_json::Map::new();
            if let Some(v) = d.vendor {
                o.insert("vendor".into(), v.into());
            }
            if let Some(m) = d.model {
                o.insert("model".into(), m.into());
            }
            Value::Object(o)
        });
        differences(device, &found, "device", &mut out);
    }
    out
}

fn result(differences: Vec<String>) -> Result<(), String> {
    if differences.is_empty() { Ok(()) } else { Err(differences.join("\n")) }
}

fn check_strict(case: &Value, decoded: &Decoded) -> Vec<String> {
    match case.get("strict") {
        None => check(&case["expect"], decoded),
        Some(Value::String(s)) if s == "same" => check(&case["expect"], decoded),
        Some(s) if s.get("rejected_by").is_some() => {
            let error = format!("error:{}", s["rejected_by"].as_str().unwrap());
            if s.get("header").and_then(Value::as_bool) == Some(true) {
                match decoded {
                    Err(e) if neutral::diagnostics(&e.diagnostics).contains(&error) => vec![],
                    _ => vec![format!("strict: expected the header to be rejected with {error}, got {}", describe(decoded))],
                }
            } else {
                match decoded {
                    Ok(p)
                        if p.data == Data::Unrecognized(Unrecognized::Malformed)
                            && neutral::diagnostics(&p.diagnostics).contains(&error) =>
                    {
                        vec![]
                    }
                    _ => vec![format!("strict: expected rejection with {error}, got {}", describe(decoded))],
                }
            }
        }
        Some(s) if s.is_object() => check(s, decoded),
        Some(other) => vec![format!("strict: unknown expectation {other}")],
    }
}

/// The one tolerance a case's lenient decoding used, if exactly one.
fn tolerance_for(case: &Value) -> Option<Code> {
    let mut codes: Vec<Code> = case["expect"]
        .get("diagnostics")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|d| d.as_str()?.strip_prefix("warning:"))
        .filter_map(Code::from_id)
        .filter(|c| c.is_tolerable())
        .collect();
    codes.sort();
    codes.dedup();
    (codes.len() == 1).then(|| codes[0])
}

// ------------------------------------------------------------------ the checks

fn lenient(case: &Value) -> Result<(), String> {
    result(check(&case["expect"], &decode(&case["input"], ParseOptions::LENIENT)))
}

fn strict(case: &Value) -> Result<(), String> {
    result(check_strict(case, &decode(&case["input"], ParseOptions::STRICT)))
}

fn tolerance(case: &Value) -> Result<(), String> {
    let code = tolerance_for(case).unwrap();
    let decoded = decode(&case["input"], ParseOptions::LENIENT.without(code));
    result(check_strict(case, &decoded).into_iter().map(|d| format!("with only {code} not tolerated: {d}")).collect())
}

fn show(bytes: &[u8]) -> String {
    Value::String(bytes.iter().map(|&b| b as char).collect()).to_string()
}

fn reencode(case: &Value) -> Result<(), String> {
    let packet =
        decode(&case["input"], ParseOptions::LENIENT).map_err(|e| format!("header error {:?}", neutral::diagnostics(&e.diagnostics)))?;
    let expected = case["reencode"].as_str().unwrap();
    let written = match &packet.data {
        Data::MicE(m) => {
            Packet::create_mic_e(packet.source.clone(), m.clone(), packet.path.clone()).map(|p| (p.information, p.destination))
        }
        data => data.encode().map(|info| (info, packet.destination.clone())),
    };
    let (info, destination) = match written {
        Err(e) => {
            return if expected == "refused" {
                Ok(())
            } else {
                Err(format!("reencode: expected {expected}, but the encoder refused: {e}"))
            };
        }
        Ok(w) => w,
    };
    let mut original: &[u8] = &packet.information;
    while let [rest @ .., b'\r' | b'\n'] = original {
        original = rest;
    }
    let mut out = Vec::new();
    match expected {
        "refused" => out.push(format!("reencode: expected the encoder to refuse, it wrote {}", show(&info))),
        "identical" => {
            if info != original {
                out.push(format!("reencode: expected the input back, got {}", show(&info)));
            }
            if matches!(packet.data, Data::MicE(_)) && destination != packet.destination {
                out.push(format!("reencode: expected the Mic-E destination {} back, got {destination}", packet.destination));
            }
        }
        "equivalent" => {
            let again = Packet::decode(packet.source.clone(), destination, packet.path.clone(), &info, ParseOptions::LENIENT);
            differences(&neutral::data(&packet.data), &neutral::data(&again.data), "reencode", &mut out);
            if again.has_errors() || again.has_warnings() {
                out.push(format!("reencode: {} decodes with {:?}", show(&info), neutral::diagnostics(&again.diagnostics)));
            }
        }
        other => out.push(format!("reencode: unknown expectation '{other}'")),
    }
    result(out)
}

fn encode(case: &Value) -> Result<(), String> {
    let input = &case["input"];
    let expect = &case["expect"];
    let data = neutral::read_data(&input["encode"]);
    let written = match data {
        Data::MicE(m) => Packet::create_mic_e(source(input), m, Vec::new()),
        data => Packet::create(source(input), destination(input), Vec::new(), data),
    };
    let packet = match written {
        Err(e) => {
            return if expect.get("refused").and_then(Value::as_bool) == Some(true) {
                Ok(())
            } else {
                Err(format!("encode: expected {}, but the encoder refused: {e}", expect["info"]))
            };
        }
        Ok(p) => p,
    };
    let mut out = Vec::new();
    if expect.get("refused").and_then(Value::as_bool) == Some(true) {
        out.push(format!("encode: expected a refusal, wrote {}", show(&packet.information)));
    } else {
        if String::from_utf8_lossy(&packet.information) != expect["info"].as_str().unwrap() {
            out.push(format!("encode: expected {}, wrote {}", expect["info"], show(&packet.information)));
        }
        if let Some(d) = expect.get("destination").and_then(Value::as_str) {
            if d != packet.destination.as_str() {
                out.push(format!("encode: expected destination {d}, computed {}", packet.destination));
            }
        }
    }
    result(out)
}

fn readback(case: &Value) -> Result<(), String> {
    let Ok(packet) = decode(&case["input"], ParseOptions::LENIENT) else {
        return Ok(());
    };
    if matches!(packet.data, Data::Unrecognized(_))
        || matches!(&packet.data, Data::ThirdParty(p) if matches!(p.data, Data::Unrecognized(_)))
    {
        return Ok(());
    }
    let written = neutral::data(&packet.data);
    let read = neutral::data(&neutral::read_data(&written));
    let mut out = Vec::new();
    differences(&written, &read, "readback", &mut out);
    result(out)
}

#[allow(dead_code)]
fn severity_name(s: Severity) -> &'static str {
    s.name()
}
