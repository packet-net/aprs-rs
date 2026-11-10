//! The packet inside a third-party packet (APRS12c ch. 17).

use pdn_aprs::{Data, Packet, ParseOptions};

/// A q-construct is read only in the outer header: the inner packet's path is kept as sent, so
/// its `qAR` is just a path entry (vectors README, "Addresses and path").
#[test]
fn an_inner_packet_has_no_q_construct() {
    let packet = Packet::decode_tnc2(b"N0CALL>APZ001:}N1CALL>APZ001,WIDE2-1,qAR,N2CALL:>hello", ParseOptions::LENIENT).unwrap();
    assert!(!packet.third_party);
    assert!(packet.q_construct().is_none());
    let Data::ThirdParty(inner) = &packet.data else { panic!("a third-party packet") };
    assert!(inner.third_party);
    assert_eq!(inner.path.iter().map(|e| e.address.as_str()).collect::<Vec<_>>(), ["WIDE2-1", "qAR", "N2CALL"]);
    assert!(inner.q_construct().is_none());
}

/// The same header outside a third-party packet does have one.
#[test]
fn an_outer_packet_keeps_its_q_construct() {
    let packet = Packet::decode_tnc2(b"N1CALL>APZ001,WIDE2-1,qAR,N2CALL:>hello", ParseOptions::LENIENT).unwrap();
    let q = packet.q_construct().unwrap();
    assert_eq!(q.construct, "qAR");
    assert_eq!(q.station.unwrap().as_str(), "N2CALL");
}

/// Diagnostics come in reading order: the beacon rule that finds a position after other text
/// comes before what the position itself holds.
#[test]
fn a_late_position_is_noted_before_its_own_diagnostics() {
    let line = b"N0ZO-13>APX220:-12!3309.71N/08643.18W_030/002g003t069r000000P000h080b10133";
    let packet = pdn_aprs::Packet::decode_tnc2(line, pdn_aprs::ParseOptions::LENIENT).unwrap();
    let codes: Vec<_> = packet.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, [pdn_aprs::Code::PositionNotAtStart, pdn_aprs::Code::WeatherComment]);
}
