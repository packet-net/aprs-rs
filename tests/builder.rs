//! The fluent builder: each kind of packet it builds, written out as TNC2 and read back under strict
//! parsing, plus the defaults and the refusals. The expected lines are the ones Packet.Aprs's builder
//! tests expect, so the two builders are held to the same output.

use pdn_aprs::{Data, MicEMessage, Packet, ParseOptions, Phg, PositionReport, Severity, Station, Symbol, Timestamp};

fn me() -> Station {
    Station::new("M0LTE").unwrap()
}

/// The packet as TNC2 text, after checking it reads back under strict parsing with no warning or error
/// (a positionless weather report still gets its "not recommended" note).
fn sent(packet: Packet) -> String {
    let line = String::from_utf8(packet.to_tnc2()).unwrap();
    let heard = Packet::decode_tnc2(line.as_bytes(), ParseOptions::STRICT).unwrap();
    let complaints: Vec<_> = heard.diagnostics.iter().filter(|d| d.severity != Severity::Info).collect();
    assert!(complaints.is_empty(), "{line}: {complaints:?}");
    assert!(!matches!(heard.data, Data::Unrecognized(_)), "{line}");
    line
}

fn heard(line: &str) -> Data {
    Packet::decode_tnc2(line.as_bytes(), ParseOptions::LENIENT).unwrap().data
}

#[test]
fn a_position_report() {
    let packet = Station::new("M0LTE-9")
        .unwrap()
        .via(&["WIDE1-1", "WIDE2-1"])
        .unwrap()
        .position(51.4543, -0.9781)
        .symbol(Symbol::CAR)
        .course(88)
        .speed(36.0)
        .altitude(120.0)
        .comment("Mobile")
        .build()
        .unwrap();
    assert_eq!(sent(packet), "M0LTE-9>APZ001,WIDE1-1,WIDE2-1:!5127.26N/00058.69W>088/036/A=000120Mobile");
}

#[test]
fn a_position_report_with_messaging_and_a_timestamp() {
    let packet = me().position(51.4543, -0.9781).symbol(Symbol::HOUSE).messaging().timestamp(Timestamp::dhm(26, 14, 5)).build().unwrap();
    assert_eq!(sent(packet), "M0LTE>APZ001:@261405z5127.26N/00058.69W-");
}

#[test]
fn a_compressed_position_decodes_to_what_was_given() {
    let line = sent(me().position(51.4543, -0.9781).symbol(Symbol::CAR).compressed().build().unwrap());
    let Data::Position(report) = heard(&line) else { panic!("{line}") };
    assert!(report.fields.compressed);
    assert!((report.fields.position.latitude - 51.4543).abs() < 1e-5);
    assert!((report.fields.position.longitude + 0.9781).abs() < 1e-5);
}

#[test]
fn other_units_are_converted_to_the_ones_aprs_sends() {
    let Data::Position(report) =
        me().position(51.4543, -0.9781).symbol(Symbol::CAR).speed_kmh(100.0).altitude_metres(100.0).to_data().unwrap()
    else {
        panic!()
    };
    assert!((report.fields.speed_knots.unwrap() - 54.0).abs() < 0.1);
    assert!((report.fields.altitude_feet.unwrap() - 328.08).abs() < 0.01);
}

#[test]
fn ambiguity_blanks_digits() {
    assert_eq!(
        sent(me().position(51.4543, -0.9781).ambiguity(2).symbol(Symbol::HOUSE).build().unwrap()),
        "M0LTE>APZ001:!5127.  N/00058.  W-"
    );
    let hidden = me().object("HIDDEN").ambiguity(1).at(51.4543, -0.9781).symbol(Symbol::HOUSE).timestamp(Timestamp::dhm(26, 14, 5));
    assert_eq!(sent(hidden.build().unwrap()), "M0LTE>APZ001:;HIDDEN   *261405z5127.2 N/00058.6 W-");
}

#[test]
fn messages() {
    assert_eq!(sent(me().message("G3NRW", "Hi Ian").id("01").build().unwrap()), "M0LTE>APZ001::G3NRW    :Hi Ian{01");
    assert_eq!(sent(me().message("G3NRW", "Hi Ian").id("02").reply_ack("17").build().unwrap()), "M0LTE>APZ001::G3NRW    :Hi Ian{02}17");
    assert_eq!(sent(me().message("G3NRW", "Hi Ian").id("03").reply_ack("").build().unwrap()), "M0LTE>APZ001::G3NRW    :Hi Ian{03}");
}

#[test]
fn acks_and_rejects() {
    assert_eq!(sent(me().ack("G3NRW", "01").build().unwrap()), "M0LTE>APZ001::G3NRW    :ack01");
    assert_eq!(sent(me().reject("G3NRW", "01").build().unwrap()), "M0LTE>APZ001::G3NRW    :rej01");
    assert_eq!(sent(me().ack("G3NRW", "02").reply_ack("17").build().unwrap()), "M0LTE>APZ001::G3NRW    :ack02}17");
}

