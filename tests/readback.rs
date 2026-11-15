//! The read-back guarantee, through the public API: what an encoder writes reads back as the data it
//! was given, in an equivalent form where the first does not, or the data is refused; and the header
//! it writes reads back as the header it was given.

use pdn_aprs::*;

fn position(comment: &str) -> Positioned {
    Positioned {
        position: Position { latitude: 51.5, longitude: -0.5, ambiguity: 0 },
        symbol: Symbol::new('/', '>'),
        comment: comment.into(),
        ..Default::default()
    }
}

fn written(data: Data) -> String {
    String::from_utf8(data.encode().unwrap()).unwrap()
}

fn refused(data: Data) -> RefusalCode {
    data.encode().unwrap_err().code()
}

fn status(text: &str) -> Data {
    Data::Status(Status { text: text.into(), ..Default::default() })
}

fn me() -> Address {
    Address::new("N0CALL").unwrap()
}

fn to() -> Address {
    Address::new("APZ001").unwrap()
}

// ------------------------------------------------------------------ the first form that does not read back

#[test]
fn a_comment_that_reads_as_a_data_extension_is_written_after_a_slash() {
    // `088/036` straight after the symbol is a course and speed; after a `/` it is text.
    let text = "088/036";
    assert_eq!(written(Data::Position(PositionReport { fields: position(text), ..Default::default() })), "!5130.00N/00030.00W>/088/036");
    let object =
        ObjectReport { name: "OBJ".into(), timestamp: Some(Timestamp::dhm(1, 2, 3)), fields: position(text), ..Default::default() };
    assert_eq!(written(Data::Object(object)), ";OBJ      *010203z5130.00N/00030.00W>/088/036");
    let item = ItemReport { name: "ITEM".into(), fields: position(text), ..Default::default() };
    assert_eq!(written(Data::Item(item)), ")ITEM!5130.00N/00030.00W>/088/036");
}

#[test]
fn a_comment_no_form_holds_is_refused_by_every_positioned_report() {
    for text in ["/A=000100", "PHG5130", "RNG0050", "hello /A=000100", "|!!!!!!!!!!!!|"] {
        let object =
            ObjectReport { name: "OBJ".into(), timestamp: Some(Timestamp::dhm(1, 2, 3)), fields: position(text), ..Default::default() };
        let item = ItemReport { name: "ITEM".into(), fields: position(text), ..Default::default() };
        for data in
            [Data::Position(PositionReport { fields: position(text), ..Default::default() }), Data::Object(object), Data::Item(item)]
        {
            assert_eq!(refused(data), RefusalCode::TextReadsAsField, "{text}");
        }
    }
}

#[test]
fn text_that_reads_as_another_report_is_refused() {
    let message = |text: &str| Data::Message(Message { addressee: "N0CALL".into(), text: text.into(), ..Default::default() });
    assert_eq!(refused(message("ack12")), RefusalCode::TextReadsAsField);
    assert_eq!(refused(message("rej12")), RefusalCode::TextReadsAsField);
    let bulletin = Data::Bulletin(Bulletin { addressee: "BLN1".into(), text: "ack12".into(), message_id: None });
    assert_eq!(refused(bulletin), RefusalCode::TextReadsAsField);
    for text in ["251830z", "IO91SX/G", "hello ^7A"] {
        assert_eq!(refused(status(text)), RefusalCode::TextReadsAsField, "{text}");
    }
}

#[test]
fn a_mic_e_comment_that_reads_as_an_altitude_is_refused() {
    let report = |comment: &str| {
        let fields = Positioned { comment: comment.into(), speed_knots: Some(10.0), course_degrees: Some(90.0), ..position("") };
        let r = MicEReport { type_code: Some('`'), fields, ..Default::default() };
        Packet::create(me(), to(), vec![], Data::MicE(r))
    };
    for text in ["abc}def", "/A=000100", "PHG5130"] {
        assert_eq!(report(text).unwrap_err().code(), RefusalCode::TextReadsAsField, "{text}");
    }
    assert!(report("hello").is_ok());
}

#[test]
fn a_third_party_packet_whose_information_is_not_its_data_is_refused() {
    // The inner information field is written as received, so data changed after the decode is caught by the read-back.
    let mut inner = Packet::decode_tnc2(b"G4ABC>APZ001:>hi", ParseOptions::LENIENT).unwrap();
    inner.information = b">ho".to_vec();
    let outer = Data::ThirdParty(Box::new(inner));
    assert_eq!(refused(outer), RefusalCode::TextReadsAsField);
    // Not text: the inner position written is not the one given.
    let mut inner = Packet::decode_tnc2(b"G4ABC>APZ001:!5127.26N/00058.69W>", ParseOptions::LENIENT).unwrap();
    inner.information = b"!5127.27N/00058.69W>".to_vec();
    assert_eq!(refused(Data::ThirdParty(Box::new(inner))), RefusalCode::DoesNotReadBack);
}

