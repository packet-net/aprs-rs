//! Refusals carry a reason code a program can act on: one representative from each group.

use pdn_aprs::{Address, Data, Message, Packet, PathEntry, Position, PositionReport, Positioned, RefusalCode, Station, Symbol};

fn message(addressee: &str, text: &str, id: Option<&str>) -> Data {
    Data::Message(Message { addressee: addressee.into(), text: text.into(), message_id: id.map(Into::into), ..Message::default() })
}

fn position(fields: Positioned) -> Data {
    Data::Position(PositionReport { fields: Positioned { symbol: Symbol::CAR, ..fields }, ..PositionReport::default() })
}

fn code(data: Data) -> RefusalCode {
    data.encode().unwrap_err().code()
}

#[test]
fn text_and_address_groups() {
    assert_eq!(code(message("", "hi", None)), RefusalCode::InvalidAddressee);
    assert_eq!(code(message("N0CALL", &"x".repeat(68), None)), RefusalCode::TooLong);
    assert_eq!(code(message("N0CALL", "a{b", None)), RefusalCode::InvalidCharacter);
    assert_eq!(code(message("N0CALL", "hi", Some("toolong"))), RefusalCode::InvalidMessageId);
}

#[test]
fn value_and_field_groups() {
    let at = |latitude, longitude| Positioned { position: Position { latitude, longitude, ambiguity: 0 }, ..Positioned::default() };
    assert_eq!(code(position(at(91.0, 0.0))), RefusalCode::ValueOutOfRange);
}

#[test]
fn ax25_header_refusals() {
    let long = Address::new("N0CALLXYZ").unwrap();
    let packet = Packet::create(long, Address::new("APZ001").unwrap(), Vec::new(), message("N0CALL", "hi", None)).unwrap();
    assert_eq!(packet.to_ax25().unwrap_err().code(), RefusalCode::Ax25CannotCarry);
    let path: Vec<PathEntry> = (0..9).map(|_| PathEntry::new(Address::new("WIDE1-1").unwrap())).collect();
    let packet =
        Packet::create(Address::new("N0CALL").unwrap(), Address::new("APZ001").unwrap(), path, message("N0CALL", "hi", None)).unwrap();
    assert_eq!(packet.to_ax25().unwrap_err().code(), RefusalCode::Ax25CannotCarry);
}

#[test]
fn builder_refusals_and_ids() {
    let station = Station::new("N0CALL").unwrap();
    assert_eq!(station.message("G3NRW", &"x".repeat(68)).build().unwrap_err().code(), RefusalCode::TooLong);
    assert_eq!(station.object("TESTOBJ").at(1.0, 1.0).symbol(Symbol::CAR).build().unwrap_err().code(), RefusalCode::MissingField);
    let weather = station.position(1.0, 1.0).symbol(Symbol::CAR);
    assert!(weather.build().is_ok());
    assert_eq!(
        station
            .weather()
            .at(1.0, 1.0)
            .symbol(Symbol::WEATHER_STATION)
            .temperature_f(50.0)
            .humidity_percent(0.0)
            .build()
            .unwrap_err()
            .code(),
        RefusalCode::ValueHasNoForm
    );
    assert_eq!(RefusalCode::ValueHasNoForm.id(), "value-has-no-form");
    assert_eq!(RefusalCode::from_id("does-not-read-back"), Some(RefusalCode::DoesNotReadBack));
    assert_eq!(RefusalCode::ALL.len(), 26);
    for c in RefusalCode::ALL {
        assert_eq!(RefusalCode::from_id(c.id()), Some(c));
    }
}

#[test]
fn several_faults_name_the_first_code_in_the_order() {
    // Out of range comes before a line break, which comes before a too-long text.
    let at = |latitude| Positioned {
        position: Position { latitude, longitude: 0.0, ambiguity: 0 },
        comment: "a\nb".into(),
        ..Positioned::default()
    };
    assert_eq!(code(position(at(95.0))), RefusalCode::ValueOutOfRange);
    assert_eq!(code(position(at(f64::NAN))), RefusalCode::NotANumber);
    assert_eq!(code(message("N0CALL", &format!("{{{}", "x".repeat(70)), None)), RefusalCode::InvalidCharacter);
    // A message to a bulletin addressee reads back as a bulletin: the addressee is the fault.
    assert_eq!(code(message("BLN1", "hi", None)), RefusalCode::InvalidAddressee);
}

#[test]
fn a_mic_e_altitude_in_the_text_wins_even_when_equal() {
    let line = b"DO1BR-9>UR1QW0:`~\\(\x1c\x1d;(/3\"3r}DO1BR6W&/A=00000070cm";
    let packet = Packet::decode_tnc2(line, pdn_aprs::ParseOptions::LENIENT).unwrap();
    let Data::MicE(report) = &packet.data else { panic!("not Mic-E") };
    assert!(report.altitude_in_comment);
    let again = Packet::create(packet.source.clone(), packet.destination.clone(), packet.path.clone(), packet.data.clone()).unwrap();
    assert!(again.information.windows(8).any(|w| w == b"/A=00000"), "{:?}", String::from_utf8_lossy(&again.information));
}
