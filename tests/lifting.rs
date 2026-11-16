//! Lifting an element never joins the text either side of it (vectors interpretations.md): an
//! element is read only where its bytes stand whole as sent. Variations the shared cases do not hold.

use pdn_aprs::{Data, Packet, ParseOptions, RefusalCode, Tone};

fn decode(line: &[u8]) -> Data {
    Packet::decode_tnc2(line, ParseOptions::LENIENT).unwrap().data
}

fn positioned(data: &Data) -> &pdn_aprs::Positioned {
    match data {
        Data::Position(p) => &p.fields,
        Data::MicE(m) => &m.fields,
        _ => panic!("not a positioned report"),
    }
}

/// Written back, text that reads as an element is refused: it has no clean form.
fn refused_as_text_reads_as_field(line: &[u8]) {
    let packet = Packet::decode_tnc2(line, ParseOptions::LENIENT).unwrap();
    let err = Packet::create(packet.source, packet.destination, Vec::new(), packet.data).unwrap_err();
    assert_eq!(err.code(), RefusalCode::TextReadsAsField, "{}", String::from_utf8_lossy(line));
}

const POS: &str = "N0CALL>APZ001:!5130.00N/00005.00W-";

#[test]
fn a_phg_split_by_a_dao_is_text() {
    let line = format!("{POS}PH!W12!G3330 x");
    let data = decode(line.as_bytes());
    let f = positioned(&data);
    assert!(f.phg.is_none() && f.dao.is_some());
    assert_eq!(f.comment, "PHG3330 x");
    refused_as_text_reads_as_field(line.as_bytes());
}

#[test]
fn an_rng_split_by_telemetry_is_text() {
    let line = format!("{POS}RNG0|!:&0'p|045 hi");
    let data = decode(line.as_bytes());
    let f = positioned(&data);
    assert!(f.range_miles.is_none() && f.telemetry.is_some());
    assert_eq!(f.comment, "RNG0045 hi");
    refused_as_text_reads_as_field(line.as_bytes());
}

#[test]
fn a_dfs_split_by_an_altitude_is_text_and_a_whole_one_after_it_is_read() {
    let line = format!("{POS}DF/A=000123S2360");
    let data = decode(line.as_bytes());
    let f = positioned(&data);
    assert!(f.dfs.is_none());
    assert_eq!(f.altitude_feet, Some(123.0));
    assert_eq!(f.comment, "DFS2360");
    refused_as_text_reads_as_field(line.as_bytes());
    // The same elements the other way round: the altitude stands apart from a whole DFS.
    let data = decode(format!("{POS}DFS2360/A=000123").as_bytes());
    let f = positioned(&data);
    assert!(f.dfs.is_some());
    assert_eq!(f.altitude_feet, Some(123.0));
}

#[test]
fn a_signpost_split_by_an_altitude_is_text() {
    let line = "N0CALL>APZ001:!5130.00N\\00005.00Wm{5/A=000123x5}";
    let data = decode(line.as_bytes());
    let f = positioned(&data);
    assert!(f.signpost.is_none());
    assert_eq!(f.altitude_feet, Some(123.0));
    assert_eq!(f.comment, "{5x5}");
}

#[test]
fn a_weather_software_type_split_by_telemetry_is_text() {
    let line = "N0CALL>APZ001:!5130.00N/00005.00W_220/004g005t077X|!:&0'p|yz";
    let data = decode(line.as_bytes());
    let f = positioned(&data);
    let weather = f.weather.as_ref().unwrap();
    assert!(weather.software.is_none() && weather.unit.is_none());
    assert!(f.telemetry.is_some());
    assert_eq!(f.comment, "Xyz");
    // Whole, with the telemetry after it, it is read.
    let data = decode(b"N0CALL>APZ001:!5130.00N/00005.00W_220/004g005t077Xyz|!:&0'p|");
    let weather = positioned(&data).weather.as_ref().unwrap();
    assert_eq!(weather.software, Some('X'));
}

#[test]
fn a_tone_after_a_lifted_altitude_is_read_and_after_telemetry_is_not() {
    let data = decode(format!("{POS}146.520MHz/A=000100 T100 x").as_bytes());
    let f = positioned(&data);
    assert_eq!(f.frequency.as_ref().unwrap().tone, Some(Tone::Tone));
    assert_eq!(f.altitude_feet, Some(100.0));
    assert_eq!(f.comment, "x");
    let data = decode(format!("{POS}146.520MHz|!:&0'p| T100 x").as_bytes());
    let f = positioned(&data);
    assert!(f.frequency.as_ref().unwrap().tone.is_none());
    assert!(f.telemetry.is_some());
    assert_eq!(f.comment, "T100 x");
}

