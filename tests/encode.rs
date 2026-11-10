//! Encoding data a program made, rather than data the decoder read.

use pdn_aprs::{Data, Packet, ParseOptions};

/// An NMEA sentence's fields, when given, must say what the sentence says; degrees reached by
/// other arithmetic differ in the last bits, so they agree within a part in 10^9, as the vectors
/// compare numbers.
#[test]
fn nmea_fields_agree_within_rounding() {
    let line = b"N0CALL>APZ001:$GPGLL,2554.459,N,08020.187,W,154027.50,A";
    let packet = Packet::decode_tnc2(line, ParseOptions::LENIENT).unwrap();
    let Data::Nmea(mut nmea) = packet.data.clone() else { panic!("an NMEA sentence") };
    // 25.907649999999997, as another implementation computes 25 + 54.459 / 60.
    nmea.latitude = nmea.latitude.map(|l| l * (1.0 + 1e-15));
    let written = Packet::create(packet.source.clone(), packet.destination.clone(), Vec::new(), Data::Nmea(nmea.clone())).unwrap();
    assert_eq!(written.information, b"$GPGLL,2554.459,N,08020.187,W,154027.50,A");
    nmea.latitude = nmea.latitude.map(|l| l + 0.001);
    assert!(Packet::create(packet.source, packet.destination, Vec::new(), Data::Nmea(nmea)).is_err());
}

/// A Mic-E comment that would join the `/A=` digits before it into a Mic-E altitude (`1{5}`) is
/// written after a space, the one clean form; `/` would join them too.
#[test]
fn a_mic_e_comment_after_a_space_where_only_that_reads_back() {
    let report = pdn_aprs::MicEReport {
        type_code: Some('`'),
        fields: pdn_aprs::Positioned {
            position: pdn_aprs::Position { latitude: 33.46, longitude: -2.69, ambiguity: 0 },
            symbol: pdn_aprs::Symbol::CAR,
            altitude_feet: Some(100.5),
            comment: "{5}".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    let source = pdn_aprs::Address::new("N0CALL").unwrap();
    let packet = Packet::create(source, pdn_aprs::Address::new("APZ001").unwrap(), Vec::new(), Data::MicE(report)).unwrap();
    assert!(packet.information.ends_with(b"/A=000101 {5}"), "{}", String::from_utf8_lossy(&packet.information));
    let back = Packet::decode(packet.source, packet.destination, Vec::new(), &packet.information, ParseOptions::STRICT);
    let Data::MicE(m) = back.data else { panic!("a Mic-E report") };
    assert_eq!(m.fields.comment, "{5}");
    assert_eq!(m.fields.altitude_feet, Some(101.0));
}

/// Rev 0 Mic-E telemetry whose last byte is a CR would end the information field in one, which
/// reads back as a trailing line break: refused.
#[test]
fn mic_e_telemetry_ending_in_a_cr_is_refused() {
    let report = |last: u8| pdn_aprs::MicEReport {
        legacy_telemetry: vec![203, 184, 177, 127, last],
        fields: pdn_aprs::Positioned {
            position: pdn_aprs::Position { latitude: 83.34616666666666, longitude: 40.285833333333336, ambiguity: 0 },
            symbol: pdn_aprs::Symbol::CAR,
            course_degrees: Some(265.0),
            speed_knots: Some(1.0),
            ..Default::default()
        },
        ..Default::default()
    };
    let source = pdn_aprs::Address::new("N0CALL").unwrap();
    let create = |report| Packet::create(source.clone(), pdn_aprs::Address::new("APZ001").unwrap(), Vec::new(), Data::MicE(report));
    assert!(create(report(13)).is_err());
    assert!(create(report(14)).is_ok());
}