#[test]
fn a_third_party_packet_is_checked_whole_and_its_inner_diagnostics_are_data() {
    // An inner packet that decodes with a warning writes back as received: its warning is in the data given.
    let line = b"N0CALL>APZ001:}G4ABC>APZ001,TCPIP,N0CALL*:>Net \x07Control";
    let packet = Packet::decode_tnc2(line, ParseOptions::LENIENT).unwrap();
    assert_eq!(packet.data.encode().unwrap(), &line[line.iter().position(|&b| b == b':').unwrap() + 1..]);
    // The same data with the warning taken off the inner packet is not what the bytes read back as.
    let Data::ThirdParty(mut inner) = packet.data else { panic!() };
    inner.diagnostics.clear();
    assert_eq!(refused(Data::ThirdParty(inner)), RefusalCode::DoesNotReadBack);
}

#[test]
fn what_is_written_over_256_bytes_is_read_back_first() {
    // Text the size would refuse is judged by what it reads back as first.
    assert_eq!(refused(status(&format!("251830z{}", "x".repeat(300)))), RefusalCode::TextReadsAsField);
    assert_eq!(refused(status(&"x".repeat(300))), RefusalCode::InformationFieldTooLong);
}

// ------------------------------------------------------------------ empty is absent

#[test]
fn an_empty_message_id_and_an_empty_target_are_absent() {
    let message = Message { addressee: "N0CALL".into(), text: "hi".into(), message_id: Some(String::new()), reply_ack: None };
    assert_eq!(written(Data::Message(message)), ":N0CALL   :hi");
    let query = DirectedQuery { addressee: "N0CALL".into(), query_type: "APRSP".into(), target: Some(String::new()) };
    assert_eq!(written(Data::DirectedQuery(query)), ":N0CALL   :?APRSP");
    // An empty reply-ack is a value, and goes with a message ID.
    let reply = Message { addressee: "N0CALL".into(), text: "hi".into(), message_id: Some("12".into()), reply_ack: Some(String::new()) };
    assert_eq!(written(Data::Message(reply)), ":N0CALL   :hi{12}");
}

// ------------------------------------------------------------------ the header

fn path(entries: &[(&str, bool)]) -> Vec<PathEntry> {
    entries.iter().map(|(a, used)| PathEntry { address: Address::new(a).unwrap(), used: *used }).collect()
}

fn packet(entries: &[(&str, bool)]) -> Packet {
    Packet::create(me(), to(), path(entries), status("hi")).unwrap()
}

#[test]
fn a_tnc2_line_marks_the_last_used_entry_only() {
    let p = packet(&[("WIDE1-1", true), ("N0CALL-1", true), ("WIDE2-1", false)]);
    assert_eq!(p.to_tnc2().unwrap(), b"N0CALL>APZ001,WIDE1-1,N0CALL-1*,WIDE2-1:>hi");
    let read = Packet::decode_tnc2(&p.to_tnc2().unwrap(), ParseOptions::STRICT).unwrap();
    assert_eq!(read.path, p.path);
}

#[test]
fn an_ax25_frame_sets_the_h_bit_of_every_used_entry() {
    let p = packet(&[("WIDE1-1", true), ("N0CALL-1", true), ("WIDE2-1", false)]);
    let frame = p.to_ax25().unwrap();
    let hex: String = frame.iter().map(|b| format!("{b:02X}")).collect();
    assert_eq!(hex, "82A0B4606062E09C608682989860AE92888A6240E29C6086829898E2AE92888A64406303F03E6869");
}

#[test]
fn a_path_entry_not_used_before_a_used_one_cannot_be_carried() {
    // Building builds it: the refusal comes from the call that writes a form, and a fault in the data comes first.
    let unwritable = path(&[("WIDE1-1", false), ("N0CALL-1", true)]);
    let p = Packet::create(me(), to(), unwritable.clone(), status("hi")).unwrap();
    assert_eq!(p.to_tnc2().unwrap_err().code(), RefusalCode::Tnc2CannotCarry);
    assert_eq!(p.to_ax25().unwrap_err().code(), RefusalCode::Ax25CannotCarry);
    let bad_name = Data::Item(pdn_aprs::ItemReport { name: "TOOLONGNAMEX".into(), ..Default::default() });
    assert_eq!(Packet::create(me(), to(), unwritable, bad_name).unwrap_err().code(), RefusalCode::InvalidName);
    // A call a frame cannot carry is built, written in a line, and refused only when the frame is written.
    let nine = Packet::create(Address::new("N0CALLXYZ").unwrap(), to(), Vec::new(), status("hi")).unwrap();
    assert_eq!(nine.to_tnc2().unwrap(), b"N0CALLXYZ>APZ001:>hi");
    assert_eq!(nine.to_ax25().unwrap_err().code(), RefusalCode::Ax25CannotCarry);
    // A path with no used entry, and one with a used first entry, are written.
    assert_eq!(packet(&[("WIDE1-1", false), ("WIDE2-1", false)]).to_tnc2().unwrap(), b"N0CALL>APZ001,WIDE1-1,WIDE2-1:>hi");
    assert_eq!(packet(&[("N0CALL-1", true), ("WIDE2-1", false)]).to_tnc2().unwrap(), b"N0CALL>APZ001,N0CALL-1*,WIDE2-1:>hi");
}

