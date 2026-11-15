//! Runs the language-neutral conformance vectors (the `vectors` submodule, packet-net/aprs-vectors)
//! against this crate. Every case becomes one test per check, named `<check>::<case id>`:
//!
//! - `lenient`: decoding with every tolerance on gives the case's `expect`;
//! - `strict`: decoding with none gives its `strict` result;
//! - `tolerance`: turning off only the tolerance behind a single tolerated defect gives the strict result;
//! - `reencode`: encoding the decoded data again gives what `reencode` says, and writes exactly the
//!   case's `canonical_info` when it has one;
//! - `encode`: an encode case writes the expected information field, or refuses;
//! - `build`: a build case, a builder recipe, is built through the builder API (the same code as the dump
//!   tool's build mode) and gives the expected TNC2 line, or refuses;
//! - `readback`: the neutral form of decoded data reads back into this crate's types unchanged.
//!
//! Cases listed in `tests/known-differences.txt` run as ignored tests, each with its reason.

mod build;
mod compare;
mod neutral;

use compare::differences;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use libtest_mimic::{Arguments, Failed, Trial};
use pdn_aprs::{Address, Code, Data, HeaderError, Packet, ParseOptions, PathEntry, Severity, Unrecognized};
use serde_json::Value;

/// How many cases the tolerance check runs on.
static CHECKED: AtomicUsize = AtomicUsize::new(0);
/// How many cases have lenient diagnostics naming exactly one tolerable code, and a strict result.
static ELIGIBLE: AtomicUsize = AtomicUsize::new(0);

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
    let full_run = args.filter.is_none() && args.skip.is_empty() && !args.list && !args.ignored && !args.exact;
    let conclusion = libtest_mimic::run(&args, trials);
    if full_run {
        let (checked, eligible) = (CHECKED.load(Ordering::Relaxed), ELIGIBLE.load(Ordering::Relaxed));
        eprintln!("tolerance check: ran on {checked} of {eligible} cases whose lenient diagnostics name exactly one tolerable code");
        assert_eq!(checked, eligible, "the tolerance check must run on every case that qualifies");
    }
    conclusion.exit();
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
    if case["input"].get("build").is_some() {
        add("build", build_case);
        return;
    }
    add("lenient", lenient);
    add("strict", strict);
    if case["strict"].is_object() && tolerance_for(&case).is_some() {
        ELIGIBLE.fetch_add(1, Ordering::Relaxed);
        add("tolerance", tolerance);
    }
    if case.get("reencode").is_some() {
        add("reencode", reencode);
    }
    add("readback", readback);
    if case.get("pass_through_inner").is_some() {
        add("pass_through_inner", pass_through_inner);
    }
    if case.get("pass_through").is_some() {
        add("pass_through", pass_through);
    }
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
    CHECKED.fetch_add(1, Ordering::Relaxed);
    let code = tolerance_for(case).unwrap();
    let decoded = decode(&case["input"], ParseOptions::LENIENT.without(code));
    let other = case["strict"].get("rejected_by").and_then(Value::as_str).filter(|x| Code::from_id(x) != Some(code));
    let differences = match other {
        // Strict is rejected by a different code: turning off this one tolerance must give a
        // reading whose diagnostics include that code as a warning (README, "Checking a case").
        Some(x) => match &decoded {
            Ok(p)
                if p.data != Data::Unrecognized(Unrecognized::Malformed)
                    && neutral::diagnostics(&p.diagnostics).contains(&format!("warning:{x}")) =>
            {
                vec![]
            }
            _ => vec![format!("expected a reading with warning:{x}, got {}", describe(&decoded))],
        },
        None => check_strict(case, &decoded),
    };
    result(differences.into_iter().map(|d| format!("with only {code} not tolerated: {d}")).collect())
}

fn show(bytes: &[u8]) -> String {
    Value::String(bytes.iter().map(|&b| b as char).collect()).to_string()
}