#[test]
fn bulletins() {
    assert_eq!(sent(me().bulletin('1', "Net tonight 8pm").build().unwrap()), "M0LTE>APZ001::BLN1     :Net tonight 8pm");
    assert_eq!(sent(me().bulletin('A', "Rally Sunday").build().unwrap()), "M0LTE>APZ001::BLNA     :Rally Sunday");
    assert_eq!(sent(me().group_bulletin('4', "WX", "Gales later").build().unwrap()), "M0LTE>APZ001::BLN4WX   :Gales later");
}

#[test]
fn an_object_announcing_a_repeater() {
    let repeater = me()
        .object("MYRPTR")
        .at(51.45, -0.98)
        .symbol(Symbol::REPEATER)
        .timestamp(Timestamp::dhm(25, 18, 30))
        .frequency(145.725)
        .tone(118.8)
        .offset_khz(-600);
    assert_eq!(sent(repeater.build().unwrap()), "M0LTE>APZ001:;MYRPTR   *251830z5127.00N/00058.80Wr145.725MHz T118 -060");
}

#[test]
fn a_permanent_object_and_a_killed_one() {
    let obj = me().object("MYRPTR").at(51.45, -0.98).symbol(Symbol::REPEATER).permanent();
    assert_eq!(sent(obj.clone().build().unwrap()), "M0LTE>APZ001:;MYRPTR   *111111z5127.00N/00058.80Wr");
    assert_eq!(sent(obj.kill().build().unwrap()), "M0LTE>APZ001:;MYRPTR   _111111z5127.00N/00058.80Wr");
}

#[test]
fn an_item_and_a_killed_one() {
    let item = me().item("AID #2").at(49.0 + 3.50 / 60.0, -(72.0 + 1.75 / 60.0)).symbol(Symbol::AID_STATION);
    assert_eq!(sent(item.clone().build().unwrap()), "M0LTE>APZ001:)AID #2!4903.50N/07201.75WA");
    assert_eq!(sent(item.kill().build().unwrap()), "M0LTE>APZ001:)AID #2_4903.50N/07201.75WA");
}

#[test]
fn a_mic_e_report_computes_its_destination_and_is_not_an_emergency_by_default() {
    let station = Station::new("M0LTE-9").unwrap().via(&["WIDE1-1"]).unwrap();
    let packet = station.mic_e(42.179, -71.1985).symbol(Symbol::CAR).course(215).speed(9.0).messaging().build().unwrap();
    assert_ne!(packet.destination.as_str(), pdn_aprs::DEFAULT_DESTINATION);
    let line = sent(packet);
    let Data::MicE(report) = heard(&line) else { panic!("{line}") };
    assert_eq!(report.message, MicEMessage::OffDuty);
    assert_eq!(report.type_code, Some('`'));
    assert!((report.fields.position.latitude - 42.179).abs() < 0.01 / 60.0);
    assert!((report.fields.position.longitude + 71.1985).abs() < 0.01 / 60.0);
    assert_eq!(report.fields.course_degrees, Some(215));
    assert_eq!(report.fields.speed_knots, Some(9.0));

    let line = sent(me().mic_e(42.179, -71.1985).symbol(Symbol::CAR).message(MicEMessage::EnRoute).build().unwrap());
    let Data::MicE(en_route) = heard(&line) else { panic!("{line}") };
    assert_eq!(en_route.message, MicEMessage::EnRoute);
}

#[test]
fn a_weather_report_with_a_position() {
    let packet = me().weather().at(51.45, -0.98).wind(220, 4.0).gust(5.0).temperature(77.0).humidity(54).pressure(1013.2).build().unwrap();
    assert_eq!(sent(packet), "M0LTE>APZ001:!5127.00N/00058.80W_220/004g005t077h54b10132");
}

#[test]
fn a_weather_report_without_a_position() {
    let packet = me()
        .weather()
        .timestamp(Timestamp::mdhm(9, 25, 18, 30))
        .wind(220, 4.0)
        .gust(5.0)
        .temperature_celsius(25.0)
        .rain_last_hour(0.1)
        .rain_since_midnight(0.25)
        .build()
        .unwrap();
    assert_eq!(sent(packet), "M0LTE>APZ001:_09251830c220s004g005t077r010P025");
}

#[test]
fn a_weather_report_needs_a_position_for_a_symbol_or_compression() {
    assert!(me().weather().timestamp(Timestamp::mdhm(9, 25, 18, 30)).temperature(77.0).compressed().build().is_err());
    assert!(me().weather().timestamp(Timestamp::mdhm(9, 25, 18, 30)).symbol(Symbol::WEATHER_STATION_WITH_DIGIPEATER).build().is_err());
    assert!(me().weather().temperature(77.0).build().is_err(), "a positionless report needs a timestamp");
}

#[test]
fn status_reports() {
    assert_eq!(sent(me().status("Net Control").build().unwrap()), "M0LTE>APZ001:>Net Control");
    assert_eq!(sent(me().status("Net Control").timestamp(Timestamp::dhm(26, 14, 5)).build().unwrap()), "M0LTE>APZ001:>261405zNet Control");
    assert_eq!(sent(me().status("On the air").locator("IO91SX", Symbol::HOUSE).build().unwrap()), "M0LTE>APZ001:>IO91SX/- On the air");
}

