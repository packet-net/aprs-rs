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
    // The same data with the warning taken off the inner packet is not what the bytes read back as; its text
    // holds a control character, which the inner data's own rule names first, as it does for data on its own.
    let Data::ThirdParty(mut inner) = packet.data else { panic!() };
    inner.diagnostics.clear();
    assert_eq!(refused(Data::ThirdParty(inner)), RefusalCode::InvalidCharacter);
}

#[test]
fn an_inner_warning_the_written_bytes_do_not_carry_has_no_form() {
    // Inner text with no control character, given a warning that only a control character raises: the
    // bytes written read back without it, and no form of the inner packet carries it.
    let warned = Packet::decode_tnc2(b"G4ABC>APZ001:>Net \x07Control", ParseOptions::LENIENT).unwrap();
    assert!(warned.has_warnings());
    let mut inner = Packet::decode_tnc2(b"G4ABC>APZ001:>hi", ParseOptions::LENIENT).unwrap();
    inner.diagnostics = warned.diagnostics.clone();
    assert_eq!(refused(Data::ThirdParty(Box::new(inner.clone()))), RefusalCode::ValueHasNoForm);
    // At every depth.
    let mut middle = Packet::decode_tnc2(b"G4ABC>APZ001:}G4ABC>APZ001:>hi", ParseOptions::LENIENT).unwrap();
    let Data::ThirdParty(deepest) = &mut middle.data else { panic!() };
    deepest.diagnostics = warned.diagnostics;
    assert_eq!(refused(Data::ThirdParty(Box::new(middle))), RefusalCode::ValueHasNoForm);
}

