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