#[test]
fn telemetry_and_what_it_means() {
    let wx = Station::new("M0LTE-11").unwrap();
    assert_eq!(
        sent(wx.telemetry(123).analog(&[172.0, 123.0, 150.0, 50.0, 113.0]).digital(0b0000_0101).build().unwrap()),
        "M0LTE-11>APZ001:T#123,172,123,150,050,113,10100000"
    );
    assert_eq!(sent(wx.telemetry(7).analog(&[1.5]).build().unwrap()), "M0LTE-11>APZ001:T#007,1.5,,,,,00000000");
    assert_eq!(sent(wx.telemetry_names(&["Battery", "Temp"]).build().unwrap()), "M0LTE-11>APZ001::M0LTE-11 :PARM.Battery,Temp");
    assert_eq!(sent(wx.telemetry_units(&["V", "degC"]).build().unwrap()), "M0LTE-11>APZ001::M0LTE-11 :UNIT.V,degC");
    assert_eq!(
        sent(wx.telemetry_coefficients(&[0.0, 0.075, 0.0, 0.0, 0.5, -40.0]).build().unwrap()),
        "M0LTE-11>APZ001::M0LTE-11 :EQNS.0,0.075,0,0,0.5,-40"
    );
    assert_eq!(sent(wx.telemetry_bits(0xFF, "Garden station").build().unwrap()), "M0LTE-11>APZ001::M0LTE-11 :BITS.11111111,Garden station");
}

#[test]
fn comment_telemetry_rides_in_a_position_report() {
    let line = sent(me().position(51.4543, -0.9781).symbol(Symbol::HOUSE).comment("Home").telemetry(1, &[100, 200]).build().unwrap());
    let Data::Position(report) = heard(&line) else { panic!("{line}") };
    let telemetry = report.fields.telemetry.unwrap();
    assert_eq!((telemetry.sequence, telemetry.analog, telemetry.digital), (1, vec![100, 200], None));
    assert_eq!(report.fields.comment, "Home");
}

#[test]
fn the_station_is_a_value_and_sets_the_header() {
    let plain = me();
    let routed = plain.to("APXYZ1").unwrap().via(&["WIDE2-1"]).unwrap();
    assert_eq!(plain.destination().as_str(), "APZ001");
    assert!(plain.path().is_empty());
    assert_eq!(sent(routed.status("Hi").build().unwrap()), "M0LTE>APXYZ1,WIDE2-1:>Hi");
    assert!(Station::new("not a call!").is_err());
}

#[test]
fn to_data_gives_the_data_for_anything_the_builder_does_not_cover() {
    let Data::Position(report) = me().position(51.4543, -0.9781).symbol(Symbol::VALUE_SIGN).to_data().unwrap() else { panic!() };
    let signpost = PositionReport { fields: pdn_aprs::Positioned { signpost: Some("30".into()), ..report.fields }, ..report };
    assert_eq!(sent(me().data(Data::Position(signpost)).build().unwrap()), "M0LTE>APZ001:!5127.26N\\00058.69Wm{30}");
}

#[test]
fn missing_pieces_are_named() {
    let no_symbol = me().position(51.4543, -0.9781).build().unwrap_err();
    assert!(no_symbol.to_string().contains("symbol"), "{no_symbol}");
    let no_position = me().object("MYRPTR").symbol(Symbol::REPEATER).timestamp(Timestamp::dhm(1, 2, 3)).build().unwrap_err();
    assert!(no_position.to_string().contains("position"), "{no_position}");
    let no_timestamp = me().object("MYRPTR").at(51.45, -0.98).symbol(Symbol::REPEATER).build().unwrap_err();
    assert!(no_timestamp.to_string().contains("timestamp"), "{no_timestamp}");
    let tone_alone = me().position(51.45, -0.98).symbol(Symbol::CAR).tone(88.5).build().unwrap_err();
    assert!(tone_alone.to_string().contains("frequency"), "{tone_alone}");
}

#[test]
fn what_the_spec_forbids_is_refused_when_built() {
    assert!(me().message("G3NRW", "Hi").id("TOOLONG").build().is_err());
    assert!(me().object("NAMEISTOOLONG").at(51.45, -0.98).symbol(Symbol::REPEATER).timestamp(Timestamp::dhm(1, 2, 3)).build().is_err());
    let phg = Phg { power: 5, height: 3, gain: 6, directivity: 0, beacons_per_hour: None };
    assert!(me().position(51.4543, -0.9781).symbol(Symbol::CAR).compressed().phg(phg).build().is_err());
    assert!(me().telemetry(1).analog(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]).build().is_err());
}

#[test]
fn named_symbols_and_overlays() {
    assert_eq!(Symbol::CAR, Symbol::new('/', '>'));
    assert_eq!(Symbol::GATEWAY.with_overlay('I'), Some(Symbol::new('I', '&')));
    assert_eq!(Symbol::CAR.with_overlay('I'), None);
    assert_eq!(Symbol::GATEWAY.with_overlay('a'), None);
}