#[test]
fn an_altitude_inside_a_frequency_splits_it_in_both_orders() {
    let data = decode(format!("{POS}146.5/A=00010020MHz").as_bytes());
    let f = positioned(&data);
    assert!(f.frequency.is_none());
    assert_eq!(f.comment, "146.520MHz");
    let data = decode(format!("{POS}146.520MHz T1/A=00010000 x").as_bytes());
    let f = positioned(&data);
    assert!(f.frequency.as_ref().unwrap().tone.is_none());
    assert_eq!(f.comment, "T100 x");
}

#[test]
fn mic_e_an_rng_split_by_a_dao_after_the_type_code_is_text() {
    let line = b"N0CALL>S32UVT:`(_fn\"Oj/`RN!W12!G0045 x";
    let data = decode(line);
    let f = positioned(&data);
    assert!(f.range_miles.is_none() && f.dao.is_some());
    assert_eq!(f.comment, "RNG0045 x");
    refused_as_text_reads_as_field(line);
}

#[test]
fn mic_e_an_altitude_in_the_text_splits_a_slash_a_altitude() {
    let line = b"N0CALL>S32UVT:`(_fn\"Oj/`x/A=00\"4e}0123";
    let data = decode(line);
    let f = positioned(&data);
    assert_eq!(f.comment, "x/A=000123");
    assert!(f.altitude_feet.is_some());
    let Data::MicE(m) = &data else { panic!("not Mic-E") };
    assert!(!m.altitude_in_comment);
    refused_as_text_reads_as_field(line);
}

#[test]
fn a_strict_mic_e_decoder_reads_a_later_altitude_as_text_and_never_rejects_by_it() {
    for line in [&b"N0CALL>S32UVT:`(_fn\"Oj/`x\"4e}hello}"[..], &b"N0CALL>S32UVT:`(_fn\"Oj/`IO91/G !W1\"4e}2!x"[..]] {
        let packet = Packet::decode_tnc2(line, ParseOptions::STRICT).unwrap();
        assert!(packet.diagnostics.iter().all(|d| d.code != pdn_aprs::Code::MicEAltitudeNotFirst), "{}", String::from_utf8_lossy(line));
    }
}

const MIC_E: &str = "N0CALL>S32UVT:`(_fn\"Oj/`";

fn mic_e(status: &str, options: ParseOptions) -> pdn_aprs::MicEReport {
    match Packet::decode_tnc2(format!("{MIC_E}{status}").as_bytes(), options).unwrap().data {
        Data::MicE(m) => m,
        _ => panic!("not a Mic-E report"),
    }
}

/// The search for a later Mic-E altitude never uses the bytes of a whole `/A=`.
#[test]
fn a_later_mic_e_altitude_never_uses_the_bytes_of_a_whole_altitude() {
    for options in [ParseOptions::LENIENT, ParseOptions::STRICT] {
        let m = mic_e("x/A=000100}", options);
        assert_eq!((m.fields.altitude_feet, m.fields.comment.as_str()), (Some(100.0), "x}"));
        // A negative one, and one with more text either side.
        let m = mic_e("x/A=-00016}y", options);
        assert_eq!((m.fields.altitude_feet, m.fields.comment.as_str()), (Some(-16.0), "x}y"));
        let m = mic_e("/A=000100}", options);
        assert_eq!((m.fields.altitude_feet, m.fields.comment.as_str()), (Some(100.0), "}"));
    }
}

#[test]
fn a_later_mic_e_altitude_clear_of_a_whole_altitude_is_still_taken() {
    let m = mic_e("x/A=000100}y\"4e}z", ParseOptions::LENIENT);
    assert_eq!((m.fields.altitude_feet, m.fields.comment.as_str()), (Some(100.0), "x}yz"));
    // Before the `/A=`, too.
    let m = mic_e("x\"4e}y/A=000100", ParseOptions::LENIENT);
    assert_eq!((m.fields.altitude_feet, m.fields.comment.as_str()), (Some(100.0), "xy"));
    // An altitude that is not whole protects nothing: its last three digits and `}` are one.
    let m = mic_e("x/A=00100}", ParseOptions::LENIENT);
    assert_eq!(m.fields.comment, "x/A=00");
    assert!(m.fields.altitude_feet.is_some_and(|f| f > 400_000.0));
}

/// The comment written after a `/A=` needs no space now: `00x}` is not a Mic-E altitude.
#[test]
fn a_comment_after_an_altitude_is_written_straight_on() {
    let packet = Packet::decode_tnc2(format!("{MIC_E}x/A=000100}}").as_bytes(), ParseOptions::LENIENT).unwrap();
    let out = Packet::create(packet.source, packet.destination, Vec::new(), packet.data).unwrap();
    let line = out.to_tnc2().unwrap();
    assert!(line.ends_with(b"/A=000100x}"), "{}", String::from_utf8_lossy(&line));
}
