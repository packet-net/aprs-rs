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