fn reencode(case: &Value) -> Result<(), String> {
    let packet =
        decode(&case["input"], ParseOptions::LENIENT).map_err(|e| format!("header error {:?}", neutral::diagnostics(&e.diagnostics)))?;
    let expected = case["reencode"].as_str().unwrap();
    let written = match &packet.data {
        Data::MicE(m) => Packet::create(packet.source.clone(), packet.destination.clone(), packet.path.clone(), Data::MicE(m.clone()))
            .map(|p| (p.information, p.destination)),
        data => data.encode().map(|info| (info, packet.destination.clone())),
    };
    let (info, destination) = match written {
        Err(e) => {
            return if expected == "refused" {
                match case.get("reencode_refusal").and_then(Value::as_str) {
                    Some(code) if code != e.code().id() => Err(format!("reencode: expected the refusal {code}, got {}: {e}", e.code())),
                    _ => Ok(()),
                }
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
        "equivalent" | "rounded" => {
            // A rounded value reads back as the nearest step, so only the bytes are compared.
            if expected == "equivalent" {
                let again = Packet::decode(packet.source.clone(), destination, packet.path.clone(), &info, ParseOptions::LENIENT);
                differences(&neutral::data(&packet.data), &neutral::data(&again.data), "reencode", &mut out);
                if again.has_errors() || again.has_warnings() {
                    out.push(format!("reencode: {} decodes with {:?}", show(&info), neutral::diagnostics(&again.diagnostics)));
                }
            }
            // The bytes the Encoding rule gives are binding: an encoder writes exactly those.
            match case.get("canonical_info").and_then(Value::as_str) {
                Some(canonical) if info != canonical.as_bytes() => {
                    out.push(format!("reencode: expected canonical_info {}, wrote {}", Value::String(canonical.into()), show(&info)))
                }
                None if expected == "rounded" => out.push("reencode: a rounded case needs its canonical_info".into()),
                _ => {}
            }
        }
        other => out.push(format!("reencode: unknown expectation '{other}'")),
    }
    result(out)
}

fn encode(case: &Value) -> Result<(), String> {
    let input = &case["input"];
    let expect = &case["expect"];
    let written = match neutral::try_read_data(&input["encode"]) {
        Err(neutral::Unreadable::Unsupported(why)) => return Err(format!("encode: this crate's data cannot hold it: {why}")),
        Err(neutral::Unreadable::Refused(code, why)) => Err((code, why)),
        // Inputs the reader replaced with stand-ins have their own refusals, settled against the encoder's.
        Ok(data) => neutral::settle(
            Packet::create(source(input), destination(input), Vec::new(), data).map_err(|e| (e.code(), e.message().to_string())),
        )
        .map_err(|(code, why, _)| (code, why)),
    };
    let packet = match written {
        Err((code, e)) => {
            return if expect.get("refused").and_then(Value::as_bool) == Some(true) {
                match expect.get("refusal").and_then(Value::as_str) {
                    Some(want) if want != code.id() => Err(format!("encode: expected the refusal {want}, got {code}: {e}")),
                    _ => Ok(()),
                }
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

fn build_case(case: &Value) -> Result<(), String> {
    let expect = &case["expect"];
    let frame = case["input"].get("write").and_then(Value::as_str) == Some("ax25");
    match build::outcome_writing(&case["input"]["build"], frame) {
        build::Outcome::Unsupported(why) => Err(format!("build: this crate's builder cannot say it: {why}")),
        build::Outcome::Refused(code, why, _) => {
            if expect.get("refused").and_then(Value::as_bool) != Some(true) {
                return Err(format!("build: expected {}, but the builder refused: {code}: {why}", expect["tnc2"]));
            }
            match expect.get("refusal").and_then(Value::as_str) {
                Some(want) if want != code => Err(format!("build: expected the refusal {want}, got {code}: {why}")),
                _ => Ok(()),
            }
        }
        build::Outcome::Built(written) => {
            if expect.get("refused").and_then(Value::as_bool) == Some(true) {
                return Err(format!("build: expected a refusal, built {}", show(&written)));
            }
            if frame {
                let hex: String = written.iter().map(|b| format!("{b:02X}")).collect();
                if !hex.eq_ignore_ascii_case(expect["ax25_hex"].as_str().unwrap()) {
                    return Err(format!("build: expected the frame {}, built {hex}", expect["ax25_hex"]));
                }
            } else if String::from_utf8_lossy(&written) != expect["tnc2"].as_str().unwrap() {
                return Err(format!("build: expected {}, built {}", expect["tnc2"], show(&written)));
            }
            Ok(())
        }
    }
}

/// The lenient packet written back as the program holds it: its TNC2 line and AX.25 frame, or the
/// refusal each gives.
fn pass_through(case: &Value) -> Result<(), String> {
    let packet =
        decode(&case["input"], ParseOptions::LENIENT).map_err(|e| format!("header error {:?}", neutral::diagnostics(&e.diagnostics)))?;
    written_back(&packet, &case["pass_through"], "pass_through")
}

/// The same for a third-party packet's inner packet, taken out of the outer one and written on its own.
fn pass_through_inner(case: &Value) -> Result<(), String> {
    let packet =
        decode(&case["input"], ParseOptions::LENIENT).map_err(|e| format!("header error {:?}", neutral::diagnostics(&e.diagnostics)))?;
    let Data::ThirdParty(inner) = &packet.data else {
        return Err("pass_through_inner: the packet is not a third-party packet".into());
    };
    written_back(inner, &case["pass_through_inner"], "pass_through_inner")
}

fn written_back(packet: &Packet, want: &Value, key: &str) -> Result<(), String> {
    let mut out = Vec::new();
    let mut one = |what: &str,
                   written: Result<Vec<u8>, pdn_aprs::EncodeError>,
                   bytes: Option<&str>,
                   refusal: Option<&str>,
                   hex_form: bool| match (written, bytes, refusal) {
        (Ok(w), Some(expected), _) => {
            let got =
                if hex_form { w.iter().map(|b| format!("{b:02X}")).collect::<String>() } else { String::from_utf8_lossy(&w).into_owned() };
            let same = if hex_form { got.eq_ignore_ascii_case(expected) } else { got == expected };
            if !same {
                out.push(format!("{key} {what}: expected {expected}, wrote {got}"));
            }
        }
        (Ok(w), None, Some(code)) => out.push(format!("{key} {what}: expected the refusal {code}, wrote {}", show(&w))),
        (Err(e), _, Some(code)) => {
            if e.code().id() != code {
                out.push(format!("{key} {what}: expected the refusal {code}, got {}: {e}", e.code()));
            }
        }
        (Err(e), Some(expected), _) => out.push(format!("{key} {what}: expected {expected}, but the writer refused: {}: {e}", e.code())),
        _ => {}
    };
    let text = |k: &str| want.get(k).and_then(Value::as_str);
    if text("tnc2").is_some() || text("tnc2_refusal").is_some() {
        one("tnc2", packet.to_tnc2(), text("tnc2"), text("tnc2_refusal"), false);
    }
    if text("ax25_hex").is_some() || text("ax25_refusal").is_some() {
        one("ax25", packet.to_ax25(), text("ax25_hex"), text("ax25_refusal"), true);
    }
    result(out)
}

fn readback(case: &Value) -> Result<(), String> {
    let Ok(packet) = decode(&case["input"], ParseOptions::LENIENT) else {
        return Ok(());
    };
    // Undecoded data has no neutral form to read back, at any depth of third-party packets.
    let mut innermost = &packet.data;
    while let Data::ThirdParty(p) = innermost {
        innermost = &p.data;
    }
    if matches!(innermost, Data::Unrecognized(_)) {
        return Ok(());
    }
    let written = neutral::data(&packet.data);
    let read = match neutral::try_read_data(&written) {
        Ok(data) => neutral::data(&data),
        // A third-party packet keeps its inner information field, which the neutral form does not
        // carry, so one whose inner data the encoder refuses cannot be made from it.
        Err(neutral::Unreadable::Refused(..)) => return Ok(()),
        Err(e) => return Err(format!("readback: {e}")),
    };
    let mut out = Vec::new();
    differences(&written, &read, "readback", &mut out);
    result(out)
}

#[allow(dead_code)]
fn severity_name(s: Severity) -> &'static str {
    s.name()
}