#[test]
fn the_header_of_a_decoded_packet_is_checked_when_it_is_written_out() {
    // The information field passes through as received; the header is read back.
    let mut p = Packet::decode_tnc2(b"N0CALL>APZ001,WIDE1-1*,WIDE2-1:>hi", ParseOptions::LENIENT).unwrap();
    assert_eq!(p.to_tnc2().unwrap(), b"N0CALL>APZ001,WIDE1-1*,WIDE2-1:>hi");
    p.path[0].used = false;
    p.path[1].used = true;
    assert_eq!(p.to_tnc2().unwrap_err().code(), RefusalCode::Tnc2CannotCarry);
    assert_eq!(p.to_ax25().unwrap_err().code(), RefusalCode::Ax25CannotCarry);
    // An empty destination, tolerated on the way in, is written as it came: the line reads back with the same empty destination.
    let empty = Packet::decode_tnc2(b"N0CALL>:>hi", ParseOptions::LENIENT).unwrap();
    assert_eq!(empty.to_tnc2().unwrap(), b"N0CALL>:>hi");
    // A source that is not an address does not read back as the same header.
    let mut bad = Packet::decode_tnc2(b"N0CALL>APZ001:>hi", ParseOptions::LENIENT).unwrap();
    bad.source = Address::third_party_source("A_B").unwrap();
    assert_eq!(bad.to_tnc2().unwrap_err().code(), RefusalCode::InvalidAddress);
}

#[test]
fn a_station_path_entry_ending_in_a_star_is_a_used_entry() {
    let station = Station::new("N0CALL").unwrap().via(&["WIDE1-1*", "N0CALL-1*", "WIDE2-1"]).unwrap();
    assert_eq!(station.path(), path(&[("WIDE1-1", true), ("N0CALL-1", true), ("WIDE2-1", false)]).as_slice());
    let built = station.status("hi").build().unwrap();
    assert_eq!(built.to_tnc2().unwrap(), b"N0CALL>APZ001,WIDE1-1,N0CALL-1*,WIDE2-1:>hi");
    assert!(Station::new("N0CALL").unwrap().via(&["*"]).is_err());
    assert!(Station::new("N0CALL").unwrap().via(&["WIDE1-1**"]).is_err());
}

#[test]
fn an_ax25_h_bit_marks_every_entry_before_it_used() {
    let heard =
        Packet::decode_ax25(&hex("82A0B4606062E09C608682989860AE92888A6240629C6086829898E303F03E78"), ParseOptions::LENIENT).unwrap();
    assert!(heard.diagnostics.is_empty());
    assert_eq!(heard.path.iter().map(|e| e.used).collect::<Vec<_>>(), [true, true]);
    assert_eq!(heard.to_tnc2().unwrap(), b"N0CALL>APZ001,WIDE1-1,N0CALL-1*:>x");
}

#[test]
fn an_empty_destination_is_six_spaces_in_a_frame() {
    let line = Packet::decode_tnc2(b"N0CALL>:>x", ParseOptions::LENIENT).unwrap();
    let frame = line.to_ax25().unwrap();
    assert_eq!(&frame[..7], &[0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0xE0]);
    assert_eq!(Packet::decode_ax25(&frame, ParseOptions::LENIENT).unwrap().to_tnc2().unwrap(), b"N0CALL>:>x");
}

#[test]
fn a_builders_empty_message_id_is_refused() {
    let station = pdn_aprs::Station::new("N0CALL").unwrap();
    assert_eq!(station.message("N0CALL", "hi").message_id("").build().unwrap_err().code(), RefusalCode::InvalidMessageId);
    assert_eq!(station.message("N0CALL", "hi").message_id("").reply_ack("1").build().unwrap_err().code(), RefusalCode::InvalidMessageId);
}

fn hex(text: &str) -> Vec<u8> {
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap()).collect()
}