#[test]
fn inner_data_is_held_to_every_rule_at_every_depth() {
    // A value no rule lets through, in inner data changed after the decode, is that rule's refusal, not a failed read-back.
    let mut inner = Packet::decode_tnc2(b"G4ABC>APZ001:!5127.26N/00058.69W>", ParseOptions::LENIENT).unwrap();
    let Data::Position(report) = &mut inner.data else { panic!() };
    report.fields.position.latitude = f64::NAN;
    assert_eq!(refused(Data::ThirdParty(Box::new(inner.clone()))), RefusalCode::NotANumber);
    let Data::Position(report) = &mut inner.data else { panic!() };
    report.fields.position.latitude = 91.0;
    assert_eq!(refused(Data::ThirdParty(Box::new(inner.clone()))), RefusalCode::ValueOutOfRange);
    let mut outer = Packet::decode_tnc2(b"G4ABC>APZ001:}G4ABC>APZ001:!5127.26N/00058.69W>", ParseOptions::LENIENT).unwrap();
    let Data::ThirdParty(deepest) = &mut outer.data else { panic!() };
    let Data::Position(report) = &mut deepest.data else { panic!() };
    report.fields.course_degrees = Some(f64::INFINITY);
    assert_eq!(refused(Data::ThirdParty(Box::new(outer))), RefusalCode::NotANumber);
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
    // The header is judged when it is given, in every form, before any fault in the data: `invalid-path`.
    let unwritable = path(&[("WIDE1-1", false), ("N0CALL-1", true)]);
    assert_eq!(Packet::create(me(), to(), unwritable.clone(), status("hi")).unwrap_err().code(), RefusalCode::InvalidPath);
    let bad_name = Data::Item(pdn_aprs::ItemReport { name: "TOOLONGNAMEX".into(), ..Default::default() });
    assert_eq!(Packet::create(me(), to(), unwritable.clone(), bad_name).unwrap_err().code(), RefusalCode::InvalidPath);
    // An address no header carries is named before the path.
    let mut bad = packet(&[]);
    bad.source = Address::third_party_source("A B").unwrap();
    bad.path = unwritable;
    assert_eq!(bad.to_tnc2().unwrap_err().code(), RefusalCode::InvalidAddress);
    assert_eq!(bad.to_ax25().unwrap_err().code(), RefusalCode::InvalidAddress);
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
    assert_eq!(p.to_tnc2().unwrap_err().code(), RefusalCode::InvalidPath);
    assert_eq!(p.to_ax25().unwrap_err().code(), RefusalCode::InvalidPath);
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
fn an_empty_destination_is_never_written_to_a_frame() {
    // A line carries it as received; a frame never carries what AX.25 forbids.
    let line = Packet::decode_tnc2(b"N0CALL>:>x", ParseOptions::LENIENT).unwrap();
    assert_eq!(line.to_ax25().unwrap_err().code(), RefusalCode::Ax25CannotCarry);
    // A frame that came with six spaces is read as an empty destination, which a line carries.
    let packet = Packet::decode_ax25(&hex("404040404040E09C60868298986103F03E78"), ParseOptions::LENIENT).unwrap();
    assert_eq!(packet.to_tnc2().unwrap(), b"N0CALL>:>x");
    assert_eq!(packet.to_ax25().unwrap_err().code(), RefusalCode::Ax25CannotCarry);
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

// ------------------------------------------------------------------ the Mic-E altitude

fn long_comment() -> String {
    format!("{}/A=001234{}WX", "x".repeat(96), "x".repeat(150))
}

#[test]
fn a_mic_e_altitude_in_a_slash_form_that_reads_back_but_is_too_long_is_refused_for_its_size() {
    // The Mic-E bytes do not read back (the comment holds a /A= of its own), the /A= form does, and it is over 256 bytes.
    let built = Station::new("N0CALL")
        .unwrap()
        .mic_e(51.5, -0.5)
        .symbol(Symbol::CAR)
        .altitude_feet(1250.0)
        .comment(&long_comment())
        .messaging()
        .build();
    assert_eq!(built.unwrap_err().code(), RefusalCode::InformationFieldTooLong);
    let fields = Positioned { altitude_feet: Some(1250.0), comment: long_comment(), ..position("") };
    let report = MicEReport { type_code: Some('`'), fields, ..Default::default() };
    let created = Packet::create(me(), to(), vec![], Data::MicE(report));
    assert_eq!(created.unwrap_err().code(), RefusalCode::InformationFieldTooLong);
}

#[test]
fn a_built_mic_e_packet_holds_the_altitude_as_given() {
    // 1000 feet is 304.8 metres: written as 305 in the Mic-E bytes, held as 1000 feet.
    let built = Station::new("N0CALL").unwrap().mic_e(51.5, -0.5).symbol(Symbol::CAR).altitude_feet(1000.0).build().unwrap();
    let Data::MicE(report) = &built.data else { panic!() };
    assert_eq!(report.fields.altitude_feet, Some(1000.0));
    assert!(!report.altitude_in_comment);
    assert!(built.information.contains(&b'}'), "the Mic-E altitude bytes end in a closing brace");
    assert!(!String::from_utf8_lossy(&built.information).contains("/A="));
    // The same data sent on its own is written by the rule for any data: a whole number of feet that the
    // bytes do not hold exactly goes in a /A= first. The value in the data is the same either way.
    let again = Packet::create(me(), to(), vec![], built.data.clone()).unwrap();
    assert!(String::from_utf8_lossy(&again.information).contains("/A=001000"));
}

#[test]
fn a_built_mic_e_altitude_the_comment_makes_unreadable_goes_in_a_slash_form_that_holds_it_exactly() {
    let built =
        Station::new("N0CALL").unwrap().mic_e(51.5, -0.5).symbol(Symbol::CAR).altitude_feet(1250.0).comment("a/A=001234b").build().unwrap();
    let text = String::from_utf8_lossy(&built.information);
    assert!(text.contains("/A=001250"), "{text}");
    // A value the /A= does not hold exactly is refused rather than written.
    let refused =
        Station::new("N0CALL").unwrap().mic_e(51.5, -0.5).symbol(Symbol::CAR).altitude_feet(1250.4).comment("a/A=001234b").build();
    assert_eq!(refused.unwrap_err().code(), RefusalCode::TextReadsAsField);
}

// ------------------------------------------------------------------ a packet written on its own

#[test]
fn an_inner_packet_written_on_its_own_is_read_back_by_the_outer_rules() {
    // Its source is valid inside the third-party packet; as a line of its own decode_tnc2 would refuse it.
    let outer = Packet::decode_tnc2(b"N0CALL>APZ001:}PY2SP_R-R>APZ001,TCPIP,N0CALL*:>hi", ParseOptions::LENIENT).unwrap();
    let Data::ThirdParty(inner) = &outer.data else { panic!() };
    assert_eq!(inner.to_tnc2().unwrap_err().code(), RefusalCode::InvalidAddress);
    assert_eq!(inner.to_ax25().unwrap_err().code(), RefusalCode::InvalidAddress);
    // The packet it is in is written as received, inner header included, and reads back by the third-party rules.
    assert_eq!(outer.to_tnc2().unwrap(), b"N0CALL>APZ001:}PY2SP_R-R>APZ001,TCPIP,N0CALL*:>hi");
    // A q-construct in an inner path is not compared; a frame cannot carry its lower-case q.
    let outer = Packet::decode_tnc2(b"N0CALL>APRS:}N0CALL>APRS,qAR,X:>hi", ParseOptions::LENIENT).unwrap();
    let Data::ThirdParty(inner) = &outer.data else { panic!() };
    assert_eq!(inner.to_tnc2().unwrap(), b"N0CALL>APRS,qAR,X:>hi");
    assert_eq!(inner.to_ax25().unwrap_err().code(), RefusalCode::Ax25CannotCarry);
}

#[test]
fn a_line_is_not_written_with_a_line_break_inside_its_information_field() {
    let mut p = packet(&[]);
    for inside in [&b">bye\r\nN0CALL>APZ001:>evil"[..], b">a\rb", b">a\nb", b"\r>hi"] {
        p.information = inside.to_vec();
        assert_eq!(p.to_tnc2().unwrap_err().code(), RefusalCode::Tnc2CannotCarry, "{inside:?}");
        // A frame carries it.
        assert!(p.to_ax25().is_ok(), "{inside:?}");
    }
    // At the very end, where a decoder takes it off as a trailing line break, it ends the line anyway and is carried.
    for end in [&b">hi\r"[..], b">hi\n", b">hi\r\n"] {
        p.information = end.to_vec();
        assert_eq!(p.to_tnc2().unwrap(), [&b"N0CALL>APZ001:"[..], end].concat());
    }
    // A header fault is named before the line break.
    p.information = b">a\rb".to_vec();
    p.source = Address::third_party_source("A B").unwrap();
    assert_eq!(p.to_tnc2().unwrap_err().code(), RefusalCode::InvalidAddress);
}

#[test]
fn a_user_defined_packet_cannot_inject_a_line() {
    let line = b"N0CALL>APZ001:{Axa\nX>Y:>evil";
    let p = Packet::decode_tnc2(line, ParseOptions::LENIENT).unwrap();
    assert_eq!(p.to_tnc2().unwrap_err().code(), RefusalCode::Tnc2CannotCarry);
}

#[test]
fn a_display_is_not_a_writer() {
    // Display of what is held is total: a header and data no writer would write are shown as held.
    let mut p = packet(&[("WIDE1-1", true), ("WIDE2-1", true)]);
    p.path[0].used = false;
    p.source = Address::third_party_source("A B").unwrap();
    p.information = b">a\rb".to_vec();
    assert_eq!(p.source.to_string(), "A B");
    assert!(format!("{p:?}").contains("A B"));
    let t = Timestamp::dhm(31, 25, 61);
    assert!(!t.to_string().is_empty());
}
